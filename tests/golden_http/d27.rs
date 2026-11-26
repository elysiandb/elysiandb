pub fn message_for(scenario: &str, id: &str) -> Option<&'static str> {
    let msg = match (scenario, id) {
        ("off", "098-garbage-request-line") => "bad request",
        ("off", "099-garbage-bytes") => "bad request",
        ("off", "100-duplicate-content-length") => "bad request",
        ("off", "102-body-4mib-plus-one") => "request body too large",
        ("off", "104-body-4mib-plus-one-chunked") => "request body too large",
        ("off", "106-head-70kib") => "request header fields too large",
        ("timeouts", "001-header-read-timeout") => "bad request",
        ("timeouts", "002-body-read-timeout") => "request timeout",
        ("limit", "002-concurrency-503") => "too many connections",
        ("off", "033-query-decode-error") | ("basic", "037-query-decode-error") => {
            r#"{"error":"invalid JSON: \"limit\" must be an integer, got a string"}"#
        }
        ("off", "034-query-syntax-error") | ("basic", "038-query-syntax-error") => {
            r#"{"error":"invalid JSON at byte 2: unexpected character 'o'"}"#
        }
        _ => return None,
    };
    Some(msg)
}

pub fn log_reason_for(scenario: &str, id: &str) -> Option<&'static str> {
    let reason = match (scenario, id) {
        ("off", "098-garbage-request-line") => "invalid request line",
        ("off", "099-garbage-bytes") => "invalid request line",
        ("off", "100-duplicate-content-length") => "duplicate Content-Length header",
        ("off", "102-body-4mib-plus-one") => "request body too large",
        ("off", "104-body-4mib-plus-one-chunked") => "request body too large",
        _ => return None,
    };
    Some(reason)
}
