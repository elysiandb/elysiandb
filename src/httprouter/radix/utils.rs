use crate::gocompat::utf8::decode_rune;

use super::node::NodeType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WildPath {
    pub(crate) path: String,
    pub(crate) keys: Vec<String>,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) p_type: NodeType,
    pub(crate) pattern: String,
    pub(crate) has_regex: bool,
}

pub(crate) fn longest_common_prefix(a: &[u8], b: &[u8]) -> usize {
    let count = |s: &[u8]| {
        let mut n = 0;
        let mut rest = s;
        while !rest.is_empty() {
            let (_, size) = decode_rune(rest);
            rest = &rest[size..];
            n += 1;
        }
        n
    };
    let max = count(a).min(count(b));
    let (mut a, mut b) = (a, b);
    let mut i = 0;
    let mut runes = 0;
    while runes < max {
        let (ra, size_a) = decode_rune(a);
        let (rb, size_b) = decode_rune(b);
        a = &a[size_a..];
        b = &b[size_b..];
        if ra != rb {
            return i;
        }
        i += size_a;
        runes += 1;
    }
    i
}

pub(crate) fn segment_end_index(path: &[u8], include_tsr: bool) -> usize {
    let mut end = 0;
    while end < path.len() && path[end] != b'/' {
        end += 1;
    }
    if include_tsr && &path[end..] == b"/" {
        end += 1;
    }
    end
}

pub(crate) fn find_wild_path(path: &str, full_path: &str) -> Option<WildPath> {
    let bytes = path.as_bytes();
    for (start, &c) in bytes.iter().enumerate() {
        if c != b'{' {
            continue;
        }
        let mut with_regex = false;
        let mut keys = 0;
        for (end, &c) in bytes[start + 1..].iter().enumerate() {
            match c {
                b'}' => {
                    if keys > 0 {
                        keys -= 1;
                        continue;
                    }
                    let end = start + end + 2;
                    let mut wp = WildPath {
                        path: path[start..end].to_string(),
                        keys: vec![path[start + 1..end - 1].to_string()],
                        start,
                        end,
                        p_type: NodeType::Param,
                        pattern: String::new(),
                        has_regex: false,
                    };
                    if path.len() > end && bytes[end] == b'{' {
                        panic!("the wildcards must be separated by at least 1 char");
                    }
                    let key = wp.keys[0].clone();
                    match key.split_once(':') {
                        Some((name, pattern)) => {
                            wp.keys = vec![name.to_string()];
                            if pattern == "*" {
                                wp.pattern = pattern.to_string();
                                wp.p_type = NodeType::Wildcard;
                            } else {
                                wp.pattern = format!("({pattern})");
                                wp.has_regex = true;
                            }
                        }
                        None => {
                            if bytes[bytes.len() - 1] != b'/' {
                                wp.pattern = "(.*)".to_string();
                            }
                        }
                    }
                    if wp.keys[0].is_empty() {
                        panic!(
                            "wildcards must be named with a non-empty name in path '{full_path}'"
                        );
                    }
                    let seg_end = end + segment_end_index(&bytes[end..], true);
                    let mut rest = &path[end..seg_end];
                    if rest == "/" {
                        rest = "";
                        wp.end += 1;
                    }
                    if !rest.is_empty() {
                        match find_wild_path(rest, full_path) {
                            Some(wp2) => {
                                let prefix = &rest[..wp2.start];
                                wp.end += wp2.end;
                                wp.path.push_str(prefix);
                                wp.path.push_str(&wp2.path);
                                wp.pattern.push_str(prefix);
                                wp.pattern.push_str(&wp2.pattern);
                                wp.keys.extend(wp2.keys);
                            }
                            None => {
                                wp.path.push_str(rest);
                                wp.pattern.push_str(rest);
                                wp.end += rest.len();
                            }
                        }
                        wp.has_regex = true;
                    }
                    if wp.has_regex {
                        panic!(
                            "regular expression route parameters are not supported in path '{full_path}'"
                        );
                    }
                    return Some(wp);
                }
                b':' => with_regex = true,
                b'{' => {
                    if !with_regex && keys == 0 {
                        panic!("the char '{{' is not allowed in the param name");
                    }
                    keys += 1;
                }
                _ => {}
            }
        }
    }
    None
}
