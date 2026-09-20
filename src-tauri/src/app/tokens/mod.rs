use serde::{Deserialize, Serialize};
use crate::app::events::TokenConfidence;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub confidence: TokenConfidence,
}

impl TokenUsage {
    pub fn remaining_context(&self) -> Option<u64> {
        if let (Some(window), Some(context)) = (self.context_window, self.context_tokens) {
            Some(window.saturating_sub(context))
        } else {
            None
        }
    }

    pub fn context_pressure(&self) -> Option<f64> {
        if let (Some(window), Some(context)) = (self.context_window, self.context_tokens) {
            if window == 0 {
                return Some(1.0);
            }
            Some((context as f64) / (window as f64))
        } else {
            None
        }
    }
}

pub struct TokenManager;

impl TokenManager {
    pub fn new() -> Self {
        Self
    }

    pub fn estimate_tokens(text: &str) -> u64 {
        if text.is_empty() {
            return 0;
        }

        let mut ascii_count: u64 = 0;
        let mut non_ascii_count: u64 = 0;

        for c in text.chars() {
            if c.is_ascii() {
                ascii_count += 1;
            } else {
                non_ascii_count += 1;
            }
        }

        let ascii_tokens = if ascii_count > 0 {
            (ascii_count + 3) / 4
        } else {
            0
        };

        // Non-ASCII (Thai, CJK, Emoji, etc.): conservative estimate of 1 token per character
        let non_ascii_tokens = non_ascii_count;

        (ascii_tokens + non_ascii_tokens).max(1)
    }
}

impl Default for TokenManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(TokenManager::estimate_tokens(""), 0);
        assert_eq!(TokenManager::estimate_tokens("1234"), 1);
        assert_eq!(TokenManager::estimate_tokens("12345678"), 2);
        assert_eq!(TokenManager::estimate_tokens("hello world!"), 3);
        // Single characters and short words must not be 0
        assert_eq!(TokenManager::estimate_tokens("a"), 1);
        assert_eq!(TokenManager::estimate_tokens("abc"), 1);
        assert_eq!(TokenManager::estimate_tokens("ก"), 1);
        // Thai text
        assert_eq!(TokenManager::estimate_tokens("สวัสดี"), 6);
        // CJK text
        assert_eq!(TokenManager::estimate_tokens("你好"), 2);
    }

    #[test]
    fn test_context_pressure() {
        let usage = TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            context_tokens: Some(500),
            context_window: Some(1000),
            confidence: TokenConfidence::Exact,
        };
        
        assert_eq!(usage.remaining_context(), Some(500));
        assert_eq!(usage.context_pressure(), Some(0.5));
    }
}
