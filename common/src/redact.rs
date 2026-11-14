//! Best-effort log scrubbing; build logs must still be treated as sensitive.
pub fn log(value: &str, secrets: &[String]) -> String {
    let mut text = value.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        text = text.replace(secret, "[REDACTED]");
    }
    // Drop complete URL tokens containing credentials or signed query parameters.
    text.split_inclusive(char::is_whitespace)
        .map(|token| {
            if token.to_ascii_lowercase().contains("x-amz-")
                || token
                    .split_once('@')
                    .is_some_and(|(prefix, _)| prefix.contains(':'))
            {
                let suffix = &token[token.trim_end().len()..];
                format!("[REDACTED URL]{suffix}")
            } else {
                token.to_owned()
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    #[test]
    fn secrets_and_capabilities() {
        assert_eq!(
            super::log("https://u:p@host/path\nsecret", &["secret".into()]),
            "[REDACTED URL]\n[REDACTED]"
        );
        assert_eq!(
            super::log("X-Amz-Signature=secret ", &[]),
            "[REDACTED URL] "
        );
        assert_eq!(super::log("user:token@host/path", &[]), "[REDACTED URL]");
        assert_eq!(
            super::log("https://host/?X-Amz-Signature=abc ", &[]),
            "[REDACTED URL] "
        );
    }
}

/// Bound broker-supplied logs independently of worker-side limits.
pub fn build_log(lines: &[String], secrets: &[String]) -> String {
    let mut text = String::new();
    for line in lines {
        let mut take = line
            .len()
            .min((10 * 1024 * 1024usize).saturating_sub(text.len()));
        while !line.is_char_boundary(take) {
            take -= 1;
        }
        text.push_str(&line[..take]);
        if take < line.len() || text.len() == 10 * 1024 * 1024 {
            break;
        }
    }
    log(&text, secrets)
}
