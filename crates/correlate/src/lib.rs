//! Log ↔ trace correlation primitives (no AWS calls).
//!
//! Extracts X-Ray trace IDs, Lambda request IDs, and API Gateway extended
//! request IDs from log text (plain or JSON-embedded) so the CLI and desktop
//! UI can jump between CloudWatch Logs and X-Ray.
//!
//! See `docs/PRD.md` for the product context.

/// Returns every X-Ray trace ID (`1-<8 hex>-<24 hex>`) found in `text`.
///
/// Works on plain text and JSON-embedded values since it scans the raw string.
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

fn is_apigw_request_id(token: &str) -> bool {
    // API Gateway request / extended request IDs are opaque alphanumeric
    // tokens (often with `-`, `_`, `=` padding). Require a minimum length
    // to avoid matching ordinary words.
    let len = token.len();
    len >= 8
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '=')
        && token.chars().any(|ch| ch.is_ascii_alphanumeric())
}

/// Extract values for a literal marker such as `ExtendedRequestId:`.
fn find_marker_tokens(text: &str, markers: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for marker in markers {
        let mut rest = text;
        while let Some(pos) = rest.find(marker) {
            let after = &rest[pos + marker.len()..];
            let token: String = after
                .chars()
                .skip_while(|ch| ch.is_whitespace() || *ch == '"' || *ch == '\'' || *ch == ':')
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '=')
                .collect();
            if is_apigw_request_id(&token) {
                out.push(token);
            }
            if after.is_empty() {
                break;
            }
            rest = after;
        }
    }
    out
}

/// Returns API Gateway request / extended request IDs found in `text`.
///
/// Covers:
/// - `ExtendedRequestId: <id>` / `extendedRequestId: <id>` markers (plain logs)
/// - `"extendedRequestId": "<id>"` JSON fields (access logs, JSON-embedded)
/// - `"requestId": "<id>"` JSON fields when the value looks like an API GW ID
pub fn find_apigw_request_ids(text: &str) -> Vec<String> {
    let mut out = find_marker_tokens(
        text,
        &[
            "ExtendedRequestId:",
            "extendedRequestId:",
            "extendedRequestId\"",
            "extended_request_id\"",
            "\"extendedRequestId\"",
        ],
    );
    // JSON `"requestId": "<id>"` — only keep non-UUID values so Lambda
    // REPORT lines stay with `find_lambda_request_ids`.
    let mut rest = text;
    while let Some(pos) = rest.find("\"requestId\"") {
        let after = &rest[pos + "\"requestId\"".len()..];
        let token: String = after
            .chars()
            .skip_while(|ch| ch.is_whitespace() || *ch == ':' || *ch == '"' || *ch == '\'')
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '=')
            .collect();
        if is_apigw_request_id(&token) && !is_uuid(&token) {
            out.push(token);
        }
        if after.is_empty() {
            break;
        }
        rest = after;
    }
    out.sort();
    out.dedup();
    out
}

/// Jump targets extracted from a single log line (FR-15 / FR-16, log → trace).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogJump {
    pub trace_ids: Vec<String>,
    pub lambda_request_ids: Vec<String>,
    pub apigw_request_ids: Vec<String>,
}

impl LogJump {
    pub fn is_empty(&self) -> bool {
        self.trace_ids.is_empty()
            && self.lambda_request_ids.is_empty()
            && self.apigw_request_ids.is_empty()
    }
}

/// Extract all jump targets from a log line (plain or JSON-embedded).
pub fn extract_jumps(text: &str) -> LogJump {
    LogJump {
        trace_ids: find_trace_ids(text),
        lambda_request_ids: find_lambda_request_ids(text),
        apigw_request_ids: find_apigw_request_ids(text),
    }
}

/// Validate an X-Ray trace ID without extracting (`1-<8hex>-<24hex>`).
pub fn is_trace_id(value: &str) -> bool {
    find_trace_ids(value) == [value.to_string()]
}

/// Suggest a CloudWatch Logs time window around a trace (FR-16, trace → logs).
///
/// Returns `(start_ms, end_ms)` padded on both sides so short traces still
/// capture surrounding log lines. Inputs and outputs are epoch milliseconds.
pub fn suggest_log_window(trace_start_ms: i64, trace_end_ms: i64, pad_secs: i64) -> (i64, i64) {
    let (mut start, mut end) = if trace_start_ms <= trace_end_ms {
        (trace_start_ms, trace_end_ms)
    } else {
        (trace_end_ms, trace_start_ms)
    };
    let pad_ms = pad_secs.max(0) * 1000;
    start = start.saturating_sub(pad_ms);
    end = end.saturating_add(pad_ms);
    if end <= start {
        end = start.saturating_add(1000);
    }
    (start, end)
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
    fn extracts_trace_id_from_json() {
        let line = r#"{"level":"error","traceId":"1-5f5e1000-abcdef1234567890abcdef12","msg":"boom"}"#;
        assert_eq!(
            find_trace_ids(line),
            vec!["1-5f5e1000-abcdef1234567890abcdef12".to_string()]
        );
        let jump = extract_jumps(line);
        assert_eq!(jump.trace_ids.len(), 1);
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

    #[test]
    fn extracts_apigw_request_ids() {
        let line = "ExtendedRequestId: AbCdEf12-XYZ_01==";
        assert_eq!(
            find_apigw_request_ids(line),
            vec!["AbCdEf12-XYZ_01==".to_string()]
        );
        let json =
            r#"{"requestId":"AbCdEfGh123=","extendedRequestId":"XyZ-987_abc==","msg":"ok"}"#;
        let ids = find_apigw_request_ids(json);
        assert!(ids.contains(&"AbCdEfGh123=".to_string()));
        assert!(ids.contains(&"XyZ-987_abc==".to_string()));
    }

    #[test]
    fn extract_jumps_combines_all_ids() {
        let line = "ERROR 1-5f5e1000-abcdef1234567890abcdef12 RequestId: 12345678-1234-1234-1234-123456789012";
        let jump = extract_jumps(line);
        assert_eq!(jump.trace_ids.len(), 1);
        assert_eq!(jump.lambda_request_ids.len(), 1);
        assert!(!jump.is_empty());
        assert!(extract_jumps("nothing here").is_empty());
    }

    #[test]
    fn validates_trace_ids() {
        assert!(is_trace_id("1-5f5e1000-abcdef1234567890abcdef12"));
        assert!(!is_trace_id("1-xyz-bogus"));
        assert!(!is_trace_id(""));
    }

    #[test]
    fn suggests_padded_window() {
        assert_eq!(suggest_log_window(1_000_000, 1_060_000, 300), (700_000, 1_360_000));
        // Reversed inputs are normalized.
        assert_eq!(suggest_log_window(2_000, 1_000, 0), (1_000, 2_000));
    }
}
