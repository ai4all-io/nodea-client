// The in-memory message log behind the Dev Logs page.
//
// It lives for the whole run and gets an entry for every gRPC event, so
// without a cap it grows for as long as the app stays open. REST entries carry
// request and response bodies, which can hold passwords and freshly created
// API keys, so those are redacted before they are kept or printed.

/// Oldest entries are dropped beyond this many.
pub const MAX_ENTRIES: usize = 1_000;

/// A body kept or printed is cut to this many characters.
pub const MAX_BODY_CHARS: usize = 2_000;

const REDACTED: &str = "[REDACTED]";

/// Append `entry`, dropping the oldest entries beyond `MAX_ENTRIES`.
pub fn push_capped<T>(log: &mut Vec<T>, entry: T) {
    log.push(entry);
    if log.len() > MAX_ENTRIES {
        let excess = log.len() - MAX_ENTRIES;
        log.drain(..excess);
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["password", "token", "secret", "key"]
        .iter()
        .any(|word| key.contains(word))
}

fn redact_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *v = serde_json::Value::String(REDACTED.to_string());
                } else {
                    redact_value(v);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_value),
        _ => {}
    }
}

/// A body safe to keep in the log or print: sensitive JSON fields replaced,
/// and the whole cut to `MAX_BODY_CHARS`. A body that is not JSON is only cut.
pub fn redact_body(body: &str) -> String {
    let redacted = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(mut value) => {
            redact_value(&mut value);
            value.to_string()
        }
        Err(_) => body.to_string(),
    };
    match redacted.char_indices().nth(MAX_BODY_CHARS) {
        Some((cut, _)) => format!("{}… [truncated]", &redacted[..cut]),
        None => redacted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_only_the_newest_entries() {
        let mut log = Vec::new();
        for i in 0..MAX_ENTRIES + 5 {
            push_capped(&mut log, i);
        }
        assert_eq!(log.len(), MAX_ENTRIES);
        assert_eq!(log[0], 5);
        assert_eq!(*log.last().unwrap(), MAX_ENTRIES + 4);
    }

    #[test]
    fn passwords_keys_and_tokens_are_redacted_at_any_depth() {
        let body = r#"{"email":"a@b.c","password":"hunter2",
            "data":{"api_key":"sk-123","items":[{"accessToken":"t","name":"x"}]},
            "client_secret":"s"}"#;
        let out = redact_body(body);

        for secret in ["hunter2", "sk-123", "\"t\"", "\"s\""] {
            assert!(!out.contains(secret), "{} leaked in {}", secret, out);
        }
        assert!(out.contains("a@b.c"));
        assert!(out.contains("\"name\":\"x\""));
    }

    #[test]
    fn a_body_that_is_not_json_is_kept_as_is() {
        assert_eq!(redact_body("Bad Gateway"), "Bad Gateway");
    }

    #[test]
    fn a_long_body_is_truncated_on_a_char_boundary() {
        let body = "é".repeat(MAX_BODY_CHARS + 10);
        let out = redact_body(&body);
        assert!(out.ends_with("… [truncated]"));
        assert_eq!(out.chars().filter(|c| *c == 'é').count(), MAX_BODY_CHARS);
    }
}
