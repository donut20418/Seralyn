use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::app::error::{AppError, Result};

pub const MAX_SINGLE_FILE_SIZE: usize = 25 * 1024 * 1024; // 25 MB
pub const MAX_ATTACHMENTS_PER_TURN: usize = 10;
pub const MAX_AGGREGATE_SIZE_PER_TURN: usize = 50 * 1024 * 1024; // 50 MB

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Image,
    Text,
    Binary,
}

impl std::fmt::Display for AttachmentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachmentKind::Image => write!(f, "image"),
            AttachmentKind::Text => write!(f, "text"),
            AttachmentKind::Binary => write!(f, "binary"),
        }
    }
}

impl std::str::FromStr for AttachmentKind {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "image" => Ok(AttachmentKind::Image),
            "text" => Ok(AttachmentKind::Text),
            "binary" => Ok(AttachmentKind::Binary),
            other => Err(AppError::InvalidInput(format!("Invalid attachment kind: {}", other))),
        }
    }
}

/// Computes the hex-encoded SHA-256 hash of the given byte slice.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Standard RFC 4648 Base64 encoder for image payloads without external dependency churn.
pub fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        result.push(CHARSET[((n >> 18) & 63) as usize] as char);
        result.push(CHARSET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARSET[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARSET[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Classifies MIME type and `AttachmentKind` using file extension, magic bytes, and client hints.
pub fn classify_mime_and_kind(
    file_name: &str,
    data: &[u8],
    client_mime: Option<&str>,
) -> (String, AttachmentKind) {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // 1. Magic byte checks for images
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) || ext == "png" {
        return ("image/png".to_string(), AttachmentKind::Image);
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) || ext == "jpg" || ext == "jpeg" {
        return ("image/jpeg".to_string(), AttachmentKind::Image);
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") || ext == "gif" {
        return ("image/gif".to_string(), AttachmentKind::Image);
    }
    if (data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP") || ext == "webp" {
        return ("image/webp".to_string(), AttachmentKind::Image);
    }
    if ext == "svg" || (data.starts_with(b"<?xml") && String::from_utf8_lossy(data).contains("<svg")) {
        return ("image/svg+xml".to_string(), AttachmentKind::Image);
    }
    if ext == "bmp" || data.starts_with(b"BM") {
        return ("image/bmp".to_string(), AttachmentKind::Image);
    }

    // 2. Client MIME check for images
    if let Some(cm) = client_mime {
        let cm_lower = cm.trim().to_lowercase();
        if cm_lower.starts_with("image/") {
            return (cm_lower, AttachmentKind::Image);
        }
    }

    // 3. Known text file extensions
    let text_extensions = [
        "txt", "md", "rs", "ts", "tsx", "js", "jsx", "json", "toml", "yaml", "yml",
        "html", "htm", "css", "c", "cpp", "h", "hpp", "py", "sh", "sql", "xml", "csv",
        "log", "diff", "patch", "env", "ini", "java", "go", "rb", "php",
    ];
    if text_extensions.contains(&ext.as_str()) {
        let mime = match ext.as_str() {
            "json" => "application/json",
            "html" | "htm" => "text/html",
            "css" => "text/css",
            "csv" => "text/csv",
            "xml" => "application/xml",
            _ => "text/plain",
        };
        return (mime.to_string(), AttachmentKind::Text);
    }

    // 4. Client MIME check for text
    if let Some(cm) = client_mime {
        let cm_lower = cm.trim().to_lowercase();
        if cm_lower.starts_with("text/") || cm_lower == "application/json" {
            return (cm_lower, AttachmentKind::Text);
        }
    }

    // 5. UTF-8 fallback heuristic for text
    if !data.is_empty() && !data.contains(&0) && std::str::from_utf8(data).is_ok() {
        return ("text/plain".to_string(), AttachmentKind::Text);
    }

    // 6. Default fallback to binary
    let mime = client_mime
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    (mime, AttachmentKind::Binary)
}

/// Sanitizes an input filename to a safe leaf without control characters, path separators, or directory traversal.
pub fn sanitize_file_name(file_name: &str) -> String {
    let raw_leaf = Path::new(file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment.bin");

    let sanitized: String = raw_leaf
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-' || *c == ' ')
        .collect();

    let safe = sanitized.trim();
    if safe.is_empty() {
        "attachment.bin".to_string()
    } else {
        safe.to_string()
    }
}

/// Returns the base directory for managed attachments: `%LOCALAPPDATA%/Seralyn/attachments`.
pub fn get_attachments_base_dir() -> Result<PathBuf> {
    let mut root_path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    root_path.push("Seralyn");
    root_path.push("attachments");
    std::fs::create_dir_all(&root_path)?;
    Ok(root_path.canonicalize()?)
}

/// Returns and verifies the directory for a specific conversation:
/// `%LOCALAPPDATA%/Seralyn/attachments/<conversation_id>/`.
/// Enforces UUID format, symlink/junction defense, and strict containment.
pub fn get_conversation_attachment_dir(conversation_id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(conversation_id).map_err(|_| {
        AppError::InvalidInput("Invalid conversation ID: must be a valid UUID".to_string())
    })?;

    let base = get_attachments_base_dir()?;
    let conv_dir = base.join(conversation_id);
    std::fs::create_dir_all(&conv_dir)?;
    let canon_conv_dir = conv_dir.canonicalize()?;

    // Strict containment verification: canon_conv_dir must be a direct child of base
    if !canon_conv_dir.starts_with(&base) || canon_conv_dir.parent() != Some(base.as_path()) {
        return Err(AppError::InvalidInput(
            "Path traversal or junction escape in conversation attachment directory".to_string(),
        ));
    }

    Ok(canon_conv_dir)
}

/// Resolves and verifies that a stored attachment file exists and is strictly contained
/// within the conversation's managed attachment directory.
pub fn resolve_and_verify_managed_path(conversation_id: &str, stored_name: &str) -> Result<PathBuf> {
    let conv_dir = get_conversation_attachment_dir(conversation_id)?;
    let dest_path = conv_dir.join(stored_name);

    if !dest_path.exists() {
        return Err(AppError::InvalidInput(format!(
            "Attachment file not found: {}",
            stored_name
        )));
    }

    let canon_dest = dest_path.canonicalize()?;
    if !canon_dest.starts_with(&conv_dir) || canon_dest.parent() != Some(conv_dir.as_path()) {
        return Err(AppError::InvalidInput(
            "Path traversal or junction escape detected in attachment file path".to_string(),
        ));
    }

    Ok(canon_dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_sha256() {
        let hash = compute_sha256(b"hello world");
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_classify_mime_and_kind() {
        let (mime, kind) = classify_mime_and_kind("photo.PNG", b"sample data", None);
        assert_eq!(mime, "image/png");
        assert_eq!(kind, AttachmentKind::Image);

        let (mime, kind) = classify_mime_and_kind("data.txt", b"plain text content", None);
        assert_eq!(mime, "text/plain");
        assert_eq!(kind, AttachmentKind::Text);

        let (mime, kind) = classify_mime_and_kind("archive.bin", &[0, 1, 2, 3], None);
        assert_eq!(mime, "application/octet-stream");
        assert_eq!(kind, AttachmentKind::Binary);
    }

    #[test]
    fn test_sanitize_file_name() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("..\\..\\secret.txt"), "secret.txt");
        assert_eq!(sanitize_file_name("my photo (1).png"), "my photo 1.png");
        assert_eq!(sanitize_file_name("///"), "attachment.bin");
    }
}
