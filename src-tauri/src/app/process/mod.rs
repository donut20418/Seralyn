pub mod json_rpc;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::app::error::{AppError, Result};

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

enum ProcessControlMsg {
    Shutdown {
        timeout: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
    Kill,
}

/// A managed background process that decouples stdin writing from stdout/stderr reading
/// and uses an actor-based control loop to prevent deadlocks on process shutdown/exit.
pub struct ManagedProcess {
    ctrl_tx: mpsc::Sender<ProcessControlMsg>,
    stdin_tx: mpsc::Sender<String>,
    stdout_rx: Arc<Mutex<mpsc::Receiver<String>>>,
    stderr_rx: Arc<Mutex<mpsc::Receiver<String>>>,
    alive: Arc<AtomicBool>,
    pid: Option<u32>,
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
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

    let mut child = cmd.spawn().map_err(|e| {
        tracing::error!("Failed to spawn process {}: {}", config.executable, e);
        e
    })?;

    let pid = child.id();

    let mut stdin = child.stdin.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdin")
    })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdout")
    })?;

    let stderr = child.stderr.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stderr")
    })?;

    // Dedicated stdin writing channel: writes never block reading
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(1000);
    let name_stdin = config.executable.clone();
    tokio::spawn(async move {
        while let Some(line) = stdin_rx.recv().await {
            let mut data = line;
            if !data.ends_with('\n') {
                data.push('\n');
            }
            if let Err(e) = stdin.write_all(data.as_bytes()).await {
                tracing::warn!("[{}] Stdin write error: {}", name_stdin, e);
                break;
            }
            if let Err(e) = stdin.flush().await {
                tracing::warn!("[{}] Stdin flush error: {}", name_stdin, e);
                break;
            }
        }
    });

    let (stdout_tx, stdout_rx) = mpsc::channel(1000);
    let (stderr_tx, stderr_rx) = mpsc::channel(1000);

    let alive = Arc::new(AtomicBool::new(true));

    let name_stdout = config.executable.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if stdout_tx.send(line).await.is_err() {
                break;
            }
        }
        tracing::debug!("Stdout stream ended for {}", name_stdout);
    });

    let name_stderr = config.executable.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::warn!("[{}] stderr: {}", name_stderr, line);
            if stderr_tx.send(line).await.is_err() {
                break;
            }
        }
        tracing::debug!("Stderr stream ended for {}", name_stderr);
    });

    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<ProcessControlMsg>(10);
    let alive_actor = alive.clone();
    let name_actor = config.executable.clone();
    let pid_actor = pid;

    // Process lifecycle actor: owns child exclusively.
    // Handles wait and external commands (Shutdown, Kill) without lock contention.
    tokio::spawn(async move {
        let mut child = child;
        tokio::select! {
            exit_res = child.wait() => {
                match exit_res {
                    Ok(status) => {
                        tracing::info!("[{}] Process exited with status: {}", name_actor, status);
                    }
                    Err(e) => {
                        tracing::warn!("[{}] Process wait error: {}", name_actor, e);
                    }
                }
                alive_actor.store(false, Ordering::SeqCst);
            }
            Some(cmd) = ctrl_rx.recv() => {
                match cmd {
                    ProcessControlMsg::Kill => {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                        alive_actor.store(false, Ordering::SeqCst);
                    }
                    ProcessControlMsg::Shutdown { timeout: timeout_duration, reply } => {
                        if let Ok(Some(_)) = child.try_wait() {
                            alive_actor.store(false, Ordering::SeqCst);
                            let _ = reply.send(Ok(()));
                            return;
                        }

                        let _ = child.start_kill();
                        let res = match timeout(timeout_duration, child.wait()).await {
                            Ok(Ok(status)) => {
                                tracing::info!("[{}] Process shut down cleanly: {}", name_actor, status);
                                Ok(())
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("[{}] Process wait error during shutdown: {}", name_actor, e);
                                Err(AppError::Process(e.to_string()))
                            }
                            Err(_) => {
                                tracing::warn!("[{}] Process shutdown timeout (PID: {:?})", name_actor, pid_actor);
                                let _ = child.kill().await;
                                Err(AppError::Process("Shutdown timeout".to_string()))
                            }
                        };
                        alive_actor.store(false, Ordering::SeqCst);
                        let _ = reply.send(res);
                    }
                }
            }
        }
    });

    // Fast startup check: ensure process didn't immediately crash on spawn
    let startup_check = config.startup_timeout.min(Duration::from_millis(50));
    tokio::time::sleep(startup_check).await;
    if !alive.load(Ordering::SeqCst) {
        return Err(AppError::Process(format!(
            "Process '{}' exited immediately after launch",
            config.executable
        )));
    }

    Ok(ManagedProcess {
        ctrl_tx,
        stdin_tx,
        stdout_rx: Arc::new(Mutex::new(stdout_rx)),
        stderr_rx: Arc::new(Mutex::new(stderr_rx)),
        alive,
        pid,
    })
}

impl ManagedProcess {
    /// Non-blocking asynchronous line write to stdin.
    /// Can be called concurrently without locking the stdout reader.
    pub async fn send_line(&self, line: &str) -> Result<()> {
        self.stdin_tx.send(line.to_string()).await.map_err(|_| {
            AppError::Process("Failed to send line to stdin (channel closed)".to_string())
        })
    }

    /// Receives the next line of stdout
    pub async fn recv_stdout_line(&self) -> Result<Option<String>> {
        let mut guard = self.stdout_rx.lock().await;
        Ok(guard.recv().await)
    }

    /// Non-blockingly checks for the next line of stderr
    pub fn try_recv_stderr(&self) -> Option<String> {
        if let Ok(mut guard) = self.stderr_rx.try_lock() {
            guard.try_recv().ok()
        } else {
            None
        }
    }

    /// Checks if the child process is still running
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Gracefully attempts to shut down the process, falling back to a hard kill on timeout.
    /// Uses actor channel to eliminate any mutex lock contention.
    pub async fn shutdown(&self, timeout_duration: Duration) -> Result<()> {
        if !self.is_alive() {
            return Ok(());
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .ctrl_tx
            .send(ProcessControlMsg::Shutdown {
                timeout: timeout_duration,
                reply: reply_tx,
            })
            .await
            .is_err()
        {
            self.alive.store(false, Ordering::SeqCst);
            return Ok(());
        }

        match reply_rx.await {
            Ok(res) => res,
            Err(_) => Ok(()),
        }
    }

    /// Kills the child process immediately via the actor channel
    pub async fn kill(&self) -> Result<()> {
        if !self.is_alive() {
            return Ok(());
        }
        let _ = self.ctrl_tx.send(ProcessControlMsg::Kill).await;
        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Retrieves the system PID of the managed process
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if self.alive.load(Ordering::SeqCst) {
            let _ = self.ctrl_tx.try_send(ProcessControlMsg::Kill);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_executable() {
        #[cfg(windows)]
        let exe = "cmd";
        #[cfg(not(windows))]
        let exe = "sh";

        let result = detect_executable(exe).await;
        assert!(result.is_ok(), "Should find standard shell executable on platform");

        let non_existent = detect_executable("non_existent_executable_12345").await;
        assert!(non_existent.is_err(), "Should fail for missing executable");
    }
}
