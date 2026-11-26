//! Remove secrets and home directory paths from the text of a public
//! `fallow feedback` issue.
//!
//! The scan is a best effort for common shapes: access tokens with a known
//! prefix, JWTs, `Bearer` values, passwords in URLs and PEM private keys. It
//! cannot find every secret, so the command and the MCP tool also tell the
//! caller not to put secrets in the report.

const REDACTED: &str = "[redacted]";
const PRIVATE_KEY_REDACTED: &str = "[redacted private key]";
/// Prefixes of access tokens. A match also needs a tail of
/// [`MIN_TOKEN_TAIL`] token characters with at least one digit, so that words
/// such as `npm_config_user_agent` stay.
const TOKEN_PREFIXES: &[&str] = &[
    "github_pat_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "glpat-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "sk-",
    "xoxa-",
    "xoxb-",
    "xoxp-",
    "xoxr-",
    "xoxs-",
    "xapp-",
    "AKIA",
    "ASIA",
    "AIza",
    "npm_",
    "eyJ",
];
/// A JWT has three dot-separated parts.
const JWT_PREFIX: &str = "eyJ";
const BEARER: &str = "bearer ";
const MIN_TOKEN_TAIL: usize = 16;
const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_PRIVATE_KEY_END: &str = "PRIVATE KEY-----";
const PEM_END: &str = "-----END ";
const URL_SCHEME_END: &str = "://";

/// Remove secrets, then replace the home directory with `~`.
pub fn redact(text: &str, home: Option<&str>) -> String {
    let text = redact_private_keys(text);
    let text = redact_url_passwords(&text);
    let text = redact_tokens(&text);
    match home {
        Some(home) => replace_home(&text, home),
        None => text,
    }
}

fn redact_private_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(PEM_BEGIN) {
        let header_end = rest[start..].find('\n').map_or(rest.len(), |i| start + i);
        if !rest[start..header_end].contains(PEM_PRIVATE_KEY_END) {
            out.push_str(&rest[..start + PEM_BEGIN.len()]);
            rest = &rest[start + PEM_BEGIN.len()..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str(PRIVATE_KEY_REDACTED);
        // Without an END line, the rest of the text is key material.
        rest = rest[start..]
            .find(PEM_END)
            .and_then(|end| {
                let end = start + end;
                rest[end..]
                    .find(PEM_PRIVATE_KEY_END)
                    .map(|tail| end + tail + PEM_PRIVATE_KEY_END.len())
            })
            .map_or("", |stop| &rest[stop..]);
    }
    out.push_str(rest);
    out
}

/// `https://user:secret@host` becomes `https://[redacted]@host`.
fn redact_url_passwords(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(URL_SCHEME_END) {
        let after = index + URL_SCHEME_END.len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        let authority_len = rest
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '?' | '#'))
            .unwrap_or(rest.len());
        if let Some(at) = rest[..authority_len].rfind('@')
            && rest[..at].contains(':')
        {
            out.push_str(REDACTED);
            rest = &rest[at..];
        }
    }
    out.push_str(rest);
    out
}

fn redact_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let at_word_start = out
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        if at_word_start {
            if let Some(len) = bearer_len(rest) {
                out.push_str(&rest[..BEARER.len()]);
                out.push_str(REDACTED);
                rest = &rest[len..];
                continue;
            }
            if let Some(len) = token_len(rest) {
                out.push_str(REDACTED);
                rest = &rest[len..];
                continue;
            }
        }
        let mut chars = rest.chars();
        if let Some(ch) = chars.next() {
            out.push(ch);
        }
        rest = chars.as_str();
    }
    out
}

fn bearer_len(text: &str) -> Option<usize> {
    let head = text.get(..BEARER.len())?;
    if !head.eq_ignore_ascii_case(BEARER) {
        return None;
    }
    let tail = tail_len(&text[BEARER.len()..], true);
    (tail >= MIN_TOKEN_TAIL).then_some(BEARER.len() + tail)
}

fn token_len(text: &str) -> Option<usize> {
    let prefix = TOKEN_PREFIXES
        .iter()
        .find(|prefix| text.starts_with(**prefix))?;
    let rest = &text[prefix.len()..];
    let tail = tail_len(rest, *prefix == JWT_PREFIX);
    let has_digit = rest[..tail].bytes().any(|byte| byte.is_ascii_digit());
    (tail >= MIN_TOKEN_TAIL && has_digit).then_some(prefix.len() + tail)
}

fn tail_len(text: &str, allow_dots: bool) -> usize {
    text.bytes()
        .take_while(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'-')
                || (allow_dots && matches!(byte, b'.' | b'+' | b'/' | b'='))
        })
        .count()
}

/// Replace each path that starts with `home` by `~`. A match must end at a
/// path separator or at the end of a path, so `/home/dev` does not change
/// `/home/developer`. The match ignores ASCII case and the slash direction,
/// because Windows and macOS paths can differ in both.
fn replace_home(text: &str, home: &str) -> String {
    let home = home.trim_end_matches(['/', '\\']);
    if home.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if let Some(len) = home_match_len(rest, home)
            && starts_path(&text[..index])
            && ends_path(&rest[len..])
        {
            out.push('~');
            index += len;
            continue;
        }
        let ch = rest.chars().next().unwrap_or_default();
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn home_match_len(text: &str, home: &str) -> Option<usize> {
    let candidate = text.get(..home.len())?;
    let same = candidate
        .bytes()
        .zip(home.bytes())
        .all(|(a, b)| normalize_path_byte(a) == normalize_path_byte(b));
    same.then_some(home.len())
}

const fn normalize_path_byte(byte: u8) -> u8 {
    if byte == b'\\' {
        b'/'
    } else {
        byte.to_ascii_lowercase()
    }
}

fn starts_path(before: &str) -> bool {
    before
        .chars()
        .next_back()
        .is_none_or(|ch| !(ch.is_alphanumeric() || matches!(ch, '/' | '\\' | '.' | '_' | '-')))
}

fn ends_path(after: &str) -> bool {
    after
        .chars()
        .next()
        .is_none_or(|ch| !(ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(prefix: &str) -> String {
        format!("{prefix}{}", "a1B2".repeat(6))
    }

    #[test]
    fn known_token_shapes_are_redacted() {
        for prefix in ["ghp_", "github_pat_", "sk_live_", "xoxs-", "AIza", "npm_"] {
            let text = format!("key {} end", token(prefix));
            assert_eq!(redact(&text, None), "key [redacted] end", "{prefix}");
        }
        let jwt = format!("{}.{}.{}", token("eyJ"), "eyJzdWIi0", "c2lnbmF0dXJl1");
        assert_eq!(redact(&jwt, None), REDACTED);
        let header = format!("Authorization: Bearer {}", "x9Y8".repeat(5));
        assert_eq!(redact(&header, None), "Authorization: Bearer [redacted]");
    }

    #[test]
    fn ordinary_words_stay() {
        for text in [
            "npm_config_user_agent is set",
            "ghp_short",
            "the risk-free path",
            "xsk-abcdefghijklmnopqrstuvwxyz1234",
            "Bearer token",
        ] {
            assert_eq!(redact(text, None), text);
        }
    }

    #[test]
    fn url_passwords_and_private_keys_are_redacted() {
        assert_eq!(
            redact("clone https://me:hunter2@git.example.com/repo.git", None),
            "clone https://[redacted]@git.example.com/repo.git"
        );
        assert_eq!(
            redact("see https://git.example.com/a@b", None),
            "see https://git.example.com/a@b"
        );
        let key = "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----\nb";
        assert_eq!(redact(key, None), "a\n[redacted private key]\nb");
        let open = "-----BEGIN PRIVATE KEY-----\nMIIE";
        assert_eq!(redact(open, None), PRIVATE_KEY_REDACTED);
        let cert = "-----BEGIN CERTIFICATE-----";
        assert_eq!(redact(cert, None), cert);
    }

    #[test]
    fn home_is_replaced_only_at_a_path_boundary() {
        let home = Some("/home/dev");
        assert_eq!(redact("/home/dev/app/a.ts", home), "~/app/a.ts");
        assert_eq!(redact("in /home/dev", home), "in ~");
        assert_eq!(redact("/home/developer/x", home), "/home/developer/x");
        assert_eq!(redact("/mnt/home/dev/x", home), "/mnt/home/dev/x");
    }

    #[test]
    fn home_matches_any_case_and_slash() {
        let home = r"C:\Users\Bob";
        assert_eq!(replace_home(r"c:\users\bob\app", home), r"~\app");
        assert_eq!(replace_home("C:/Users/Bob/app", home), "~/app");
        assert_eq!(replace_home(r"C:\Users\Bobby", home), r"C:\Users\Bobby");
        assert_eq!(replace_home("/Users/bob/x", "/Users/Bob"), "~/x");
    }
}
