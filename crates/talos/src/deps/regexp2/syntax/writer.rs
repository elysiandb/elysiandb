use std::collections::HashMap;

use gocompat::utf8::Rune;

use super::charclass::CharSet;
use super::code::{
    BACKJUMP, BRANCHCOUNT, BRANCHMARK, CAPTUREMARK, Code, FOREJUMP, GETMARK, GOTO, InstOp,
    LAZYBRANCH, NOTONEREP, NULLCOUNT, NULLMARK, ONEREP, SETCOUNT, SETJUMP, SETMARK, SETREP, STOP,
    TESTREF, opcode_backtracks,
};
use super::code::{CI, RTL};
use super::parser::{DEBUG, IGNORE_CASE, RIGHT_TO_LEFT};
use super::prefix::{BmPrefix, get_anchors, get_first_chars_prefix, get_prefix};
use super::tree::{
    NT_ALTERNATE, NT_BEGINNING, NT_BOL, NT_BOUNDARY, NT_CAPTURE, NT_CONCATENATE, NT_ECMA_BOUNDARY,
    NT_EMPTY, NT_END, NT_END_Z, NT_EOL, NT_GREEDY, NT_GROUP, NT_LAZYLOOP, NT_LOOP, NT_MULTI,
    NT_NON_ECMA_BOUNDARY, NT_NONBOUNDARY, NT_NOTHING, NT_NOTONE, NT_NOTONELAZY, NT_NOTONELOOP,
    NT_ONE, NT_ONELAZY, NT_ONELOOP, NT_PREVENT, NT_REF, NT_REQUIRE, NT_SET, NT_SETLAZY, NT_SETLOOP,
    NT_START, NT_TESTGROUP, NT_TESTREF, NodeId, NodeType, RegexTree,
};
use super::{MAX_INT32, runes_to_string, write_stdout};

pub(crate) const BEFORE_CHILD: NodeType = 64;
pub(crate) const AFTER_CHILD: NodeType = 128;
pub const MAX_PREFIX_SIZE: usize = 50;

pub fn write(tree: &RegexTree) -> Code {
    let mut w = Writer {
        emitted: vec![0; 2],
        int_stack: Vec::with_capacity(32),
        curpos: 0,
        stringhash: HashMap::new(),
        stringtable: Vec::new(),
        sethash: HashMap::new(),
        settable: Vec::new(),
        counting: false,
        count: 0,
        trackcount: 0,
        caps: None,
    };
    let code = w.code_from_tree(tree);
    if tree.options & DEBUG > 0 {
        write_stdout(&code.dump());
        write_stdout("\n");
    }
    code
}

struct Writer {
    emitted: Vec<isize>,
    int_stack: Vec<isize>,
    curpos: usize,
    stringhash: HashMap<String, isize>,
    stringtable: Vec<Vec<Rune>>,
    sethash: HashMap<Vec<u8>, isize>,
    settable: Vec<CharSet>,
    counting: bool,
    count: usize,
    trackcount: isize,
    caps: Option<HashMap<isize, isize>>,
}

const CONCAT_BEFORE: NodeType = NT_CONCATENATE | BEFORE_CHILD;
const CONCAT_AFTER: NodeType = NT_CONCATENATE | AFTER_CHILD;
const ALT_BEFORE: NodeType = NT_ALTERNATE | BEFORE_CHILD;
const ALT_AFTER: NodeType = NT_ALTERNATE | AFTER_CHILD;
const TESTREF_BEFORE: NodeType = NT_TESTREF | BEFORE_CHILD;
const TESTREF_AFTER: NodeType = NT_TESTREF | AFTER_CHILD;
const TESTGROUP_BEFORE: NodeType = NT_TESTGROUP | BEFORE_CHILD;
const TESTGROUP_AFTER: NodeType = NT_TESTGROUP | AFTER_CHILD;
const LOOP_BEFORE: NodeType = NT_LOOP | BEFORE_CHILD;
const LAZYLOOP_BEFORE: NodeType = NT_LAZYLOOP | BEFORE_CHILD;
const LOOP_AFTER: NodeType = NT_LOOP | AFTER_CHILD;
const LAZYLOOP_AFTER: NodeType = NT_LAZYLOOP | AFTER_CHILD;
const GROUP_BEFORE: NodeType = NT_GROUP | BEFORE_CHILD;
const GROUP_AFTER: NodeType = NT_GROUP | AFTER_CHILD;
const CAPTURE_BEFORE: NodeType = NT_CAPTURE | BEFORE_CHILD;
const CAPTURE_AFTER: NodeType = NT_CAPTURE | AFTER_CHILD;
const REQUIRE_BEFORE: NodeType = NT_REQUIRE | BEFORE_CHILD;
const REQUIRE_AFTER: NodeType = NT_REQUIRE | AFTER_CHILD;
const PREVENT_BEFORE: NodeType = NT_PREVENT | BEFORE_CHILD;
const PREVENT_AFTER: NodeType = NT_PREVENT | AFTER_CHILD;
const GREEDY_BEFORE: NodeType = NT_GREEDY | BEFORE_CHILD;
const GREEDY_AFTER: NodeType = NT_GREEDY | AFTER_CHILD;

impl Writer {
    fn code_from_tree(&mut self, tree: &RegexTree) -> Code {
        let capsize: isize;
        match &tree.capnumlist {
            Some(list) if tree.captop != list.len() as isize => {
                capsize = list.len() as isize;
                let mut caps = tree.caps.clone();
                for (i, &k) in list.iter().enumerate() {
                    caps.insert(k, i as isize);
                }
                self.caps = Some(caps);
            }
            _ => {
                capsize = tree.captop;
                self.caps = None;
            }
        }
        self.counting = true;
        loop {
            if !self.counting {
                self.emitted = vec![0; self.count];
            }
            let mut cur_node: NodeId = tree.root;
            let mut cur_child = 0usize;
            self.emit1(LAZYBRANCH, 0);
            loop {
                let len = tree.nodes[cur_node].children.len();
                if len == 0 {
                    self.emit_fragment(tree, tree.nodes[cur_node].t, cur_node, 0);
                } else if cur_child < len {
                    self.emit_fragment(
                        tree,
                        tree.nodes[cur_node].t | BEFORE_CHILD,
                        cur_node,
                        cur_child,
                    );
                    cur_node = tree.nodes[cur_node].children[cur_child];
                    self.push_int(cur_child as isize);
                    cur_child = 0;
                    continue;
                }
                if self.empty_stack() {
                    break;
                }
                cur_child = self.pop_int() as usize;
                cur_node = tree.nodes[cur_node]
                    .next
                    .expect("a written node has a parent");
                self.emit_fragment(
                    tree,
                    tree.nodes[cur_node].t | AFTER_CHILD,
                    cur_node,
                    cur_child,
                );
                cur_child += 1;
            }
            self.patch_jump(0, self.cur_pos());
            self.emit(STOP);
            if !self.counting {
                break;
            }
            self.counting = false;
        }
        let fc_prefix = get_first_chars_prefix(tree);
        let prefix = get_prefix(tree);
        let rtl = tree.options & RIGHT_TO_LEFT != 0;
        let bm_prefix = match prefix {
            Some(mut prefix) if !prefix.prefix_str.is_empty() => {
                if prefix.prefix_str.len() > MAX_PREFIX_SIZE {
                    prefix.prefix_str.truncate(MAX_PREFIX_SIZE);
                }
                BmPrefix::new(prefix.prefix_str, prefix.case_insensitive, rtl)
            }
            _ => None,
        };
        Code {
            codes: std::mem::take(&mut self.emitted),
            strings: std::mem::take(&mut self.stringtable),
            sets: std::mem::take(&mut self.settable),
            track_count: self.trackcount,
            caps: self.caps.take(),
            capsize,
            fc_prefix,
            bm_prefix,
            anchors: get_anchors(tree),
            right_to_left: rtl,
        }
    }

    fn emit_fragment(
        &mut self,
        tree: &RegexTree,
        nodetype: NodeType,
        node_id: NodeId,
        cur_index: usize,
    ) {
        let node = &tree.nodes[node_id];
        let mut bits: InstOp = 0;
        if nodetype <= NT_REF {
            if node.options & RIGHT_TO_LEFT != 0 {
                bits |= RTL;
            }
            if node.options & IGNORE_CASE != 0 {
                bits |= CI;
            }
        }
        let t = node.t as InstOp;
        match nodetype {
            CONCAT_BEFORE | CONCAT_AFTER | NT_EMPTY => {}
            ALT_BEFORE => {
                if cur_index + 1 < node.children.len() {
                    self.push_int(self.cur_pos());
                    self.emit1(LAZYBRANCH, 0);
                }
            }
            ALT_AFTER => {
                if cur_index + 1 < node.children.len() {
                    let lb_pos = self.pop_int();
                    self.push_int(self.cur_pos());
                    self.emit1(GOTO, 0);
                    self.patch_jump(lb_pos, self.cur_pos());
                } else {
                    for _ in 0..cur_index {
                        let p = self.pop_int();
                        self.patch_jump(p, self.cur_pos());
                    }
                }
            }
            TESTREF_BEFORE => {
                if cur_index == 0 {
                    self.emit(SETJUMP);
                    self.push_int(self.cur_pos());
                    self.emit1(LAZYBRANCH, 0);
                    let cap = self.map_capnum(node.m);
                    self.emit1(TESTREF, cap);
                    self.emit(FOREJUMP);
                }
            }
            TESTREF_AFTER => {
                if cur_index == 0 {
                    let branchpos = self.pop_int();
                    self.push_int(self.cur_pos());
                    self.emit1(GOTO, 0);
                    self.patch_jump(branchpos, self.cur_pos());
                    self.emit(FOREJUMP);
                    if node.children.len() <= 1 {
                        let p = self.pop_int();
                        self.patch_jump(p, self.cur_pos());
                    }
                } else if cur_index == 1 {
                    let p = self.pop_int();
                    self.patch_jump(p, self.cur_pos());
                }
            }
            TESTGROUP_BEFORE => {
                if cur_index == 0 {
                    self.emit(SETJUMP);
                    self.emit(SETMARK);
                    self.push_int(self.cur_pos());
                    self.emit1(LAZYBRANCH, 0);
                }
            }
            TESTGROUP_AFTER => {
                if cur_index == 0 {
                    self.emit(GETMARK);
                    self.emit(FOREJUMP);
                } else if cur_index == 1 {
                    let branchpos = self.pop_int();
                    self.push_int(self.cur_pos());
                    self.emit1(GOTO, 0);
                    self.patch_jump(branchpos, self.cur_pos());
                    self.emit(GETMARK);
                    self.emit(FOREJUMP);
                    if node.children.len() <= 2 {
                        let p = self.pop_int();
                        self.patch_jump(p, self.cur_pos());
                    }
                } else if cur_index == 2 {
                    let p = self.pop_int();
                    self.patch_jump(p, self.cur_pos());
                }
            }
            LOOP_BEFORE | LAZYLOOP_BEFORE => {
                if node.n < MAX_INT32 || node.m > 1 {
                    if node.m == 0 {
                        self.emit1(NULLCOUNT, 0);
                    } else {
                        self.emit1(SETCOUNT, 1 - node.m);
                    }
                } else if node.m == 0 {
                    self.emit(NULLMARK);
                } else {
                    self.emit(SETMARK);
                }
                if node.m == 0 {
                    self.push_int(self.cur_pos());
                    self.emit1(GOTO, 0);
                }
                self.push_int(self.cur_pos());
            }
            LOOP_AFTER | LAZYLOOP_AFTER => {
                let start_jump_pos = self.cur_pos();
                let lazy = (nodetype - LOOP_AFTER) as InstOp;
                if node.n < MAX_INT32 || node.m > 1 {
                    let target = self.pop_int();
                    if node.n == MAX_INT32 {
                        self.emit2(BRANCHCOUNT + lazy, target, MAX_INT32);
                    } else {
                        self.emit2(BRANCHCOUNT + lazy, target, node.n - node.m);
                    }
                } else {
                    let target = self.pop_int();
                    self.emit1(BRANCHMARK + lazy, target);
                }
                if node.m == 0 {
                    let p = self.pop_int();
                    self.patch_jump(p, start_jump_pos);
                }
            }
            GROUP_BEFORE | GROUP_AFTER => {}
            CAPTURE_BEFORE => self.emit(SETMARK),
            CAPTURE_AFTER => {
                let m = self.map_capnum(node.m);
                let n = self.map_capnum(node.n);
                self.emit2(CAPTUREMARK, m, n);
            }
            REQUIRE_BEFORE => {
                self.emit(SETJUMP);
                self.emit(SETMARK);
            }
            REQUIRE_AFTER => {
                self.emit(GETMARK);
                self.emit(FOREJUMP);
            }
            PREVENT_BEFORE => {
                self.emit(SETJUMP);
                self.push_int(self.cur_pos());
                self.emit1(LAZYBRANCH, 0);
            }
            PREVENT_AFTER => {
                self.emit(BACKJUMP);
                let p = self.pop_int();
                self.patch_jump(p, self.cur_pos());
                self.emit(FOREJUMP);
            }
            GREEDY_BEFORE => self.emit(SETJUMP),
            GREEDY_AFTER => self.emit(FOREJUMP),
            NT_ONE | NT_NOTONE => self.emit1(t | bits, node.ch as isize),
            NT_NOTONELOOP | NT_NOTONELAZY | NT_ONELOOP | NT_ONELAZY => {
                if node.m > 0 {
                    if node.t == NT_ONELOOP || node.t == NT_ONELAZY {
                        self.emit2(ONEREP | bits, node.ch as isize, node.m);
                    } else {
                        self.emit2(NOTONEREP | bits, node.ch as isize, node.m);
                    }
                }
                if node.n > node.m {
                    if node.n == MAX_INT32 {
                        self.emit2(t | bits, node.ch as isize, MAX_INT32);
                    } else {
                        self.emit2(t | bits, node.ch as isize, node.n - node.m);
                    }
                }
            }
            NT_SETLOOP | NT_SETLAZY => {
                if node.m > 0 {
                    let set = self.set_code(node.set.as_ref());
                    self.emit2(SETREP | bits, set, node.m);
                }
                if node.n > node.m {
                    let set = self.set_code(node.set.as_ref());
                    if node.n == MAX_INT32 {
                        self.emit2(t | bits, set, MAX_INT32);
                    } else {
                        self.emit2(t | bits, set, node.n - node.m);
                    }
                }
            }
            NT_MULTI => {
                let s = self.string_code(&node.str);
                self.emit1(t | bits, s);
            }
            NT_SET => {
                let set = self.set_code(node.set.as_ref());
                self.emit1(t | bits, set);
            }
            NT_REF => {
                let cap = self.map_capnum(node.m);
                self.emit1(t | bits, cap);
            }
            NT_NOTHING | NT_BOL | NT_EOL | NT_BOUNDARY | NT_NONBOUNDARY | NT_ECMA_BOUNDARY
            | NT_NON_ECMA_BOUNDARY | NT_BEGINNING | NT_START | NT_END_Z | NT_END => self.emit(t),
            _ => {}
        }
    }

    fn push_int(&mut self, i: isize) {
        self.int_stack.push(i);
    }

    fn empty_stack(&self) -> bool {
        self.int_stack.is_empty()
    }

    fn pop_int(&mut self) -> isize {
        self.int_stack.pop().expect("a pushed jump")
    }

    fn cur_pos(&self) -> isize {
        self.curpos as isize
    }

    fn patch_jump(&mut self, offset: isize, jump_dest: isize) {
        self.emitted[(offset + 1) as usize] = jump_dest;
    }

    fn set_code(&mut self, set: Option<&CharSet>) -> isize {
        if self.counting {
            return 0;
        }
        let set = set.expect("a set node holds a set");
        let mut buf = Vec::new();
        set.map_hash_fill(&mut buf);
        if let Some(&i) = self.sethash.get(&buf) {
            return i;
        }
        let i = self.sethash.len() as isize;
        self.sethash.insert(buf, i);
        self.settable.push(set.clone());
        i
    }

    fn string_code(&mut self, str: &[Rune]) -> isize {
        if self.counting {
            return 0;
        }
        let hash = runes_to_string(str);
        if let Some(&i) = self.stringhash.get(&hash) {
            return i;
        }
        let i = self.stringhash.len() as isize;
        self.stringhash.insert(hash, i);
        self.stringtable.push(str.to_vec());
        i
    }

    fn map_capnum(&self, capnum: isize) -> isize {
        if capnum == -1 {
            return -1;
        }
        if let Some(caps) = &self.caps {
            return caps.get(&capnum).copied().unwrap_or(0);
        }
        capnum
    }

    fn emit(&mut self, op: InstOp) {
        if self.counting {
            self.count += 1;
            if opcode_backtracks(op) {
                self.trackcount += 1;
            }
            return;
        }
        self.emitted[self.curpos] = op;
        self.curpos += 1;
    }

    fn emit1(&mut self, op: InstOp, opd1: isize) {
        if self.counting {
            self.count += 2;
            if opcode_backtracks(op) {
                self.trackcount += 1;
            }
            return;
        }
        self.emitted[self.curpos] = op;
        self.curpos += 1;
        self.emitted[self.curpos] = opd1;
        self.curpos += 1;
    }

    fn emit2(&mut self, op: InstOp, opd1: isize, opd2: isize) {
        if self.counting {
            self.count += 3;
            if opcode_backtracks(op) {
                self.trackcount += 1;
            }
            return;
        }
        self.emitted[self.curpos] = op;
        self.curpos += 1;
        self.emitted[self.curpos] = opd1;
        self.curpos += 1;
        self.emitted[self.curpos] = opd2;
        self.curpos += 1;
    }
}
