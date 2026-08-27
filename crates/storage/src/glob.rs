//! Minimal Redis-style glob matcher: supports `*`, `?`, `[abc]`, `[^abc]`,
//! `[a-z]` character ranges, and `\` escaping. Pure byte-level, no regex
//! crate involved.

pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    match_inner(pattern, text)
}

fn match_inner(mut pattern: &[u8], mut text: &[u8]) -> bool {
    while let Some(&pc) = pattern.first() {
        match pc {
            b'*' => {
                // Collapse consecutive '*'.
                while pattern.first() == Some(&b'*') {
                    pattern = &pattern[1..];
                }
                if pattern.is_empty() {
                    return true;
                }
                for i in 0..=text.len() {
                    if match_inner(pattern, &text[i..]) {
                        return true;
                    }
                }
                return false;
            }
            b'?' => {
                if text.is_empty() {
                    return false;
                }
                text = &text[1..];
                pattern = &pattern[1..];
            }
            b'[' => {
                let (matched, rest, consumed_text) = match_class(&pattern[1..], text);
                if !matched {
                    return false;
                }
                pattern = rest;
                text = consumed_text;
            }
            b'\\' if pattern.len() > 1 => {
                if text.first() != Some(&pattern[1]) {
                    return false;
                }
                pattern = &pattern[2..];
                text = &text[1..];
            }
            c => {
                if text.first() != Some(&c) {
                    return false;
                }
                pattern = &pattern[1..];
                text = &text[1..];
            }
        }
    }
    text.is_empty()
}

/// Parses a `[...]` class starting just after the `[`. Returns
/// (matched_first_char, remaining_pattern_after_class, remaining_text).
fn match_class<'a, 'b>(mut pattern: &'a [u8], text: &'b [u8]) -> (bool, &'a [u8], &'b [u8]) {
    let ch = match text.first() {
        Some(&c) => c,
        None => {
            // No text left; still need to skip the class in the pattern to
            // keep parsing sane, but the match fails regardless.
            let mut p = pattern;
            let negate = p.first() == Some(&b'^');
            if negate {
                p = &p[1..];
            }
            while let Some(&c) = p.first() {
                p = &p[1..];
                if c == b']' {
                    break;
                }
                if c == b'\\' && !p.is_empty() {
                    p = &p[1..];
                }
            }
            return (false, p, text);
        }
    };

    let negate = pattern.first() == Some(&b'^');
    if negate {
        pattern = &pattern[1..];
    }
    let mut found = false;
    loop {
        match pattern.first() {
            None => break,
            Some(&b']') => {
                pattern = &pattern[1..];
                break;
            }
            Some(&b'\\') if pattern.len() > 1 => {
                if pattern[1] == ch {
                    found = true;
                }
                pattern = &pattern[2..];
            }
            Some(&lo) => {
                if pattern.get(1) == Some(&b'-') && pattern.len() > 2 && pattern[2] != b']' {
                    let hi = pattern[2];
                    if lo <= ch && ch <= hi {
                        found = true;
                    }
                    pattern = &pattern[3..];
                } else {
                    if lo == ch {
                        found = true;
                    }
                    pattern = &pattern[1..];
                }
            }
        }
    }
    let matched = found != negate;
    (matched, pattern, &text[1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal() {
        assert!(glob_match(b"hello", b"hello"));
        assert!(!glob_match(b"hello", b"world"));
    }

    #[test]
    fn star() {
        assert!(glob_match(b"user:*", b"user:123"));
        assert!(glob_match(b"*", b"anything"));
        assert!(glob_match(b"a*b*c", b"aXbYc"));
        assert!(!glob_match(b"a*b*c", b"aXbYd"));
    }

    #[test]
    fn question_mark() {
        assert!(glob_match(b"h?llo", b"hello"));
        assert!(!glob_match(b"h?llo", b"hllo"));
    }

    #[test]
    fn char_class() {
        assert!(glob_match(b"[abc]ey", b"bey"));
        assert!(!glob_match(b"[abc]ey", b"dey"));
        assert!(glob_match(b"[a-z]ey", b"key"));
        assert!(glob_match(b"[^a-c]ey", b"key"));
        assert!(!glob_match(b"[^a-c]ey", b"aey"));
    }
}
