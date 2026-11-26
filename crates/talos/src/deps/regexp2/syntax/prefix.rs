use gocompat::unicode;
use gocompat::utf8::{self, Rune};

use super::charclass::{CharSet, any_class};
use super::escape::escape;
use super::parser::{IGNORE_CASE, RIGHT_TO_LEFT};
use super::runes_to_string;
use super::tree::{
    NT_ALTERNATE, NT_BEGINNING, NT_BOL, NT_BOUNDARY, NT_CAPTURE, NT_CONCATENATE, NT_ECMA_BOUNDARY,
    NT_EMPTY, NT_END, NT_END_Z, NT_EOL, NT_GREEDY, NT_GROUP, NT_LAZYLOOP, NT_LOOP, NT_MULTI,
    NT_NON_ECMA_BOUNDARY, NT_NONBOUNDARY, NT_NOTHING, NT_NOTONE, NT_NOTONELAZY, NT_NOTONELOOP,
    NT_ONE, NT_ONELAZY, NT_ONELOOP, NT_PREVENT, NT_REF, NT_REQUIRE, NT_SET, NT_SETLAZY, NT_SETLOOP,
    NT_START, NT_TESTGROUP, NT_TESTREF, NodeId, NodeType, RegexTree,
};
use super::writer::{AFTER_CHILD, BEFORE_CHILD, MAX_PREFIX_SIZE};

#[derive(Clone, Debug, Default)]
pub struct Prefix {
    pub prefix_str: Vec<Rune>,
    pub prefix_set: CharSet,
    pub case_insensitive: bool,
}

pub(crate) fn get_first_chars_prefix(tree: &RegexTree) -> Option<Prefix> {
    let mut s = RegexFcd {
        int_stack: vec![0; 32],
        int_depth: 0,
        fc_stack: vec![RegexFc::default(); 32],
        fc_depth: 0,
        skip_all_children: false,
        skipchild: false,
        failed: false,
    };
    let fc = s.regex_fc_from_regex_tree(tree)?;
    if fc.nullable || fc.cc.is_empty() {
        return None;
    }
    let mut fc = fc;
    let fc_set = fc.get_first_chars();
    Some(Prefix {
        prefix_str: Vec::new(),
        prefix_set: fc_set,
        case_insensitive: fc.case_insensitive,
    })
}

struct RegexFcd {
    int_stack: Vec<usize>,
    int_depth: usize,
    fc_stack: Vec<RegexFc>,
    fc_depth: usize,
    skip_all_children: bool,
    skipchild: bool,
    failed: bool,
}

impl RegexFcd {
    fn regex_fc_from_regex_tree(&mut self, tree: &RegexTree) -> Option<RegexFc> {
        let nodes = &tree.nodes;
        let mut cur_node = tree.root;
        let mut cur_child = 0usize;
        loop {
            if nodes[cur_node].children.is_empty() {
                self.calculate_fc(tree, nodes[cur_node].t, cur_node, 0);
            } else if cur_child < nodes[cur_node].children.len() && !self.skip_all_children {
                self.calculate_fc(tree, nodes[cur_node].t | BEFORE_CHILD, cur_node, cur_child);
                if !self.skipchild {
                    cur_node = nodes[cur_node].children[cur_child];
                    self.push_int(cur_child);
                    cur_child = 0;
                } else {
                    cur_child += 1;
                    self.skipchild = false;
                }
                continue;
            }
            self.skip_all_children = false;
            if self.int_is_empty() {
                break;
            }
            cur_child = self.pop_int();
            cur_node = nodes[cur_node].next.expect("a walked node has a parent");
            self.calculate_fc(tree, nodes[cur_node].t | AFTER_CHILD, cur_node, cur_child);
            if self.failed {
                return None;
            }
            cur_child += 1;
        }
        if self.fc_is_empty() {
            return None;
        }
        Some(self.pop_fc())
    }

    fn push_int(&mut self, i: usize) {
        if self.int_depth >= self.int_stack.len() {
            let mut expanded = vec![0; self.int_depth * 2];
            expanded[..self.int_stack.len()].copy_from_slice(&self.int_stack);
            self.int_stack = expanded;
        }
        self.int_stack[self.int_depth] = i;
        self.int_depth += 1;
    }

    fn int_is_empty(&self) -> bool {
        self.int_depth == 0
    }

    fn pop_int(&mut self) -> usize {
        self.int_depth -= 1;
        self.int_stack[self.int_depth]
    }

    fn push_fc(&mut self, fc: RegexFc) {
        if self.fc_depth >= self.fc_stack.len() {
            let mut expanded = vec![RegexFc::default(); self.fc_depth * 2];
            for (i, f) in self.fc_stack.drain(..).enumerate() {
                expanded[i] = f;
            }
            self.fc_stack = expanded;
        }
        self.fc_stack[self.fc_depth] = fc;
        self.fc_depth += 1;
    }

    fn fc_is_empty(&self) -> bool {
        self.fc_depth == 0
    }

    fn pop_fc(&mut self) -> RegexFc {
        self.fc_depth -= 1;
        self.fc_stack[self.fc_depth].clone()
    }

    fn top_fc(&mut self) -> &mut RegexFc {
        &mut self.fc_stack[self.fc_depth - 1]
    }

    fn skip_child(&mut self) {
        self.skipchild = true;
    }

    fn calculate_fc(&mut self, tree: &RegexTree, nt: NodeType, node_id: NodeId, cur_index: usize) {
        let node = &tree.nodes[node_id];
        let mut ci = false;
        let mut rtl = false;
        if nt <= NT_REF {
            if node.options & IGNORE_CASE != 0 {
                ci = true;
            }
            if node.options & RIGHT_TO_LEFT != 0 {
                rtl = true;
            }
        }
        const CONCAT_BEFORE: NodeType = NT_CONCATENATE | BEFORE_CHILD;
        const ALT_BEFORE: NodeType = NT_ALTERNATE | BEFORE_CHILD;
        const TESTREF_BEFORE: NodeType = NT_TESTREF | BEFORE_CHILD;
        const LOOP_BEFORE: NodeType = NT_LOOP | BEFORE_CHILD;
        const LAZYLOOP_BEFORE: NodeType = NT_LAZYLOOP | BEFORE_CHILD;
        const TESTGROUP_BEFORE: NodeType = NT_TESTGROUP | BEFORE_CHILD;
        const CONCAT_AFTER: NodeType = NT_CONCATENATE | AFTER_CHILD;
        const TESTGROUP_AFTER: NodeType = NT_TESTGROUP | AFTER_CHILD;
        const ALT_AFTER: NodeType = NT_ALTERNATE | AFTER_CHILD;
        const TESTREF_AFTER: NodeType = NT_TESTREF | AFTER_CHILD;
        const LOOP_AFTER: NodeType = NT_LOOP | AFTER_CHILD;
        const LAZYLOOP_AFTER: NodeType = NT_LAZYLOOP | AFTER_CHILD;
        const GROUP_BEFORE: NodeType = NT_GROUP | BEFORE_CHILD;
        const GROUP_AFTER: NodeType = NT_GROUP | AFTER_CHILD;
        const CAPTURE_BEFORE: NodeType = NT_CAPTURE | BEFORE_CHILD;
        const CAPTURE_AFTER: NodeType = NT_CAPTURE | AFTER_CHILD;
        const GREEDY_BEFORE: NodeType = NT_GREEDY | BEFORE_CHILD;
        const GREEDY_AFTER: NodeType = NT_GREEDY | AFTER_CHILD;
        const REQUIRE_BEFORE: NodeType = NT_REQUIRE | BEFORE_CHILD;
        const PREVENT_BEFORE: NodeType = NT_PREVENT | BEFORE_CHILD;
        const REQUIRE_AFTER: NodeType = NT_REQUIRE | AFTER_CHILD;
        const PREVENT_AFTER: NodeType = NT_PREVENT | AFTER_CHILD;
        match nt {
            CONCAT_BEFORE | ALT_BEFORE | TESTREF_BEFORE | LOOP_BEFORE | LAZYLOOP_BEFORE => {}
            TESTGROUP_BEFORE => {
                if cur_index == 0 {
                    self.skip_child();
                }
            }
            NT_EMPTY => self.push_fc(RegexFc::nullable()),
            CONCAT_AFTER => {
                if cur_index != 0 {
                    let child = self.pop_fc();
                    let cumul = self.top_fc();
                    let ok = cumul.add_fc(&child, true);
                    self.failed = !ok;
                }
                if !self.top_fc().nullable {
                    self.skip_all_children = true;
                }
            }
            TESTGROUP_AFTER => {
                if cur_index > 1 {
                    let child = self.pop_fc();
                    let cumul = self.top_fc();
                    let ok = cumul.add_fc(&child, false);
                    self.failed = !ok;
                }
            }
            ALT_AFTER | TESTREF_AFTER => {
                if cur_index != 0 {
                    let child = self.pop_fc();
                    let cumul = self.top_fc();
                    let ok = cumul.add_fc(&child, false);
                    self.failed = !ok;
                }
            }
            LOOP_AFTER | LAZYLOOP_AFTER => {
                if node.m == 0 {
                    self.top_fc().nullable = true;
                }
            }
            GROUP_BEFORE | GROUP_AFTER | CAPTURE_BEFORE | CAPTURE_AFTER | GREEDY_BEFORE
            | GREEDY_AFTER => {}
            REQUIRE_BEFORE | PREVENT_BEFORE => {
                self.skip_child();
                self.push_fc(RegexFc::nullable());
            }
            REQUIRE_AFTER | PREVENT_AFTER => {}
            NT_ONE | NT_NOTONE => self.push_fc(RegexFc::new(node.ch, nt == NT_NOTONE, false, ci)),
            NT_ONELOOP | NT_ONELAZY => self.push_fc(RegexFc::new(node.ch, false, node.m == 0, ci)),
            NT_NOTONELOOP | NT_NOTONELAZY => {
                self.push_fc(RegexFc::new(node.ch, true, node.m == 0, ci))
            }
            NT_MULTI => {
                if node.str.is_empty() {
                    self.push_fc(RegexFc::nullable());
                } else if !rtl {
                    self.push_fc(RegexFc::new(node.str[0], false, false, ci));
                } else {
                    self.push_fc(RegexFc::new(node.str[node.str.len() - 1], false, false, ci));
                }
            }
            NT_SET => self.push_fc(RegexFc {
                cc: node.set.as_ref().map(CharSet::copy).unwrap_or_default(),
                nullable: false,
                case_insensitive: ci,
            }),
            NT_SETLOOP | NT_SETLAZY => self.push_fc(RegexFc {
                cc: node.set.as_ref().map(CharSet::copy).unwrap_or_default(),
                nullable: node.m == 0,
                case_insensitive: ci,
            }),
            NT_REF => self.push_fc(RegexFc {
                cc: any_class(),
                nullable: true,
                case_insensitive: false,
            }),
            NT_NOTHING | NT_BOL | NT_EOL | NT_BOUNDARY | NT_NONBOUNDARY | NT_ECMA_BOUNDARY
            | NT_NON_ECMA_BOUNDARY | NT_BEGINNING | NT_START | NT_END_Z | NT_END => {
                self.push_fc(RegexFc::nullable())
            }
            _ => panic!("unexpected op code: {nt}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct RegexFc {
    cc: CharSet,
    nullable: bool,
    case_insensitive: bool,
}

impl RegexFc {
    fn nullable() -> RegexFc {
        RegexFc {
            nullable: true,
            ..RegexFc::default()
        }
    }

    fn new(ch: Rune, not: bool, nullable: bool, case_insensitive: bool) -> RegexFc {
        let mut r = RegexFc {
            case_insensitive,
            nullable,
            ..RegexFc::default()
        };
        if not {
            if ch > 0 {
                r.cc.add_range(0, ch - 1);
            }
            if ch < 0xFFFF {
                r.cc.add_range(ch + 1, utf8::MAX_RUNE);
            }
        } else {
            r.cc.add_range(ch, ch);
        }
        r
    }

    fn get_first_chars(&mut self) -> CharSet {
        if self.case_insensitive {
            self.cc.add_lowercase();
        }
        self.cc.clone()
    }

    fn add_fc(&mut self, fc: &RegexFc, concatenate: bool) -> bool {
        if !self.cc.is_mergeable() || !fc.cc.is_mergeable() {
            return false;
        }
        if concatenate {
            if !self.nullable {
                return true;
            }
            if !fc.nullable {
                self.nullable = false;
            }
        } else if fc.nullable {
            self.nullable = true;
        }
        self.case_insensitive = self.case_insensitive || fc.case_insensitive;
        self.cc.add_set(&fc.cc);
        true
    }
}

pub(crate) fn get_prefix(tree: &RegexTree) -> Option<Prefix> {
    let nodes = &tree.nodes;
    let mut concat_node: Option<NodeId> = None;
    let mut next_child = 0usize;
    let mut cur_node = tree.root;
    loop {
        let node = &nodes[cur_node];
        match node.t {
            NT_CONCATENATE => {
                if !node.children.is_empty() {
                    concat_node = Some(cur_node);
                    next_child = 0;
                }
            }
            NT_GREEDY | NT_CAPTURE => {
                cur_node = node.children[0];
                concat_node = None;
                continue;
            }
            NT_ONELOOP | NT_ONELAZY => {
                if node.m > 0 {
                    return Some(Prefix {
                        prefix_str: repeat(node.ch, node.m),
                        prefix_set: CharSet::default(),
                        case_insensitive: node.options & IGNORE_CASE != 0,
                    });
                }
                return None;
            }
            NT_ONE => {
                return Some(Prefix {
                    prefix_str: vec![node.ch],
                    prefix_set: CharSet::default(),
                    case_insensitive: node.options & IGNORE_CASE != 0,
                });
            }
            NT_MULTI => {
                return Some(Prefix {
                    prefix_str: node.str.clone(),
                    prefix_set: CharSet::default(),
                    case_insensitive: node.options & IGNORE_CASE != 0,
                });
            }
            NT_BOL | NT_EOL | NT_BOUNDARY | NT_ECMA_BOUNDARY | NT_BEGINNING | NT_START
            | NT_END_Z | NT_END | NT_EMPTY | NT_REQUIRE | NT_PREVENT => {}
            _ => return None,
        }
        let concat = concat_node?;
        if next_child >= nodes[concat].children.len() {
            return None;
        }
        cur_node = nodes[concat].children[next_child];
        next_child += 1;
    }
}

fn repeat(r: Rune, c: isize) -> Vec<Rune> {
    let c = if c > MAX_PREFIX_SIZE as isize {
        MAX_PREFIX_SIZE
    } else {
        c as usize
    };
    vec![r; c]
}

#[derive(Clone, Debug)]
pub struct BmPrefix {
    positive: Vec<isize>,
    negative_ascii: Vec<isize>,
    negative_unicode: Option<Vec<Option<Vec<isize>>>>,
    unicode_zero_is_ascii: bool,
    pattern: Vec<Rune>,
    low_ascii: Rune,
    high_ascii: Rune,
    right_to_left: bool,
    case_insensitive: bool,
}

impl BmPrefix {
    pub(crate) fn new(
        pattern: Vec<Rune>,
        case_insensitive: bool,
        right_to_left: bool,
    ) -> Option<BmPrefix> {
        let mut b = BmPrefix {
            positive: Vec::new(),
            negative_ascii: Vec::new(),
            negative_unicode: None,
            unicode_zero_is_ascii: false,
            pattern,
            low_ascii: 0,
            high_ascii: 0,
            right_to_left,
            case_insensitive,
        };
        if case_insensitive {
            for r in &mut b.pattern {
                *r = unicode::to_lower(*r);
            }
        }
        let beforefirst: isize;
        let last: isize;
        let bump: isize;
        let plen = b.pattern.len() as isize;
        if !right_to_left {
            beforefirst = -1;
            last = plen - 1;
            bump = 1;
        } else {
            beforefirst = plen;
            last = 0;
            bump = -1;
        }
        b.positive = vec![0; b.pattern.len()];
        let mut examine = last;
        let mut ch = b.pattern[examine as usize];
        b.positive[examine as usize] = bump;
        examine -= bump;
        let mut scan: isize;
        let mut matched: isize;
        'outerloop: loop {
            loop {
                if examine == beforefirst {
                    break 'outerloop;
                }
                if b.pattern[examine as usize] == ch {
                    break;
                }
                examine -= bump;
            }
            matched = last;
            scan = examine;
            loop {
                if scan == beforefirst || b.pattern[matched as usize] != b.pattern[scan as usize] {
                    if b.positive[matched as usize] == 0 {
                        b.positive[matched as usize] = matched - scan;
                    }
                    break;
                }
                scan -= bump;
                matched -= bump;
            }
            examine -= bump;
        }
        matched = last - bump;
        while matched != beforefirst {
            if b.positive[matched as usize] == 0 {
                b.positive[matched as usize] = bump;
            }
            matched -= bump;
        }
        b.negative_ascii = vec![last - beforefirst; 128];
        b.low_ascii = 127;
        b.high_ascii = 0;
        examine = last;
        while examine != beforefirst {
            ch = b.pattern[examine as usize];
            if ch < 128 {
                if b.low_ascii > ch {
                    b.low_ascii = ch;
                }
                if b.high_ascii < ch {
                    b.high_ascii = ch;
                }
                if b.negative_ascii[ch as usize] == last - beforefirst {
                    b.negative_ascii[ch as usize] = last - examine;
                }
            } else if ch <= 0xffff {
                let i = (ch >> 8) as usize;
                let j = (ch & 0xFF) as usize;
                let tables = b.negative_unicode.get_or_insert_with(|| vec![None; 256]);
                if tables[i].is_none() && !(i == 0 && b.unicode_zero_is_ascii) {
                    let mut newarray = vec![last - beforefirst; 256];
                    if i == 0 {
                        newarray[..b.negative_ascii.len()].copy_from_slice(&b.negative_ascii);
                        b.negative_ascii = newarray;
                        b.unicode_zero_is_ascii = true;
                    } else {
                        tables[i] = Some(newarray);
                    }
                }
                let slot = if i == 0 {
                    &mut b.negative_ascii[j]
                } else {
                    &mut tables[i].as_mut().expect("a unicode table")[j]
                };
                if *slot == last - beforefirst {
                    *slot = last - examine;
                }
            } else {
                return None;
            }
            examine -= bump;
        }
        Some(b)
    }

    fn unicode_lookup(&self, hi: usize) -> Option<&[isize]> {
        let tables = self.negative_unicode.as_ref()?;
        if hi == 0 && self.unicode_zero_is_ascii {
            return Some(&self.negative_ascii);
        }
        tables[hi].as_deref()
    }

    fn has_unicode_tables(&self) -> bool {
        self.negative_unicode.is_some()
    }

    pub fn string(&self) -> String {
        runes_to_string(&self.pattern)
    }

    pub fn dump(&self, indent: &str) -> String {
        let mut buf = format!(
            "{indent}BM Pattern: {}\n{indent}Positive: ",
            runes_to_string(&self.pattern)
        );
        for p in &self.positive {
            buf.push_str(&p.to_string());
            buf.push(' ');
        }
        buf.push('\n');
        buf.push_str(indent);
        buf.push_str("Negative table\n");
        for (i, &v) in self.negative_ascii.iter().enumerate() {
            if v != self.pattern.len() as isize {
                buf.push_str(&format!(
                    "{indent}  {} {}\n",
                    escape(runes_to_string(&[i as Rune]).as_bytes()),
                    v
                ));
            }
        }
        buf
    }

    pub fn scan(&self, text: &[Rune], index: isize, beglimit: isize, endlimit: isize) -> isize {
        let defadv: isize;
        let startmatch: isize;
        let endmatch: isize;
        let mut test: isize;
        let bump: isize;
        let plen = self.pattern.len() as isize;
        if !self.right_to_left {
            defadv = plen;
            startmatch = plen - 1;
            endmatch = 0;
            test = index + defadv - 1;
            bump = 1;
        } else {
            defadv = -plen;
            startmatch = 0;
            endmatch = -defadv - 1;
            test = index + defadv;
            bump = -1;
        }
        let ch_match = self.pattern[startmatch as usize];
        loop {
            if test >= endlimit || test < beglimit {
                return -1;
            }
            let mut ch_test = text[test as usize];
            if self.case_insensitive {
                ch_test = unicode::to_lower(ch_test);
            }
            if ch_test != ch_match {
                let advance = if ch_test < 128 {
                    self.negative_ascii[ch_test as usize]
                } else if ch_test < 0xffff && self.has_unicode_tables() {
                    match self.unicode_lookup((ch_test >> 8) as usize) {
                        Some(lookup) if !lookup.is_empty() => lookup[(ch_test & 0xFF) as usize],
                        _ => defadv,
                    }
                } else {
                    defadv
                };
                test += advance;
            } else {
                let mut test2 = test;
                let mut matched = startmatch;
                loop {
                    if matched == endmatch {
                        if self.right_to_left {
                            return test2 + 1;
                        }
                        return test2;
                    }
                    matched -= bump;
                    test2 -= bump;
                    let mut ch_test = text[test2 as usize];
                    if self.case_insensitive {
                        ch_test = unicode::to_lower(ch_test);
                    }
                    if ch_test != self.pattern[matched as usize] {
                        let mut advance = self.positive[matched as usize];
                        if ch_test < 128 {
                            test2 = (matched - startmatch) + self.negative_ascii[ch_test as usize];
                        } else if ch_test < 0xffff && self.has_unicode_tables() {
                            match self.unicode_lookup((ch_test >> 8) as usize) {
                                Some(lookup) if !lookup.is_empty() => {
                                    test2 =
                                        (matched - startmatch) + lookup[(ch_test & 0xFF) as usize];
                                }
                                _ => {
                                    test += advance;
                                    break;
                                }
                            }
                        } else {
                            test += advance;
                            break;
                        }
                        if self.right_to_left {
                            if test2 < advance {
                                advance = test2;
                            }
                        } else if test2 > advance {
                            advance = test2;
                        }
                        test += advance;
                        break;
                    }
                }
            }
        }
    }

    pub fn is_match(&self, text: &[Rune], index: isize, beglimit: isize, endlimit: isize) -> bool {
        let plen = self.pattern.len() as isize;
        if !self.right_to_left {
            if index < beglimit || endlimit - index < plen {
                return false;
            }
            self.match_pattern(text, index)
        } else {
            if index > endlimit || index - beglimit < plen {
                return false;
            }
            self.match_pattern(text, index - plen)
        }
    }

    fn match_pattern(&self, text: &[Rune], index: isize) -> bool {
        if (text.len() as isize) - index < self.pattern.len() as isize {
            return false;
        }
        if self.case_insensitive {
            for (i, &p) in self.pattern.iter().enumerate() {
                if unicode::to_lower(text[index as usize + i]) != p {
                    return false;
                }
            }
            true
        } else {
            for (i, &p) in self.pattern.iter().enumerate() {
                if text[index as usize + i] != p {
                    return false;
                }
            }
            true
        }
    }
}

pub type AnchorLoc = i16;

pub const ANCHOR_BEGINNING: AnchorLoc = 0x0001;
pub const ANCHOR_BOL: AnchorLoc = 0x0002;
pub const ANCHOR_START: AnchorLoc = 0x0004;
pub const ANCHOR_EOL: AnchorLoc = 0x0008;
pub const ANCHOR_END_Z: AnchorLoc = 0x0010;
pub const ANCHOR_END: AnchorLoc = 0x0020;
pub const ANCHOR_BOUNDARY: AnchorLoc = 0x0040;
pub const ANCHOR_ECMA_BOUNDARY: AnchorLoc = 0x0080;

pub(crate) fn get_anchors(tree: &RegexTree) -> AnchorLoc {
    let nodes = &tree.nodes;
    let mut concat_node: Option<NodeId> = None;
    let mut next_child = 0usize;
    let result: AnchorLoc = 0;
    let mut cur_node = tree.root;
    loop {
        let node = &nodes[cur_node];
        match node.t {
            NT_CONCATENATE => {
                if !node.children.is_empty() {
                    concat_node = Some(cur_node);
                    next_child = 0;
                }
            }
            NT_GREEDY | NT_CAPTURE => {
                cur_node = node.children[0];
                concat_node = None;
                continue;
            }
            NT_BOL | NT_EOL | NT_BOUNDARY | NT_ECMA_BOUNDARY | NT_BEGINNING | NT_START
            | NT_END_Z | NT_END => return result | anchor_from_type(node.t),
            NT_EMPTY | NT_REQUIRE | NT_PREVENT => {}
            _ => return result,
        }
        let Some(concat) = concat_node else {
            return result;
        };
        if next_child >= nodes[concat].children.len() {
            return result;
        }
        cur_node = nodes[concat].children[next_child];
        next_child += 1;
    }
}

fn anchor_from_type(t: NodeType) -> AnchorLoc {
    match t {
        NT_BOL => ANCHOR_BOL,
        NT_EOL => ANCHOR_EOL,
        NT_BOUNDARY => ANCHOR_BOUNDARY,
        NT_ECMA_BOUNDARY => ANCHOR_ECMA_BOUNDARY,
        NT_BEGINNING => ANCHOR_BEGINNING,
        NT_START => ANCHOR_START,
        NT_END_Z => ANCHOR_END_Z,
        NT_END => ANCHOR_END,
        _ => 0,
    }
}

pub fn anchor_description(anchors: AnchorLoc) -> String {
    let mut buf = String::new();
    if anchors & ANCHOR_BEGINNING != 0 {
        buf.push_str(", Beginning");
    }
    if anchors & ANCHOR_START != 0 {
        buf.push_str(", Start");
    }
    if anchors & ANCHOR_BOL != 0 {
        buf.push_str(", Bol");
    }
    if anchors & ANCHOR_BOUNDARY != 0 {
        buf.push_str(", Boundary");
    }
    if anchors & ANCHOR_ECMA_BOUNDARY != 0 {
        buf.push_str(", ECMABoundary");
    }
    if anchors & ANCHOR_EOL != 0 {
        buf.push_str(", Eol");
    }
    if anchors & ANCHOR_END != 0 {
        buf.push_str(", End");
    }
    if anchors & ANCHOR_END_Z != 0 {
        buf.push_str(", EndZ");
    }
    if buf.len() >= 2 {
        return buf[2..].to_string();
    }
    "None".to_string()
}
