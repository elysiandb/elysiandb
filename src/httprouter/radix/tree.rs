use super::node::{Node, NodeType};
use super::utils::longest_common_prefix;
use crate::httpd::{Handler, RequestCtx};

#[derive(Clone, Debug)]
pub struct Tree {
    root: Node,
}

impl Default for Tree {
    fn default() -> Tree {
        Tree::new()
    }
}

impl Tree {
    pub fn new() -> Tree {
        Tree {
            root: Node::new_root(),
        }
    }

    pub fn add(&mut self, path: &str, handler: Handler) {
        if !path.starts_with('/') {
            panic!("path must begin with '/' in path '{path}'");
        }
        let full_path = path;
        let mut path = path;

        let i = longest_common_prefix(path.as_bytes(), self.root.path.as_bytes());
        if i > 0 {
            if self.root.path.len() > i {
                self.root.split(i);
            }
            path = &path[i..];
        }

        if let Err(err) = self.root.add(path, full_path, handler) {
            panic!("{err}");
        }

        if self.root.path.is_empty() {
            let first = self.root.children.remove(0);
            self.root = first;
            self.root.n_type = NodeType::Root;
        }

        self.root.sort();
    }

    pub fn get(&self, path: &[u8], ctx: Option<&mut RequestCtx>) -> (Option<Handler>, bool) {
        let rp = self.root.path.as_bytes();
        if path.len() > rp.len() {
            if &path[..rp.len()] != rp {
                return (None, false);
            }
            return self.root.get_from_child(&path[rp.len()..], ctx);
        } else if path == rp {
            if self.root.tsr {
                return (None, true);
            }
            if let Some(h) = &self.root.handler {
                return (Some(h.clone()), false);
            }
            if let Some(w) = &self.root.wildcard {
                if let Some(c) = ctx {
                    c.set_param(w.param_key.clone(), b"");
                }
                return (Some(w.handler.clone()), false);
            }
        }
        (None, false)
    }

    pub fn find_case_insensitive_path(
        &self,
        path: &[u8],
        fix_trailing_slash: bool,
        buf: &mut Vec<u8>,
    ) -> bool {
        let (found, tsr) = self.root.find(path, buf);
        if !found || (tsr && !fix_trailing_slash) {
            buf.clear();
            return false;
        }
        true
    }
}
