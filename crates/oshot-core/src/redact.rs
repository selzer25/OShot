//! Finds secrets in recognised text so they can be pixelated automatically.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretKind {
    Email,
    ApiKey,
    Token,
    CardNumber,
}

impl SecretKind {
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::Email => "email",
            SecretKind::ApiKey => "API key",
            SecretKind::Token => "token",
            SecretKind::CardNumber => "card number",
        }
    }
}

static PATTERNS: LazyLock<Vec<(Regex, SecretKind)>> = LazyLock::new(|| {
    let p = |re: &str, k| (Regex::new(re).expect("valid pattern"), k);
    vec![
        p(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}", SecretKind::Email),
        p(r"\bAKIA[0-9A-Z]{16}\b", SecretKind::ApiKey),
        p(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b", SecretKind::ApiKey),
        p(r"\bgithub_pat_[A-Za-z0-9_]{30,}\b", SecretKind::ApiKey),
        p(r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_\-]{20,}", SecretKind::ApiKey),
        p(r"\bxox[abprs]-[A-Za-z0-9\-]{10,}", SecretKind::ApiKey),
        p(r"\bAIza[0-9A-Za-z_\-]{35}\b", SecretKind::ApiKey),
        p(
            r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
            SecretKind::Token,
        ),
        p(r"\b[A-Fa-f0-9]{32,}\b", SecretKind::Token),
        p(r"\b(?:\d[ \-]?){12,18}\d\b", SecretKind::CardNumber),
    ]
});

/// Byte ranges of likely secrets in `text`, sorted and non-overlapping.
pub fn find_secrets(text: &str) -> Vec<(Range<usize>, SecretKind)> {
    let mut found: Vec<(Range<usize>, SecretKind)> = Vec::new();
    for (re, kind) in PATTERNS.iter() {
        for m in re.find_iter(text) {
            if *kind == SecretKind::CardNumber && !luhn(m.as_str()) {
                continue;
            }
            found.push((m.range(), *kind));
        }
    }
    found.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let mut out: Vec<(Range<usize>, SecretKind)> = Vec::new();
    for (r, k) in found {
        if out.last().is_some_and(|(last, _)| r.start < last.end) {
            continue;
        }
        out.push((r, k));
    }
    out
}

fn luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                if d * 2 > 9 { d * 2 - 9 } else { d * 2 }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(String, SecretKind)> {
        find_secrets(text)
            .into_iter()
            .map(|(r, k)| (text[r].to_string(), k))
            .collect()
    }

    #[test]
    fn finds_common_secrets() {
        let text = "mail max@example.com key AKIAABCDEFGHIJKLMNOP and sk-ant-abcdefghijklmnopqrstuvwx";
        let k = kinds(text);
        assert_eq!(k[0], ("max@example.com".into(), SecretKind::Email));
        assert_eq!(k[1], ("AKIAABCDEFGHIJKLMNOP".into(), SecretKind::ApiKey));
        assert_eq!(k[2].1, SecretKind::ApiKey);
    }

    #[test]
    fn card_numbers_need_luhn() {
        assert_eq!(kinds("card 4242 4242 4242 4242 ok")[0].1, SecretKind::CardNumber);
        assert!(kinds("order 1234 5678 9012 3456").is_empty());
    }

    #[test]
    fn plain_text_is_left_alone() {
        assert!(kinds("Settings General Shortcuts Version 0.1.0").is_empty());
    }
}
