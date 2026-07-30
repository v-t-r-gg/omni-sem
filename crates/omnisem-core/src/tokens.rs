//! Deterministic heuristic token estimation for context packing.

/// Fixed overhead charged once per retrieval response.
pub const RESPONSE_OVERHEAD_TOKENS: usize = 16;
/// Fixed overhead charged once per included hit.
pub const RESULT_OVERHEAD_TOKENS: usize = 8;
/// Hard byte cap applied while packing (UTF-8 bytes of returned text fields).
pub const HARD_BYTE_CAP: usize = 48_000;
/// Maximum UTF-8 bytes retained for a single hit excerpt.
pub const MAX_HIT_TEXT_BYTES: usize = 4_000;

/// Estimates token usage for budgeting. Values are conservative and not model-exact.
pub trait TokenEstimator: Send + Sync {
    /// Returns a non-negative estimated token count for `text`.
    fn estimate(&self, text: &str) -> usize;
}

/// Character-based estimator:
///
/// ```text
/// tokens = ceil(char_count / 3)   for non-empty text
/// tokens = 0                      for empty text
/// ```
///
/// The divisor 3 is intentionally conservative relative to common English
/// subword ratios so packing under-fills rather than overfills model windows.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicTokenEstimator;

impl TokenEstimator for HeuristicTokenEstimator {
    fn estimate(&self, text: &str) -> usize {
        let chars = text.chars().count();
        if chars == 0 { 0 } else { chars.div_ceil(3) }
    }
}

/// Estimates total tokens for a full retrieval response shape.
#[must_use]
pub fn estimate_response_tokens(
    estimator: &dyn TokenEstimator,
    query: &str,
    hit_texts: &[&str],
) -> usize {
    let mut total = RESPONSE_OVERHEAD_TOKENS + estimator.estimate(query);
    for text in hit_texts {
        total = total.saturating_add(RESULT_OVERHEAD_TOKENS + estimator.estimate(text));
    }
    total
}

/// Truncates to at most `max_bytes` on a UTF-8 character boundary.
#[must_use]
pub fn truncate_utf8(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_owned();
    if !out.ends_with('…') {
        out.push('…');
    }
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimator_is_stable_and_conservative() {
        let estimator = HeuristicTokenEstimator;
        assert_eq!(estimator.estimate(""), 0);
        assert_eq!(estimator.estimate("abc"), 1);
        assert_eq!(estimator.estimate("abcdef"), 2);
        assert_eq!(estimator.estimate("日本語"), 1);
        assert_eq!(estimator.estimate(&"x".repeat(10)), 4);
    }

    #[test]
    fn truncate_preserves_utf8() {
        let (text, truncated) = truncate_utf8("aé🙂cd", 4);
        assert!(truncated);
        assert!(text.is_char_boundary(text.len()));
        assert!(text.ends_with('…'));
    }
}
