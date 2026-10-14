//! Log ↔ trace correlation primitives (no AWS calls).
//!
//! Extracts X-Ray trace IDs and Lambda request IDs from log text so the CLI
//! and desktop UI can jump between CloudWatch Logs and X-Ray.
//!
//! See `docs/PRD.md` for the product context.

/// Returns every X-Ray trace ID (`1-<8 hex>-<24 hex>`) found in `text`.
pub fn find_trace_ids(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let fits = i + 35 <= bytes.len()
            && bytes[i] == b'1'
            && bytes[i + 1] == b'-'
            && bytes[i + 2..i + 10].iter().all(|b| b.is_ascii_hexdigit())
            && bytes[i + 10] == b'-'
            && bytes[i + 11..i + 35].iter().all(|b| b.is_ascii_hexdigit())
            && (i + 35 == bytes.len() || !bytes[i + 35].is_ascii_hexdigit());
        if fits {
            out.push(text[i..i + 35].to_string());
            i += 35;
        } else {
            i += 1;
        }
    }
    out
}

fn is_uuid(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    matches!(parts.as_slice(), [a, b, c, d, e]
        if a.len() == 8 && b.len() == 4 && c.len() == 4 && d.len() == 4 && e.len() == 12)
        && token
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

/// Returns every Lambda request ID following a `RequestId:` marker in `text`.
pub fn find_lambda_request_ids(text: &str) -> Vec<String> {
    const MARKER: &str = "RequestId:";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(MARKER) {
        let after = &rest[pos + MARKER.len()..];
        let token: String = after
            .chars()
            .skip_while(|ch| ch.is_whitespace())
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .collect();
        if is_uuid(&token) {
            out.push(token);
        }
        rest = after;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_trace_ids() {
        let line = "ERROR 1-5f5e1000-abcdef1234567890abcdef12 something failed";
        assert_eq!(
            find_trace_ids(line),
            vec!["1-5f5e1000-abcdef1234567890abcdef12".to_string()]
        );
    }

    #[test]
    fn ignores_malformed_trace_ids() {
        assert!(find_trace_ids("trace 1-xyz-bogus here").is_empty());
        assert!(find_trace_ids("nothing here").is_empty());
    }

    #[test]
    fn extracts_lambda_request_ids() {
        let line = "REPORT RequestId: 12345678-1234-1234-1234-123456789012\tDuration: 5.00 ms";
        assert_eq!(
            find_lambda_request_ids(line),
            vec!["12345678-1234-1234-1234-123456789012".to_string()]
        );
    }

    #[test]
    fn ignores_non_uuid_request_ids() {
        assert!(find_lambda_request_ids("RequestId: not-a-uuid").is_empty());
    }
}
