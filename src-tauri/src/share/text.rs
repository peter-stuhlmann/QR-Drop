//! Classifying shared texts: link, secret or plain text.

use super::ShareSource;

/// Larger texts are rejected (nobody copies more by hand to a phone).
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// Longer URLs make the QR code too dense; they are served as text instead.
const MAX_DIRECT_URL: usize = 1200;

/// `http(s)://…` without spaces, short enough for a readable QR code.
fn is_direct_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && s.len() > "https://".len() + 2
        && s.len() <= MAX_DIRECT_URL
        && !s.chars().any(char::is_whitespace)
}

/// First line of a text, shortened for the window header.
pub fn text_preview(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    if line.chars().count() <= max_chars {
        line.to_string()
    } else {
        let cut: String = line.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

/// Heuristic for passwords, PINs-with-letters and API tokens: one "word"
/// without spaces that mixes character classes. False positives only mean
/// the text is masked and expires sooner -- the safe direction.
pub fn looks_like_secret(s: &str) -> bool {
    let len = s.chars().count();
    if !(8..=128).contains(&len) || s.chars().any(char::is_whitespace) {
        return false;
    }
    // URLs and e-mail addresses are not secrets.
    if s.contains("://") || (s.contains('@') && s.contains('.') && !s.starts_with('@')) {
        return false;
    }
    let lower = s.chars().any(|c| c.is_lowercase());
    let upper = s.chars().any(|c| c.is_uppercase());
    let digit = s.chars().any(|c| c.is_ascii_digit());
    let other = s.chars().any(|c| !c.is_alphanumeric());
    let classes = [lower, upper, digit, other].iter().filter(|b| **b).count();
    classes >= 3 || (classes == 2 && digit && len >= 20)
}

impl ShareSource {
    /// Classifies a text: link, secret or plain text. `None` for empty or
    /// oversized input.
    pub fn from_text(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() || text.len() > MAX_TEXT_BYTES {
            return None;
        }
        if is_direct_url(trimmed) {
            return Some(ShareSource::Url {
                url: trimmed.to_string(),
            });
        }
        let secret = looks_like_secret(trimmed);
        Some(ShareSource::Text {
            // Secrets without the stray newline that copying often adds.
            text: if secret {
                trimmed.to_string()
            } else {
                text.trim_end_matches(['\r', '\n']).to_string()
            },
            secret,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::FileKind;

    #[test]
    fn classifies_links_secrets_and_text() {
        let url = ShareSource::from_text("  https://example.com/a?b=1  ").unwrap();
        assert!(matches!(url, ShareSource::Url { ref url } if url == "https://example.com/a?b=1"));
        assert_eq!(url.kind(), FileKind::Link);

        for secret in [
            "Tr0ub4dor&3",
            "hunter2!Xy",
            "sk-proj-4f9a8b7c6d5e4f3a2b1c",
            "P@ssw0rd",
        ] {
            let s = ShareSource::from_text(secret).unwrap();
            assert!(s.is_secret(), "{secret}");
            assert_eq!(s.kind(), FileKind::Secret);
        }
        for plain in [
            "Einkaufsliste: Milch, Brot",
            "hallo",
            "max.mustermann@example.com",
            "12345678",
            "www example",
            "https://example.com mit Leerzeichen",
        ] {
            let s = ShareSource::from_text(plain).unwrap();
            assert!(!s.is_secret(), "{plain}");
            assert!(matches!(s, ShareSource::Text { .. }), "{plain}");
        }
        assert!(ShareSource::from_text("   ").is_none());
        assert!(ShareSource::from_text(&"x".repeat(MAX_TEXT_BYTES + 1)).is_none());
        let long_url = format!("https://example.com/{}", "a".repeat(2000));
        assert!(matches!(
            ShareSource::from_text(&long_url),
            Some(ShareSource::Text { .. })
        ));
    }

    #[test]
    fn preview_takes_first_non_empty_line() {
        assert_eq!(text_preview("\n  Hallo Welt  \nzweite", 80), "Hallo Welt");
        assert_eq!(text_preview("abcdef", 3), "abc…");
        assert_eq!(text_preview("", 3), "");
    }
}
