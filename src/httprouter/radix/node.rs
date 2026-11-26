use std::fmt;

use super::utils::{find_wild_path, longest_common_prefix, segment_end_index};
use crate::gocompat::strings::equal_fold;
use crate::httpd::{Handler, RequestCtx};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NodeType {
    Root,
    Static,
    Param,
    Wildcard,
}

#[derive(Clone)]
pub(crate) struct NodeWildcard {
    pub(crate) path: String,
    pub(crate) param_key: String,
    pub(crate) handler: Handler,
}

#[derive(Clone)]
pub(crate) struct Node {
    pub(crate) n_type: NodeType,
    pub(crate) path: String,
    pub(crate) tsr: bool,
    pub(crate) handler: Option<Handler>,
    pub(crate) children: Vec<Node>,
    pub(crate) wildcard: Option<NodeWildcard>,
    pub(crate) param_keys: Vec<String>,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("n_type", &self.n_type)
            .field("path", &self.path)
            .field("tsr", &self.tsr)
            .field("handler", &self.handler.is_some())
            .field("children", &self.children)
            .field(
                "wildcard",
                &self.wildcard.as_ref().map(|w| (&w.path, &w.param_key)),
            )
            .field("param_keys", &self.param_keys)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadixError {
    SetHandler(String),
    SetWildcardHandler(String),
    WildPathConflict(String, String, String, String),
    WildcardConflict(String, String, String, String),
    WildcardSlash(String),
    WildcardNotAtEnd(String),
}

impl fmt::Display for RadixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RadixError::SetHandler(p) => {
                write!(f, "a handler is already registered for path '{p}'")
            }
            RadixError::SetWildcardHandler(p) => {
                write!(f, "a wildcard handler is already registered for path '{p}'")
            }
            RadixError::WildPathConflict(a, b, c, d) => write!(
                f,
                "'{a}' in new path '{b}' conflicts with existing wild path '{c}' in existing prefix '{d}'"
            ),
            RadixError::WildcardConflict(a, b, c, d) => write!(
                f,
                "'{a}' in new path '{b}' conflicts with existing wildcard '{c}' in existing prefix '{d}'"
            ),
            RadixError::WildcardSlash(p) => write!(f, "no / before wildcard in path '{p}'"),
            RadixError::WildcardNotAtEnd(p) => {
                write!(
                    f,
                    "wildcard routes are only allowed at the end of the path in path '{p}'"
                )
            }
        }
    }
}

impl std::error::Error for RadixError {}

fn prefix_before(full_path: &str, path: &str) -> String {
    match full_path.rfind(path) {
        Some(i) => full_path[..i].to_string(),
        None => String::new(),
    }
}

pub(crate) fn new_node(path: &str) -> Node {
    Node {
        n_type: NodeType::Static,
        path: path.to_string(),
        tsr: false,
        handler: None,
        children: Vec::new(),
        wildcard: None,
        param_keys: Vec::new(),
    }
}

impl Node {
    pub(crate) fn new_root() -> Node {
        let mut n = new_node("");
        n.n_type = NodeType::Root;
        n
    }

    fn wild_path_conflict(&self, path: &str, full_path: &str) -> RadixError {
        let path_seg = path.split('/').next().unwrap_or("").to_string();
        let prefix = prefix_before(full_path, path) + &self.path;
        RadixError::WildPathConflict(path_seg, full_path.to_string(), self.path.clone(), prefix)
    }

    pub(crate) fn split(&mut self, i: usize) {
        let mut clone_child = self.clone();
        clone_child.n_type = NodeType::Static;
        clone_child.path = clone_child.path[i..].to_string();
        clone_child.param_keys = Vec::new();

        self.path.truncate(i);
        self.handler = None;
        self.tsr = false;
        self.wildcard = None;
        self.children = vec![clone_child];
    }

    fn set_handler(&mut self, handler: Handler, full_path: &str) -> Result<(), RadixError> {
        if self.handler.is_some() || self.tsr {
            return Err(RadixError::SetHandler(full_path.to_string()));
        }
        self.handler = Some(handler);
        let mut found_tsr = false;
        for child in self.children.iter_mut() {
            if child.path != "/" {
                continue;
            }
            child.tsr = true;
            found_tsr = true;
            break;
        }
        if self.path != "/" && !found_tsr {
            if self.path.ends_with('/') {
                let n = self.path.len() - 1;
                self.split(n);
                self.tsr = true;
            } else {
                let mut child_tsr = new_node("/");
                child_tsr.tsr = true;
                self.children.push(child_tsr);
            }
        }
        Ok(())
    }

    fn insert(&mut self, path: &str, full_path: &str, handler: Handler) -> Result<(), RadixError> {
        let end = segment_end_index(path.as_bytes(), true);
        let mut child = new_node(path);

        if let Some(wp) = find_wild_path(path, full_path) {
            let j = if wp.start > 0 { wp.start } else { end };
            child.path = path[..j].to_string();

            if wp.start > 0 {
                self.children.push(child);
                let last = self.children.len() - 1;
                return self.children[last].insert(&path[j..], full_path, handler);
            }

            match wp.p_type {
                NodeType::Param => {
                    child.n_type = wp.p_type;
                    child.param_keys = wp.keys.clone();
                }
                NodeType::Wildcard => {
                    let last_byte = self.path.as_bytes().last().copied();
                    if path.len() == end && last_byte != Some(b'/') {
                        return Err(RadixError::WildcardSlash(full_path.to_string()));
                    } else if path.len() != end {
                        return Err(RadixError::WildcardNotAtEnd(full_path.to_string()));
                    }

                    let target: &mut Node = if self.path != "/" && last_byte == Some(b'/') {
                        let n = self.path.len() - 1;
                        self.split(n);
                        self.tsr = true;
                        &mut self.children[0]
                    } else {
                        self
                    };

                    if let Some(w) = &target.wildcard {
                        if w.path == path {
                            return Err(RadixError::SetWildcardHandler(full_path.to_string()));
                        }
                        let prefix = prefix_before(full_path, path) + &w.path;
                        return Err(RadixError::WildcardConflict(
                            path.to_string(),
                            full_path.to_string(),
                            w.path.clone(),
                            prefix,
                        ));
                    }

                    target.wildcard = Some(NodeWildcard {
                        path: wp.path.clone(),
                        param_key: wp.keys[0].clone(),
                        handler,
                    });
                    return Ok(());
                }
                _ => {}
            }

            let rest = &path[wp.end..];
            if !rest.is_empty() {
                self.children.push(child);
                let last = self.children.len() - 1;
                return self.children[last].insert(rest, full_path, handler);
            }
        }

        child.handler = Some(handler);
        self.children.push(child);
        let last = self.children.len() - 1;
        if self.children[last].path == "/" {
            self.tsr = true;
        } else if self.children[last].path.ends_with('/') {
            let child = &mut self.children[last];
            let n = child.path.len() - 1;
            child.split(n);
            child.tsr = true;
        } else {
            let mut child_tsr = new_node("/");
            child_tsr.tsr = true;
            self.children[last].children.push(child_tsr);
        }
        Ok(())
    }

    pub(crate) fn add(
        &mut self,
        path: &str,
        full_path: &str,
        handler: Handler,
    ) -> Result<(), RadixError> {
        if path.is_empty() {
            return self.set_handler(handler, full_path);
        }

        for idx in 0..self.children.len() {
            let i = longest_common_prefix(path.as_bytes(), self.children[idx].path.as_bytes());
            if i == 0 {
                continue;
            }

            match self.children[idx].n_type {
                NodeType::Static => {
                    if self.children[idx].path.len() > i {
                        self.children[idx].split(i);
                    }
                    if path.len() > i {
                        return self.children[idx].add(&path[i..], full_path, handler);
                    }
                }
                NodeType::Param => {
                    let Some(wp) = find_wild_path(path, full_path) else {
                        panic!("invalid wild path in path '{full_path}'");
                    };
                    let is_param = wp.start == 0 && wp.p_type == NodeType::Param;
                    let has_handler = self.children[idx].handler.is_some();

                    if path.len() == wp.end && is_param && has_handler {
                        if self.children[idx].path == path {
                            return Err(RadixError::SetHandler(full_path.to_string()));
                        }
                        return Err(self.children[idx].wild_path_conflict(path, full_path));
                    }

                    if path.len() > i {
                        if self.children[idx].path == wp.path {
                            return self.children[idx].add(&path[i..], full_path, handler);
                        }
                        return self.insert(path, full_path, handler);
                    }
                }
                _ => {}
            }

            if path == "/" {
                self.tsr = true;
            }
            return self.children[idx].set_handler(handler, full_path);
        }

        self.insert(path, full_path, handler)
    }

    pub(crate) fn get_from_child(
        &self,
        path: &[u8],
        mut ctx: Option<&mut RequestCtx>,
    ) -> (Option<Handler>, bool) {
        for child in &self.children {
            match child.n_type {
                NodeType::Static => {
                    if path[0] != child.path.as_bytes()[0] {
                        continue;
                    }
                    let cp = child.path.as_bytes();
                    if path.len() > cp.len() {
                        if &path[..cp.len()] != cp {
                            continue;
                        }
                        let (h, tsr) = child.get_from_child(&path[cp.len()..], ctx.as_deref_mut());
                        if h.is_some() || tsr {
                            return (h, tsr);
                        }
                    } else if path == cp {
                        if child.tsr {
                            return (None, true);
                        }
                        if let Some(h) = &child.handler {
                            return (Some(h.clone()), false);
                        }
                        if let Some(w) = &child.wildcard {
                            if let Some(c) = ctx {
                                c.set_param(w.param_key.clone(), b"");
                            }
                            return (Some(w.handler.clone()), false);
                        }
                        return (None, false);
                    }
                }
                NodeType::Param => {
                    let end = segment_end_index(path, false);
                    let values = [path[..end].to_vec()];

                    if path.len() > end {
                        let (h, tsr) = child.get_from_child(&path[end..], ctx.as_deref_mut());
                        if tsr {
                            return (None, tsr);
                        } else if h.is_some() {
                            if let Some(c) = ctx.as_deref_mut() {
                                for (i, key) in child.param_keys.iter().enumerate() {
                                    c.set_param(key.clone(), &values[i]);
                                }
                            }
                            return (h, false);
                        }
                    } else if path.len() == end {
                        if child.tsr {
                            return (None, true);
                        }
                        let Some(h) = &child.handler else {
                            continue;
                        };
                        if let Some(c) = ctx.as_deref_mut() {
                            for (i, key) in child.param_keys.iter().enumerate() {
                                c.set_param(key.clone(), &values[i]);
                            }
                        }
                        return (Some(h.clone()), false);
                    }
                }
                _ => panic!("invalid node type"),
            }
        }

        if let Some(w) = &self.wildcard {
            if let Some(c) = ctx {
                c.set_param(w.param_key.clone(), path);
            }
            return (Some(w.handler.clone()), false);
        }

        (None, false)
    }

    pub(crate) fn find(&self, path: &[u8], buf: &mut Vec<u8>) -> (bool, bool) {
        let np = self.path.as_bytes();
        if path.len() > np.len() {
            if !equal_fold(&path[..np.len()], np) {
                return (false, false);
            }
            let rest = &path[np.len()..];
            buf.extend_from_slice(np);
            let (found, tsr) = self.find_from_child(rest, buf);
            if found {
                return (found, tsr);
            }
            buf.truncate(buf.len() - np.len());
        } else if equal_fold(path, np) {
            buf.extend_from_slice(np);
            if self.tsr {
                if self.path == "/" {
                    buf.truncate(buf.len() - np.len());
                } else {
                    buf.push(b'/');
                }
                return (true, true);
            }
            if self.handler.is_some() {
                return (true, false);
            }
            buf.truncate(buf.len() - np.len());
        }
        (false, false)
    }

    fn find_from_child(&self, path: &[u8], buf: &mut Vec<u8>) -> (bool, bool) {
        for child in &self.children {
            match child.n_type {
                NodeType::Static => {
                    let (found, tsr) = child.find(path, buf);
                    if found {
                        return (found, tsr);
                    }
                }
                NodeType::Param => {
                    let end = segment_end_index(path, false);
                    buf.extend_from_slice(&path[..end]);
                    if path.len() > end {
                        let (found, tsr) = child.find_from_child(&path[end..], buf);
                        if found {
                            return (found, tsr);
                        }
                    } else if path.len() == end {
                        if child.tsr {
                            buf.push(b'/');
                            return (true, true);
                        }
                        if child.handler.is_some() {
                            return (true, false);
                        }
                    }
                    buf.truncate(buf.len() - end);
                }
                _ => panic!("invalid node type"),
            }
        }
        if self.wildcard.is_some() {
            buf.extend_from_slice(path);
            return (true, false);
        }
        (false, false)
    }

    pub(crate) fn sort(&mut self) {
        for child in self.children.iter_mut() {
            child.sort();
        }
        crate::gocompat::sort::slice(&mut self.children, |a, b| {
            if a.n_type < b.n_type {
                return true;
            }
            if a.n_type > b.n_type {
                return false;
            }
            a.children.len() > b.children.len()
        });
    }
}
