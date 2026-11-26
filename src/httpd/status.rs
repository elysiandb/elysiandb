pub const STATUS_CONTINUE: u16 = 100;
pub const STATUS_SWITCHING_PROTOCOLS: u16 = 101;
pub const STATUS_PROCESSING: u16 = 102;
pub const STATUS_EARLY_HINTS: u16 = 103;
pub const STATUS_OK: u16 = 200;
pub const STATUS_CREATED: u16 = 201;
pub const STATUS_ACCEPTED: u16 = 202;
pub const STATUS_NON_AUTHORITATIVE_INFO: u16 = 203;
pub const STATUS_NO_CONTENT: u16 = 204;
pub const STATUS_RESET_CONTENT: u16 = 205;
pub const STATUS_PARTIAL_CONTENT: u16 = 206;
pub const STATUS_MULTI_STATUS: u16 = 207;
pub const STATUS_ALREADY_REPORTED: u16 = 208;
pub const STATUS_IM_USED: u16 = 226;
pub const STATUS_MULTIPLE_CHOICES: u16 = 300;
pub const STATUS_MOVED_PERMANENTLY: u16 = 301;
pub const STATUS_FOUND: u16 = 302;
pub const STATUS_SEE_OTHER: u16 = 303;
pub const STATUS_NOT_MODIFIED: u16 = 304;
pub const STATUS_USE_PROXY: u16 = 305;
pub const STATUS_TEMPORARY_REDIRECT: u16 = 307;
pub const STATUS_PERMANENT_REDIRECT: u16 = 308;
pub const STATUS_BAD_REQUEST: u16 = 400;
pub const STATUS_UNAUTHORIZED: u16 = 401;
pub const STATUS_PAYMENT_REQUIRED: u16 = 402;
pub const STATUS_FORBIDDEN: u16 = 403;
pub const STATUS_NOT_FOUND: u16 = 404;
pub const STATUS_METHOD_NOT_ALLOWED: u16 = 405;
pub const STATUS_NOT_ACCEPTABLE: u16 = 406;
pub const STATUS_PROXY_AUTH_REQUIRED: u16 = 407;
pub const STATUS_REQUEST_TIMEOUT: u16 = 408;
pub const STATUS_CONFLICT: u16 = 409;
pub const STATUS_GONE: u16 = 410;
pub const STATUS_LENGTH_REQUIRED: u16 = 411;
pub const STATUS_PRECONDITION_FAILED: u16 = 412;
pub const STATUS_REQUEST_ENTITY_TOO_LARGE: u16 = 413;
pub const STATUS_REQUEST_URI_TOO_LONG: u16 = 414;
pub const STATUS_UNSUPPORTED_MEDIA_TYPE: u16 = 415;
pub const STATUS_REQUESTED_RANGE_NOT_SATISFIABLE: u16 = 416;
pub const STATUS_EXPECTATION_FAILED: u16 = 417;
pub const STATUS_TEAPOT: u16 = 418;
pub const STATUS_MISDIRECTED_REQUEST: u16 = 421;
pub const STATUS_UNPROCESSABLE_ENTITY: u16 = 422;
pub const STATUS_LOCKED: u16 = 423;
pub const STATUS_FAILED_DEPENDENCY: u16 = 424;
pub const STATUS_UPGRADE_REQUIRED: u16 = 426;
pub const STATUS_PRECONDITION_REQUIRED: u16 = 428;
pub const STATUS_TOO_MANY_REQUESTS: u16 = 429;
pub const STATUS_REQUEST_HEADER_FIELDS_TOO_LARGE: u16 = 431;
pub const STATUS_UNAVAILABLE_FOR_LEGAL_REASONS: u16 = 451;
pub const STATUS_INTERNAL_SERVER_ERROR: u16 = 500;
pub const STATUS_NOT_IMPLEMENTED: u16 = 501;
pub const STATUS_BAD_GATEWAY: u16 = 502;
pub const STATUS_SERVICE_UNAVAILABLE: u16 = 503;
pub const STATUS_GATEWAY_TIMEOUT: u16 = 504;
pub const STATUS_HTTP_VERSION_NOT_SUPPORTED: u16 = 505;
pub const STATUS_VARIANT_ALSO_NEGOTIATES: u16 = 506;
pub const STATUS_INSUFFICIENT_STORAGE: u16 = 507;
pub const STATUS_LOOP_DETECTED: u16 = 508;
pub const STATUS_NOT_EXTENDED: u16 = 510;
pub const STATUS_NETWORK_AUTHENTICATION_REQUIRED: u16 = 511;

const UNKNOWN_STATUS_CODE: &str = "Unknown Status Code";

pub fn status_message(status_code: u16) -> &'static str {
    match status_code {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Request Entity Too Large",
        414 => "Request URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Requested Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => UNKNOWN_STATUS_CODE,
    }
}

pub(crate) fn format_status_line(
    dst: &mut Vec<u8>,
    protocol: &[u8],
    status_code: u16,
    status_text: &[u8],
) {
    dst.extend_from_slice(protocol);
    dst.push(b' ');
    dst.extend_from_slice(status_code.to_string().as_bytes());
    dst.push(b' ');
    if status_text.is_empty() {
        dst.extend_from_slice(status_message(status_code).as_bytes());
    } else {
        dst.extend_from_slice(status_text);
    }
    dst.extend_from_slice(b"\r\n");
}
