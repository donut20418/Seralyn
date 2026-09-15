use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::app::error::Result;

/// Information about a detected executable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableInfo {
    pub name: String,
    pub path: PathBuf,
    pub version: Option<String>,
}

/// Configuration for spawning a managed process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnConfig {
    pub executable: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub startup_timeout: Duration,
}

/// A managed background process that provides line-by-line stdout/stderr communication
pub struct ManagedProcess {
    child: Child,
    stdin: ChildStdin,
    #[allow(dead_code)]
    stdout_task: tokio::task::JoinHandle<()>,
    #[allow(dead_code)]
    stderr_task: tokio::task::JoinHandle<()>,
    stdout_rx: mpsc::Receiver<String>,
    stderr_rx: mpsc::Receiver<String>,
    alive: Arc<AtomicBool>,
}

/// Detects an executable by name and optionally gets its version
pub async fn detect_executable(name: &str) -> Result<ExecutableInfo> {
    let path = which::which(name).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Executable '{}' not found: {}", name, e),
        )
    })?;

    let mut cmd = Command::new(&path);
    cmd.arg("--version");

    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

    let version = match timeout(Duration::from_secs(5), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => {
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            Some(stdout_str.trim().to_string())
        }
        _ => None,
    };

    Ok(ExecutableInfo {
        name: name.to_string(),
        path,
        version,
    })
}

/// Spawns a new managed process with the given configuration
pub async fn spawn(config: SpawnConfig) -> Result<ManagedProcess> {
    tracing::info!(
        "Spawning process: {} with args: {:?}",
        config.executable,
        config.args
    );

    let mut cmd = Command::new(&config.executable);
    cmd.args(&config.args);

    if let Some(cwd) = &config.working_dir {
        cmd.current_dir(cwd);
    }

    cmd.envs(&config.env);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    let mut child = cmd.spawn().map_err(|e| {
        tracing::error!("Failed to spawn process {}: {}", config.executable, e);
        e
    })?;

    let stdin = child.stdin.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdin")
    })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdout")
    })?;

    let stderr = child.stderr.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stderr")
    })?;

    let (stdout_tx, stdout_rx) = mpsc::channel(1000);
    let (stderr_tx, stderr_rx) = mpsc::channel(1000);

    let alive = Arc::new(AtomicBool::new(true));

    let alive_stdout = alive.clone();
    let name_stdout = config.executable.clone();
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if stdout_tx.send(line).await.is_err() {
                break;
            }
        }
        alive_stdout.store(false, Ordering::SeqCst);
        tracing::info!("Stdout closed for {}", name_stdout);
    });

    let alive_stderr = alive.clone();
    let name_stderr = config.executable.clone();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::warn!("[{}] stderr: {}", name_stderr, line);
            if stderr_tx.send(line).await.is_err() {
                break;
            }
        }
        alive_stderr.store(false, Ordering::SeqCst);
        tracing::info!("Stderr closed for {}", name_stderr);
    });

    Ok(ManagedProcess {
        child,
        stdin,
        stdout_task,
        stderr_task,
        stdout_rx,
        stderr_rx,
        alive,
    })
}

impl ManagedProcess {
    /// Sends a line of text to the process's stdin
    pub async fn send_line(&mut self, line: &str) -> Result<()> {
        let mut data = String::with_capacity(line.len() + 1);
        data.push_str(line);
        data.push('\n');

        self.stdin.write_all(data.as_bytes()).await.map_err(|e| {
            tracing::error!("Failed to write to stdin: {}", e);
            e
        })?;
        self.stdin.flush().await.map_err(|e| {
            tracing::error!("Failed to flush stdin: {}", e);
            e
        })?;

        Ok(())
    }

    /// Receives the next line of stdout, blocking until available
    pub async fn recv_stdout_line(&mut self) -> Result<Option<String>> {
        Ok(self.stdout_rx.recv().await)
    }

    /// Non-blockingly checks for the next line of stderr
    pub fn try_recv_stderr(&mut self) -> Option<String> {
        self.stderr_rx.try_recv().ok()
    }

    /// Checks if the process and its communication tasks are still alive
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Gracefully attempts to shut down the process, falling back to a hard kill on timeout
    pub async fn shutdown(&mut self, timeout_duration: Duration) -> Result<()> {
        if !self.is_alive() {
            return Ok(());
        }

        if let Ok(Some(_)) = self.child.try_wait() {
            self.alive.store(false, Ordering::SeqCst);
            return Ok(());
        }

        if timeout(timeout_duration, self.child.wait()).await.is_err() {
            tracing::info!(
                "Process shutdown timeout, forcing kill (PID: {:?})",
                self.pid()
            );
            let _ = self.child.kill().await;
        }

        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Retrieves the system PID of the managed process
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if self.alive.load(Ordering::SeqCst) {
            let _ = self.child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_executable() {
        // 'cmd' is virtually guaranteed to exist on Windows machines.
        let result = detect_executable("cmd").await;
        assert!(result.is_ok(), "Should find cmd executable on Windows");

        if let Ok(info) = result {
            assert_eq!(info.name, "cmd");
            assert!(info.path.exists());
        }
    }
}
