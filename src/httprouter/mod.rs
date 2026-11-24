mod path;
pub mod radix;

use std::any::Any;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

pub use path::{clean_path, get_optional_paths};
pub use radix::Tree;

use crate::httpd::{
    Handler, MATCHED_ROUTE_PATH_PARAM, METHOD_CONNECT, METHOD_DELETE, METHOD_GET, METHOD_HEAD,
    METHOD_OPTIONS, METHOD_PATCH, METHOD_POST, METHOD_PUT, METHOD_TRACE, RequestCtx,
    STATUS_METHOD_NOT_ALLOWED, STATUS_MOVED_PERMANENTLY, STATUS_NOT_FOUND,
    STATUS_PERMANENT_REDIRECT, status_message,
};

pub const METHOD_WILD: &str = "*";

pub type PanicHandler = Arc<dyn Fn(&mut RequestCtx, Box<dyn Any + Send>) + Send + Sync>;

thread_local! {
    static RECOVERY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub fn in_recovery_point() -> bool {
    RECOVERY_DEPTH.with(|d| d.get() > 0)
}

struct RecoveryGuard;

impl RecoveryGuard {
    fn enter() -> RecoveryGuard {
        RECOVERY_DEPTH.with(|d| d.set(d.get() + 1));
        RecoveryGuard
    }
}

impl Drop for RecoveryGuard {
    fn drop(&mut self) {
        RECOVERY_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

fn validate_path(path: &str) {
    if path.is_empty() || !path.starts_with('/') {
        panic!("path must begin with '/' in path '{path}'");
    }
}

pub struct Router {
    trees: Vec<Option<Tree>>,
    custom_methods_index: BTreeMap<String, usize>,
    registered_paths: BTreeMap<String, Vec<String>>,
    pub save_matched_route_path: bool,
    pub redirect_trailing_slash: bool,
    pub redirect_fixed_path: bool,
    pub handle_method_not_allowed: bool,
    pub handle_options: bool,
    pub global_options: Option<Handler>,
    pub not_found: Option<Handler>,
    pub method_not_allowed: Option<Handler>,
    pub panic_handler: Option<PanicHandler>,
    global_allowed: String,
}

impl Default for Router {
    fn default() -> Router {
        Router::new()
    }
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Router")
            .field("registered_paths", &self.registered_paths)
            .field("save_matched_route_path", &self.save_matched_route_path)
            .finish()
    }
}

impl Router {
    pub fn new() -> Router {
        Router {
            trees: (0..10).map(|_| None).collect(),
            custom_methods_index: BTreeMap::new(),
            registered_paths: BTreeMap::new(),
            save_matched_route_path: false,
            redirect_trailing_slash: true,
            redirect_fixed_path: true,
            handle_method_not_allowed: true,
            handle_options: true,
            global_options: None,
            not_found: None,
            method_not_allowed: None,
            panic_handler: None,
            global_allowed: String::new(),
        }
    }

    fn method_index_of(&self, method: &[u8]) -> Option<usize> {
        let idx = match method {
            b"GET" => 0,
            b"HEAD" => 1,
            b"POST" => 2,
            b"PUT" => 3,
            b"PATCH" => 4,
            b"DELETE" => 5,
            b"CONNECT" => 6,
            b"OPTIONS" => 7,
            b"TRACE" => 8,
            b"*" => 9,
            _ => {
                let m = std::str::from_utf8(method).ok()?;
                return self.custom_methods_index.get(m).copied();
            }
        };
        Some(idx)
    }

    pub fn list(&self) -> &BTreeMap<String, Vec<String>> {
        &self.registered_paths
    }

    pub fn get(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_GET, path, handler);
    }

    pub fn head(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_HEAD, path, handler);
    }

    pub fn post(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_POST, path, handler);
    }

    pub fn put(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_PUT, path, handler);
    }

    pub fn patch(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_PATCH, path, handler);
    }

    pub fn delete(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_DELETE, path, handler);
    }

    pub fn connect(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_CONNECT, path, handler);
    }

    pub fn options(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_OPTIONS, path, handler);
    }

    pub fn trace(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_TRACE, path, handler);
    }

    pub fn any(&mut self, path: &str, handler: Handler) {
        self.handle(METHOD_WILD, path, handler);
    }

    pub fn handle(&mut self, method: &str, path: &str, handler: Handler) {
        if method.is_empty() {
            panic!("method must not be empty");
        }
        validate_path(path);

        self.registered_paths
            .entry(method.to_string())
            .or_default()
            .push(path.to_string());

        let method_index = match self.method_index_of(method.as_bytes()) {
            Some(i) => i,
            None => {
                self.trees.push(None);
                let i = self.trees.len() - 1;
                self.custom_methods_index.insert(method.to_string(), i);
                i
            }
        };

        if self.trees[method_index].is_none() {
            self.trees[method_index] = Some(Tree::new());
            self.global_allowed = self.allowed(b"*", "");
        }

        let handler = if self.save_matched_route_path {
            let route: Arc<str> = Arc::from(path);
            let inner = handler;
            Arc::new(move |ctx: &mut RequestCtx| {
                ctx.set_user_value(MATCHED_ROUTE_PATH_PARAM, route.clone());
                inner(ctx);
            }) as Handler
        } else {
            handler
        };

        let optional_paths = get_optional_paths(path);
        let Some(tree) = self.trees[method_index].as_mut() else {
            unreachable!("tree is initialized");
        };
        if optional_paths.is_empty() {
            tree.add(path, handler);
        } else {
            for p in &optional_paths {
                tree.add(p, handler.clone());
            }
        }
    }

    pub fn lookup(
        &self,
        method: &str,
        path: &[u8],
        mut ctx: Option<&mut RequestCtx>,
    ) -> (Option<Handler>, bool) {
        let Some(method_index) = self.method_index_of(method.as_bytes()) else {
            return (None, false);
        };
        if let Some(tree) = &self.trees[method_index] {
            let (h, tsr) = tree.get(path, ctx.as_deref_mut());
            if h.is_some() || tsr {
                return (h, tsr);
            }
        }
        if let Some(tree) = &self.trees[9] {
            return tree.get(path, ctx);
        }
        (None, false)
    }

    fn allowed(&self, path: &[u8], req_method: &str) -> String {
        let mut allowed: Vec<String> = Vec::new();
        if path == b"*" || path == b"/*" {
            if req_method.is_empty() {
                for method in self.registered_paths.keys() {
                    if method == METHOD_OPTIONS {
                        continue;
                    }
                    allowed.push(method.clone());
                }
            } else {
                return self.global_allowed.clone();
            }
        } else {
            for method in self.registered_paths.keys() {
                if method == req_method || method == METHOD_OPTIONS {
                    continue;
                }
                let Some(idx) = self.method_index_of(method.as_bytes()) else {
                    continue;
                };
                if let Some(tree) = &self.trees[idx] {
                    let (h, _) = tree.get(path, None);
                    if h.is_some() {
                        allowed.push(method.clone());
                    }
                }
            }
        }

        if allowed.is_empty() {
            return String::new();
        }
        allowed.push(METHOD_OPTIONS.to_string());
        for i in 1..allowed.len() {
            let mut j = i;
            while j > 0 && allowed[j] < allowed[j - 1] {
                allowed.swap(j, j - 1);
                j -= 1;
            }
        }
        allowed.join(", ")
    }

    fn try_redirect(
        &self,
        ctx: &mut RequestCtx,
        tree: &Tree,
        tsr: bool,
        method: &[u8],
        path: &[u8],
    ) -> bool {
        let code = if method == METHOD_GET.as_bytes() {
            STATUS_MOVED_PERMANENTLY
        } else {
            STATUS_PERMANENT_REDIRECT
        };

        if tsr && self.redirect_trailing_slash {
            let mut uri = Vec::new();
            if path.len() > 1 && path[path.len() - 1] == b'/' {
                uri.extend_from_slice(&path[..path.len() - 1]);
            } else {
                uri.extend_from_slice(path);
                uri.push(b'/');
            }
            let query = ctx.uri().query_string().to_vec();
            if !query.is_empty() {
                uri.push(b'?');
                uri.extend_from_slice(&query);
            }
            ctx.redirect(&uri, code);
            return true;
        }

        if self.redirect_fixed_path {
            let path2 = ctx.uri().path().to_vec();
            let mut uri = Vec::new();
            let found = tree.find_case_insensitive_path(
                clean_path(&path2),
                self.redirect_trailing_slash,
                &mut uri,
            );
            if found {
                let query = ctx.uri().query_string().to_vec();
                if !query.is_empty() {
                    uri.push(b'?');
                    uri.extend_from_slice(&query);
                }
                ctx.redirect(&uri, code);
                return true;
            }
        }
        false
    }

    pub fn handler(&self, ctx: &mut RequestCtx) {
        match &self.panic_handler {
            None => self.dispatch(ctx),
            Some(ph) => {
                let outcome = {
                    let _guard = RecoveryGuard::enter();
                    catch_unwind(AssertUnwindSafe(|| self.dispatch(ctx)))
                };
                if let Err(payload) = outcome {
                    ph(ctx, payload);
                }
            }
        }
    }

    fn dispatch(&self, ctx: &mut RequestCtx) {
        let path = ctx.uri().path_original().to_vec();
        let method = ctx.request.header.method().to_vec();
        let method_index = self.method_index_of(&method);

        if let Some(idx) = method_index
            && let Some(tree) = &self.trees[idx]
        {
            let (h, tsr) = tree.get(&path, Some(ctx));
            if let Some(h) = h {
                h(ctx);
                return;
            } else if method != METHOD_CONNECT.as_bytes()
                && path != b"/"
                && self.try_redirect(ctx, tree, tsr, &method, &path)
            {
                return;
            }
        }

        if let Some(tree) = &self.trees[9] {
            let (h, tsr) = tree.get(&path, Some(ctx));
            if let Some(h) = h {
                h(ctx);
                return;
            } else if method != METHOD_CONNECT.as_bytes()
                && path != b"/"
                && self.try_redirect(ctx, tree, tsr, &method, &path)
            {
                return;
            }
        }

        let method_str = String::from_utf8_lossy(&method).into_owned();
        if self.handle_options && method == METHOD_OPTIONS.as_bytes() {
            let allow = self.allowed(&path, METHOD_OPTIONS);
            if !allow.is_empty() {
                ctx.response.header.set("Allow", allow);
                if let Some(g) = &self.global_options {
                    g(ctx);
                }
                return;
            }
        } else if self.handle_method_not_allowed {
            let allow = self.allowed(&path, &method_str);
            if !allow.is_empty() {
                ctx.response.header.set("Allow", allow);
                match &self.method_not_allowed {
                    Some(h) => h(ctx),
                    None => {
                        ctx.set_status_code(STATUS_METHOD_NOT_ALLOWED);
                        ctx.set_body_string(status_message(STATUS_METHOD_NOT_ALLOWED));
                    }
                }
                return;
            }
        }

        match &self.not_found {
            Some(h) => h(ctx),
            None => ctx.error(status_message(STATUS_NOT_FOUND), STATUS_NOT_FOUND),
        }
    }

    pub fn into_handler(self) -> Handler {
        let r = Arc::new(self);
        Arc::new(move |ctx: &mut RequestCtx| r.handler(ctx))
    }
}
