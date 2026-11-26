use std::collections::HashMap;

use gocompat::utf8::Rune;

use super::charclass::{CharSet, char_description};
use super::parser::{
    ECMASCRIPT, EXPLICIT_CAPTURE, IGNORE_CASE, IGNORE_PATTERN_WHITESPACE, MULTILINE, RIGHT_TO_LEFT,
    RegexOptions, SINGLELINE,
};
use super::{MAX_INT32, push_rune};

pub(crate) type NodeType = i32;
pub(crate) type NodeId = usize;

pub(crate) const NT_ONELOOP: NodeType = 3;
pub(crate) const NT_NOTONELOOP: NodeType = 4;
pub(crate) const NT_SETLOOP: NodeType = 5;
pub(crate) const NT_ONELAZY: NodeType = 6;
pub(crate) const NT_NOTONELAZY: NodeType = 7;
pub(crate) const NT_SETLAZY: NodeType = 8;
pub(crate) const NT_ONE: NodeType = 9;
pub(crate) const NT_NOTONE: NodeType = 10;
pub(crate) const NT_SET: NodeType = 11;
pub(crate) const NT_MULTI: NodeType = 12;
pub(crate) const NT_REF: NodeType = 13;
pub(crate) const NT_BOL: NodeType = 14;
pub(crate) const NT_EOL: NodeType = 15;
pub(crate) const NT_BOUNDARY: NodeType = 16;
pub(crate) const NT_NONBOUNDARY: NodeType = 17;
pub(crate) const NT_BEGINNING: NodeType = 18;
pub(crate) const NT_START: NodeType = 19;
pub(crate) const NT_END_Z: NodeType = 20;
pub(crate) const NT_END: NodeType = 21;
pub(crate) const NT_NOTHING: NodeType = 22;
pub(crate) const NT_EMPTY: NodeType = 23;
pub(crate) const NT_ALTERNATE: NodeType = 24;
pub(crate) const NT_CONCATENATE: NodeType = 25;
pub(crate) const NT_LOOP: NodeType = 26;
pub(crate) const NT_LAZYLOOP: NodeType = 27;
pub(crate) const NT_CAPTURE: NodeType = 28;
pub(crate) const NT_GROUP: NodeType = 29;
pub(crate) const NT_REQUIRE: NodeType = 30;
pub(crate) const NT_PREVENT: NodeType = 31;
pub(crate) const NT_GREEDY: NodeType = 32;
pub(crate) const NT_TESTREF: NodeType = 33;
pub(crate) const NT_TESTGROUP: NodeType = 34;
pub(crate) const NT_ECMA_BOUNDARY: NodeType = 41;
pub(crate) const NT_NON_ECMA_BOUNDARY: NodeType = 42;

#[derive(Clone, Debug, Default)]
pub(crate) struct RegexNode {
    pub(crate) t: NodeType,
    pub(crate) children: Vec<NodeId>,
    pub(crate) str: Vec<Rune>,
    pub(crate) set: Option<CharSet>,
    pub(crate) ch: Rune,
    pub(crate) m: isize,
    pub(crate) n: isize,
    pub(crate) options: RegexOptions,
    pub(crate) next: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub struct RegexTree {
    pub(crate) nodes: Vec<RegexNode>,
    pub(crate) root: NodeId,
    pub(crate) caps: HashMap<isize, isize>,
    pub(crate) capnumlist: Option<Vec<isize>>,
    pub(crate) captop: isize,
    pub capnames: Option<HashMap<String, isize>>,
    pub caplist: Option<Vec<String>>,
    pub(crate) options: RegexOptions,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Arena {
    pub(crate) nodes: Vec<RegexNode>,
}

impl Arena {
    fn push(&mut self, node: RegexNode) -> NodeId {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub(crate) fn new_node(&mut self, t: NodeType, opt: RegexOptions) -> NodeId {
        self.push(RegexNode {
            t,
            options: opt,
            ..RegexNode::default()
        })
    }

    pub(crate) fn new_node_ch(&mut self, t: NodeType, opt: RegexOptions, ch: Rune) -> NodeId {
        self.push(RegexNode {
            t,
            options: opt,
            ch,
            ..RegexNode::default()
        })
    }

    pub(crate) fn new_node_str(
        &mut self,
        t: NodeType,
        opt: RegexOptions,
        str: Vec<Rune>,
    ) -> NodeId {
        self.push(RegexNode {
            t,
            options: opt,
            str,
            ..RegexNode::default()
        })
    }

    pub(crate) fn new_node_set(&mut self, t: NodeType, opt: RegexOptions, set: CharSet) -> NodeId {
        self.push(RegexNode {
            t,
            options: opt,
            set: Some(set),
            ..RegexNode::default()
        })
    }

    pub(crate) fn new_node_m(&mut self, t: NodeType, opt: RegexOptions, m: isize) -> NodeId {
        self.push(RegexNode {
            t,
            options: opt,
            m,
            ..RegexNode::default()
        })
    }

    pub(crate) fn new_node_mn(
        &mut self,
        t: NodeType,
        opt: RegexOptions,
        m: isize,
        n: isize,
    ) -> NodeId {
        self.push(RegexNode {
            t,
            options: opt,
            m,
            n,
            ..RegexNode::default()
        })
    }

    pub(crate) fn write_str_to_buf(&self, n: NodeId, buf: &mut String) {
        for &r in &self.nodes[n].str {
            push_rune(buf, r);
        }
    }

    pub(crate) fn add_child(&mut self, n: NodeId, child: NodeId) {
        let reduced = self.reduce(child);
        self.nodes[n].children.push(reduced);
        self.nodes[reduced].next = Some(n);
    }

    fn insert_children(&mut self, n: NodeId, after_index: usize, nodes: &[NodeId]) {
        let old = &self.nodes[n].children;
        let mut new_children = Vec::with_capacity(old.len() + nodes.len());
        new_children.extend_from_slice(&old[..after_index]);
        new_children.extend_from_slice(nodes);
        new_children.extend_from_slice(&old[after_index..]);
        self.nodes[n].children = new_children;
    }

    fn remove_children(&mut self, n: NodeId, start_index: usize, end_index: usize) {
        self.nodes[n].children.drain(start_index..end_index);
    }

    fn make_rep(&mut self, n: NodeId, t: NodeType, min: isize, max: isize) {
        let node = &mut self.nodes[n];
        node.t += t - NT_ONE;
        node.m = min;
        node.n = max;
    }

    pub(crate) fn reduce(&mut self, n: NodeId) -> NodeId {
        match self.nodes[n].t {
            NT_ALTERNATE => self.reduce_alternation(n),
            NT_CONCATENATE => self.reduce_concatenation(n),
            NT_LOOP | NT_LAZYLOOP => self.reduce_rep(n),
            NT_GROUP => self.reduce_group(n),
            NT_SET | NT_SETLOOP => self.reduce_set(n),
            _ => n,
        }
    }

    fn reduce_alternation(&mut self, n: NodeId) -> NodeId {
        if self.nodes[n].children.is_empty() {
            let opt = self.nodes[n].options;
            return self.new_node(NT_NOTHING, opt);
        }
        let mut was_last_set = false;
        let mut last_node_cannot_merge = false;
        let mut options_last: RegexOptions = 0;
        let mut i: isize = 0;
        let mut j: isize = 0;
        while (i as usize) < self.nodes[n].children.len() {
            let at = self.nodes[n].children[i as usize];
            if j < i {
                self.nodes[n].children[j as usize] = at;
            }
            let at_t = self.nodes[at].t;
            if at_t == NT_ALTERNATE {
                let at_children = self.nodes[at].children.clone();
                for &k in &at_children {
                    self.nodes[k].next = Some(n);
                }
                self.insert_children(n, (i + 1) as usize, &at_children);
                j -= 1;
            } else if at_t == NT_SET || at_t == NT_ONE {
                let options_at = self.nodes[at].options & (RIGHT_TO_LEFT | IGNORE_CASE);
                let mut merge = true;
                if at_t == NT_SET {
                    let mergeable = self.nodes[at]
                        .set
                        .as_ref()
                        .is_some_and(CharSet::is_mergeable);
                    if !was_last_set
                        || options_last != options_at
                        || last_node_cannot_merge
                        || !mergeable
                    {
                        was_last_set = true;
                        last_node_cannot_merge = !mergeable;
                        options_last = options_at;
                        merge = false;
                    }
                } else if !was_last_set || options_last != options_at || last_node_cannot_merge {
                    was_last_set = true;
                    last_node_cannot_merge = false;
                    options_last = options_at;
                    merge = false;
                }
                if merge {
                    j -= 1;
                    let prev = self.nodes[n].children[j as usize];
                    let mut prev_char_class = if self.nodes[prev].t == NT_ONE {
                        let mut cs = CharSet::default();
                        cs.add_char(self.nodes[prev].ch);
                        cs
                    } else {
                        self.nodes[prev].set.take().unwrap_or_default()
                    };
                    if at_t == NT_ONE {
                        prev_char_class.add_char(self.nodes[at].ch);
                    } else {
                        let at_set = self.nodes[at].set.clone().unwrap_or_default();
                        prev_char_class.add_set(&at_set);
                    }
                    self.nodes[prev].t = NT_SET;
                    self.nodes[prev].set = Some(prev_char_class);
                }
            } else if at_t == NT_NOTHING {
                j -= 1;
            } else {
                was_last_set = false;
                last_node_cannot_merge = false;
            }
            i += 1;
            j += 1;
        }
        if j < i {
            self.remove_children(n, j as usize, i as usize);
        }
        self.strip_enation(n, NT_NOTHING)
    }

    fn reduce_concatenation(&mut self, n: NodeId) -> NodeId {
        if self.nodes[n].children.is_empty() {
            let opt = self.nodes[n].options;
            return self.new_node(NT_EMPTY, opt);
        }
        let mut options_last: RegexOptions = 0;
        let mut was_last_string = false;
        let mut i: isize = 0;
        let mut j: isize = 0;
        while (i as usize) < self.nodes[n].children.len() {
            let at = self.nodes[n].children[i as usize];
            if j < i {
                self.nodes[n].children[j as usize] = at;
            }
            let at_t = self.nodes[at].t;
            if at_t == NT_CONCATENATE
                && (self.nodes[at].options & RIGHT_TO_LEFT)
                    == (self.nodes[n].options & RIGHT_TO_LEFT)
            {
                let at_children = self.nodes[at].children.clone();
                for &k in &at_children {
                    self.nodes[k].next = Some(n);
                }
                self.insert_children(n, (i + 1) as usize, &at_children);
                j -= 1;
            } else if at_t == NT_MULTI || at_t == NT_ONE {
                let options_at = self.nodes[at].options & (RIGHT_TO_LEFT | IGNORE_CASE);
                if !was_last_string || options_last != options_at {
                    was_last_string = true;
                    options_last = options_at;
                    i += 1;
                    j += 1;
                    continue;
                }
                j -= 1;
                let prev = self.nodes[n].children[j as usize];
                if self.nodes[prev].t == NT_ONE {
                    self.nodes[prev].t = NT_MULTI;
                    let ch = self.nodes[prev].ch;
                    self.nodes[prev].str = vec![ch];
                }
                if (options_at & RIGHT_TO_LEFT) == 0 {
                    if at_t == NT_ONE {
                        let ch = self.nodes[at].ch;
                        self.nodes[prev].str.push(ch);
                    } else {
                        let s = self.nodes[at].str.clone();
                        self.nodes[prev].str.extend_from_slice(&s);
                    }
                } else if at_t == NT_ONE {
                    let ch = self.nodes[at].ch;
                    self.nodes[prev].str.insert(0, ch);
                } else {
                    let mut merge = self.nodes[at].str.clone();
                    merge.extend_from_slice(&self.nodes[prev].str);
                    self.nodes[prev].str = merge;
                }
            } else if at_t == NT_EMPTY {
                j -= 1;
            } else {
                was_last_string = false;
            }
            i += 1;
            j += 1;
        }
        if j < i {
            self.remove_children(n, j as usize, i as usize);
        }
        self.strip_enation(n, NT_EMPTY)
    }

    fn reduce_rep(&mut self, n: NodeId) -> NodeId {
        let mut u = n;
        let t = self.nodes[n].t;
        let min = self.nodes[n].m;
        let max = self.nodes[n].n;
        loop {
            if self.nodes[u].children.is_empty() {
                break;
            }
            let child = self.nodes[u].children[0];
            let child_type = self.nodes[child].t;
            if child_type != t
                && !((NT_ONELOOP..=NT_SETLOOP).contains(&child_type) && t == NT_LOOP
                    || (NT_ONELAZY..=NT_SETLAZY).contains(&child_type) && t == NT_LAZYLOOP)
            {
                break;
            }
            if self.nodes[u].m == 0 && self.nodes[child].m > 1
                || self.nodes[child].n < self.nodes[child].m * 2
            {
                break;
            }
            u = child;
            let node = &mut self.nodes[u];
            if node.m > 0 {
                if (MAX_INT32 - 1) / node.m < min {
                    node.m = MAX_INT32;
                } else {
                    node.m *= min;
                }
            }
            if node.n > 0 {
                if (MAX_INT32 - 1) / node.n < max {
                    node.n = MAX_INT32;
                } else {
                    node.n *= max;
                }
            }
        }
        if MAX_INT32 == min {
            let opt = self.nodes[n].options;
            return self.new_node(NT_NOTHING, opt);
        }
        u
    }

    fn strip_enation(&mut self, n: NodeId, empty_type: NodeType) -> NodeId {
        match self.nodes[n].children.len() {
            0 => {
                let opt = self.nodes[n].options;
                self.new_node(empty_type, opt)
            }
            1 => self.nodes[n].children[0],
            _ => n,
        }
    }

    fn reduce_group(&mut self, n: NodeId) -> NodeId {
        let mut u = n;
        while self.nodes[u].t == NT_GROUP {
            u = self.nodes[u].children[0];
        }
        u
    }

    fn reduce_set(&mut self, n: NodeId) -> NodeId {
        let node = &mut self.nodes[n];
        let (singleton, inverse, ch) = match &node.set {
            None => (false, false, 0),
            Some(set) => (set.is_singleton(), set.is_singleton_inverse(), {
                if set.is_singleton() || set.is_singleton_inverse() {
                    set.singleton_char()
                } else {
                    0
                }
            }),
        };
        if node.set.is_none() {
            node.t = NT_NOTHING;
        } else if singleton {
            node.ch = ch;
            node.set = None;
            node.t += NT_ONE - NT_SET;
        } else if inverse {
            node.ch = ch;
            node.set = None;
            node.t += NT_NOTONE - NT_SET;
        }
        n
    }

    pub(crate) fn reverse_left(&mut self, n: NodeId) -> NodeId {
        let node = &mut self.nodes[n];
        if node.options & RIGHT_TO_LEFT != 0
            && node.t == NT_CONCATENATE
            && !node.children.is_empty()
        {
            node.children.reverse();
        }
        n
    }

    pub(crate) fn make_quantifier(
        &mut self,
        n: NodeId,
        lazy: bool,
        min: isize,
        max: isize,
    ) -> NodeId {
        let opt = self.nodes[n].options;
        if min == 0 && max == 0 {
            return self.new_node(NT_EMPTY, opt);
        }
        if min == 1 && max == 1 {
            return n;
        }
        match self.nodes[n].t {
            NT_ONE | NT_NOTONE | NT_SET => {
                if lazy {
                    self.make_rep(n, NT_ONELAZY, min, max);
                } else {
                    self.make_rep(n, NT_ONELOOP, min, max);
                }
                n
            }
            _ => {
                let t = if lazy { NT_LAZYLOOP } else { NT_LOOP };
                let result = self.new_node_mn(t, opt, min, max);
                self.add_child(result, n);
                result
            }
        }
    }
}

const PAD_SPACE: &str = "                                ";

const TYPE_STR: [&str; 43] = [
    "Onerep",
    "Notonerep",
    "Setrep",
    "Oneloop",
    "Notoneloop",
    "Setloop",
    "Onelazy",
    "Notonelazy",
    "Setlazy",
    "One",
    "Notone",
    "Set",
    "Multi",
    "Ref",
    "Bol",
    "Eol",
    "Boundary",
    "Nonboundary",
    "Beginning",
    "Start",
    "EndZ",
    "End",
    "Nothing",
    "Empty",
    "Alternate",
    "Concatenate",
    "Loop",
    "Lazyloop",
    "Capture",
    "Group",
    "Require",
    "Prevent",
    "Greedy",
    "Testref",
    "Testgroup",
    "Unknown",
    "Unknown",
    "Unknown",
    "Unknown",
    "Unknown",
    "Unknown",
    "ECMABoundary",
    "NonECMABoundary",
];

impl RegexTree {
    pub fn dump(&self) -> String {
        dump_nodes(&self.nodes, self.root)
    }
}

pub(crate) fn description(nodes: &[RegexNode], n: NodeId) -> String {
    let node = &nodes[n];
    let mut buf = String::from(TYPE_STR[node.t as usize]);
    if node.options & EXPLICIT_CAPTURE != 0 {
        buf.push_str("-C");
    }
    if node.options & IGNORE_CASE != 0 {
        buf.push_str("-I");
    }
    if node.options & RIGHT_TO_LEFT != 0 {
        buf.push_str("-L");
    }
    if node.options & MULTILINE != 0 {
        buf.push_str("-M");
    }
    if node.options & SINGLELINE != 0 {
        buf.push_str("-S");
    }
    if node.options & IGNORE_PATTERN_WHITESPACE != 0 {
        buf.push_str("-X");
    }
    if node.options & ECMASCRIPT != 0 {
        buf.push_str("-E");
    }
    match node.t {
        NT_ONELOOP | NT_NOTONELOOP | NT_ONELAZY | NT_NOTONELAZY | NT_ONE | NT_NOTONE => {
            buf.push_str("(Ch = ");
            buf.push_str(&char_description(node.ch));
            buf.push(')');
        }
        NT_CAPTURE => {
            buf.push_str(&format!("(index = {}, unindex = {})", node.m, node.n));
        }
        NT_REF | NT_TESTREF => {
            buf.push_str(&format!("(index = {})", node.m));
        }
        NT_MULTI => {
            buf.push_str("(String = ");
            for &r in &node.str {
                push_rune(&mut buf, r);
            }
            buf.push(')');
        }
        NT_SET | NT_SETLOOP | NT_SETLAZY => {
            buf.push_str("(Set = ");
            buf.push_str(&node.set.as_ref().map(CharSet::string).unwrap_or_default());
            buf.push(')');
        }
        _ => {}
    }
    match node.t {
        NT_ONELOOP | NT_NOTONELOOP | NT_ONELAZY | NT_NOTONELAZY | NT_SETLOOP | NT_SETLAZY
        | NT_LOOP | NT_LAZYLOOP => {
            buf.push_str("(Min = ");
            buf.push_str(&node.m.to_string());
            buf.push_str(", Max = ");
            if node.n == MAX_INT32 {
                buf.push_str("inf");
            } else {
                buf.push_str(&node.n.to_string());
            }
            buf.push(')');
        }
        _ => {}
    }
    buf
}

pub(crate) fn dump_nodes(nodes: &[RegexNode], n: NodeId) -> String {
    let mut stack: Vec<usize> = Vec::new();
    let mut cur_node = n;
    let mut cur_child = 0usize;
    let mut buf = description(nodes, cur_node);
    buf.push('\n');
    loop {
        if cur_child < nodes[cur_node].children.len() {
            stack.push(cur_child + 1);
            cur_node = nodes[cur_node].children[cur_child];
            cur_child = 0;
            let depth = stack.len().min(32);
            buf.push_str(&PAD_SPACE[..depth]);
            buf.push_str(&description(nodes, cur_node));
            buf.push('\n');
        } else {
            let Some(top) = stack.pop() else {
                break;
            };
            cur_child = top;
            cur_node = nodes[cur_node].next.expect("a dumped node has a parent");
        }
    }
    buf
}
