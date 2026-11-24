use std::any::Any;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

use super::args::Args;
use super::conn::{ConnHandle, HijackConn};
use super::header::DEFAULT_CONTENT_TYPE;
use super::multipart::Form;
use super::request::{MultipartFormError, Request};
use super::response::{Response, StreamWriter};
use super::uri::URI;

pub type HijackHandler = Box<dyn FnOnce(&mut RequestCtx, &mut HijackConn) + Send>;

static GLOBAL_CONN_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_conn_id() -> u64 {
    GLOBAL_CONN_ID.fetch_add(1, Ordering::Relaxed) + 1
}

pub const MATCHED_ROUTE_PATH_PARAM: &str = "__matchedRoutePath__";

fn zero_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
}

pub struct RequestCtx {
    pub request: Request,
    pub response: Response,
    user_values: Vec<(String, Box<dyn Any + Send>)>,
    conn: Option<ConnHandle>,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    conn_id: u64,
    conn_request_num: u64,
    conn_time: Instant,
    time: Instant,
    wall_time: SystemTime,
    hijack_handler: Option<HijackHandler>,
    hijack_no_response: bool,
}

impl Default for RequestCtx {
    fn default() -> RequestCtx {
        RequestCtx::new()
    }
}

impl std::fmt::Debug for RequestCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestCtx")
            .field("request", &self.request)
            .field("response", &self.response)
            .field("conn_id", &self.conn_id)
            .finish()
    }
}

impl RequestCtx {
    pub fn new() -> RequestCtx {
        let now = Instant::now();
        RequestCtx {
            request: Request::new(),
            response: Response::new(),
            user_values: Vec::new(),
            conn: None,
            remote_addr: None,
            local_addr: None,
            conn_id: 0,
            conn_request_num: 0,
            conn_time: now,
            time: now,
            wall_time: SystemTime::now(),
            hijack_handler: None,
            hijack_no_response: false,
        }
    }

    pub fn init(&mut self, req: &Request, remote_addr: Option<SocketAddr>) {
        *self = RequestCtx::new();
        self.remote_addr = Some(remote_addr.unwrap_or_else(zero_addr));
        self.local_addr = Some(zero_addr());
        self.conn_id = next_conn_id();
        req.copy_to(&mut self.request);
    }

    pub(crate) fn attach(
        &mut self,
        conn: ConnHandle,
        remote_addr: SocketAddr,
        local_addr: SocketAddr,
        conn_id: u64,
        conn_time: Instant,
    ) {
        self.conn = Some(conn);
        self.remote_addr = Some(remote_addr);
        self.local_addr = Some(local_addr);
        self.conn_id = conn_id;
        self.conn_time = conn_time;
    }

    pub(crate) fn start(&mut self, conn_request_num: u64) {
        self.conn_request_num = conn_request_num;
        self.time = Instant::now();
        self.wall_time = SystemTime::now();
    }

    pub(crate) fn reset_for_next(&mut self) {
        self.request.reset();
        self.response.reset();
        self.user_values.clear();
        self.hijack_handler = None;
        self.hijack_no_response = false;
    }

    pub fn set_user_value<T: Any + Send>(&mut self, key: impl Into<String>, value: T) {
        let key = key.into();
        for kv in self.user_values.iter_mut() {
            if kv.0 == key {
                kv.1 = Box::new(value);
                return;
            }
        }
        self.user_values.push((key, Box::new(value)));
    }

    pub fn user_value<T: Any>(&self, key: &str) -> Option<&T> {
        self.user_values
            .iter()
            .find(|kv| kv.0 == key)
            .and_then(|kv| kv.1.downcast_ref::<T>())
    }

    pub fn user_value_mut<T: Any>(&mut self, key: &str) -> Option<&mut T> {
        self.user_values
            .iter_mut()
            .find(|kv| kv.0 == key)
            .and_then(|kv| kv.1.downcast_mut::<T>())
    }

    pub fn has_user_value(&self, key: &str) -> bool {
        self.user_values.iter().any(|kv| kv.0 == key)
    }

    pub fn remove_user_value(&mut self, key: &str) {
        self.user_values.retain(|kv| kv.0 != key);
    }

    pub fn reset_user_values(&mut self) {
        self.user_values.clear();
    }

    pub fn user_value_keys(&self) -> Vec<&str> {
        self.user_values.iter().map(|kv| kv.0.as_str()).collect()
    }

    pub fn set_param(&mut self, name: impl Into<String>, value: impl AsRef<[u8]>) {
        self.set_user_value(name, value.as_ref().to_vec());
    }

    pub fn param(&self, name: &str) -> &[u8] {
        if let Some(v) = self.user_value::<Vec<u8>>(name) {
            return v;
        }
        if let Some(v) = self.user_value::<String>(name) {
            return v.as_bytes();
        }
        if let Some(v) = self.user_value::<&'static str>(name) {
            return v.as_bytes();
        }
        &[]
    }

    pub fn has_param(&self, name: &str) -> bool {
        self.user_value::<Vec<u8>>(name).is_some()
            || self.user_value::<String>(name).is_some()
            || self.user_value::<&'static str>(name).is_some()
    }

    pub fn matched_route_path(&self) -> Option<&str> {
        self.user_value::<Arc<str>>(MATCHED_ROUTE_PATH_PARAM)
            .map(|s| &**s)
    }

    pub fn conn(&self) -> Option<&ConnHandle> {
        self.conn.as_ref()
    }

    pub fn is_tls(&self) -> bool {
        self.request.is_tls()
    }

    pub fn id(&self) -> u64 {
        (self.conn_id << 32) | self.conn_request_num
    }

    pub fn conn_id(&self) -> u64 {
        self.conn_id
    }

    pub fn conn_request_num(&self) -> u64 {
        self.conn_request_num
    }

    pub fn time(&self) -> Instant {
        self.time
    }

    pub fn wall_time(&self) -> SystemTime {
        self.wall_time
    }

    pub fn conn_time(&self) -> Instant {
        self.conn_time
    }

    pub fn set_connection_close(&mut self) {
        self.response.set_connection_close();
    }

    pub fn set_status_code(&mut self, status_code: u16) {
        self.response.set_status_code(status_code);
    }

    pub fn set_content_type(&mut self, content_type: impl AsRef<[u8]>) {
        self.response.header.set_content_type(content_type);
    }

    pub fn request_uri(&self) -> &[u8] {
        self.request.header.request_uri()
    }

    pub fn uri(&self) -> &URI {
        self.request.uri()
    }

    pub fn uri_mut(&mut self) -> &mut URI {
        self.request.uri_mut()
    }

    pub fn referer(&self) -> &[u8] {
        self.request.header.referer()
    }

    pub fn user_agent(&self) -> &[u8] {
        self.request.header.user_agent()
    }

    pub fn path(&self) -> &[u8] {
        self.uri().path()
    }

    pub fn host(&self) -> &[u8] {
        self.uri().host()
    }

    pub fn query_args(&self) -> &Args {
        self.uri().query_args()
    }

    pub fn query_args_mut(&mut self) -> &mut Args {
        self.uri_mut().query_args_mut()
    }

    pub fn multipart_form(&mut self) -> Result<&Form, MultipartFormError> {
        self.request.multipart_form()
    }

    pub fn method(&self) -> &[u8] {
        self.request.header.method()
    }

    pub fn is_get(&self) -> bool {
        self.request.header.is_get()
    }

    pub fn is_post(&self) -> bool {
        self.request.header.is_post()
    }

    pub fn is_put(&self) -> bool {
        self.request.header.is_put()
    }

    pub fn is_delete(&self) -> bool {
        self.request.header.is_delete()
    }

    pub fn is_connect(&self) -> bool {
        self.request.header.is_connect()
    }

    pub fn is_options(&self) -> bool {
        self.request.header.is_options()
    }

    pub fn is_trace(&self) -> bool {
        self.request.header.is_trace()
    }

    pub fn is_patch(&self) -> bool {
        self.request.header.is_patch()
    }

    pub fn is_head(&self) -> bool {
        self.request.header.is_head()
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr.unwrap_or_else(zero_addr)
    }

    pub fn set_remote_addr(&mut self, addr: SocketAddr) {
        self.remote_addr = Some(addr);
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr.unwrap_or_else(zero_addr)
    }

    pub fn remote_ip(&self) -> IpAddr {
        self.remote_addr().ip()
    }

    pub fn local_ip(&self) -> IpAddr {
        self.local_addr().ip()
    }

    pub fn error(&mut self, msg: impl AsRef<str>, status_code: u16) {
        self.response.reset();
        self.set_status_code(status_code);
        self.set_content_type(DEFAULT_CONTENT_TYPE);
        self.set_body_string(msg);
    }

    pub fn success(&mut self, content_type: impl AsRef<[u8]>, body: impl AsRef<[u8]>) {
        self.set_content_type(content_type);
        self.set_body(body);
    }

    pub fn redirect(&mut self, uri: impl AsRef<[u8]>, status_code: u16) {
        let mut u = self.uri().clone();
        u.update(uri);
        let full = u.full_uri();
        self.response.header.set_non_special(b"Location", &full);
        let status = match status_code {
            301 | 302 | 303 | 307 | 308 => status_code,
            _ => 302,
        };
        self.response.set_status_code(status);
    }

    pub fn set_body(&mut self, body: impl AsRef<[u8]>) {
        self.response.set_body(body);
    }

    pub fn set_body_string(&mut self, body: impl AsRef<str>) {
        self.response.set_body_string(body);
    }

    pub fn reset_body(&mut self) {
        self.response.reset_body();
    }

    pub fn write_string(&mut self, s: impl AsRef<str>) -> usize {
        let s = s.as_ref();
        self.response.append_body(s.as_bytes());
        s.len()
    }

    pub fn post_body(&self) -> &[u8] {
        self.request.body()
    }

    pub fn set_body_stream(&mut self, body_stream: Box<dyn Read + Send>, body_size: i64) {
        self.response.set_body_stream(body_stream, body_size);
    }

    pub fn set_body_stream_writer(
        &mut self,
        sw: impl FnOnce(&mut StreamWriter<'_>) + Send + 'static,
    ) {
        self.response.set_body_stream_writer(Box::new(sw));
    }

    pub fn is_body_stream(&self) -> bool {
        self.response.is_body_stream()
    }

    pub fn hijack(
        &mut self,
        handler: impl FnOnce(&mut RequestCtx, &mut HijackConn) + Send + 'static,
    ) {
        self.hijack_handler = Some(Box::new(handler));
    }

    pub fn hijack_set_no_response(&mut self, no_response: bool) {
        self.hijack_no_response = no_response;
    }

    pub fn hijacked(&self) -> bool {
        self.hijack_handler.is_some()
    }

    pub(crate) fn take_hijack(&mut self) -> (Option<HijackHandler>, bool) {
        let h = self.hijack_handler.take();
        let no_response = self.hijack_no_response && h.is_some();
        self.hijack_no_response = false;
        (h, no_response)
    }
}

impl Write for RequestCtx {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.response.append_body(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
