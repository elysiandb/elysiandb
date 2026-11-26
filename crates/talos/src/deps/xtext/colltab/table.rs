use gocompat::utf8;

use super::collelem::{
    CeType, Elem, MAX_TERTIARY, implicit_primary, make_implicit_ce, split_contract_index,
    split_decompose, split_expand_index,
};
use super::contract::ContractTrieSet;
use super::le_u32;
use super::trie::Trie;
use super::weighter::Weighter;
use crate::deps::xtext::norm::{self, Form};

#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    pub index: Trie<'a>,
    pub expand_elem: &'a [u8],
    pub contract_tries: ContractTrieSet<'a>,
    pub contract_elem: &'a [u8],
    pub max_contract_len: usize,
    pub variable_top: u32,
}

impl Weighter for Table<'_> {
    fn start(&self, _p: usize, _b: &[u8]) -> usize {
        panic!("not implemented")
    }

    fn start_string(&self, _p: usize, _s: &[u8]) -> usize {
        panic!("not implemented")
    }

    fn append_next(&self, w: &mut Vec<Elem>, b: &[u8]) -> usize {
        self.append_next_src(w, b)
    }

    fn append_next_string(&self, w: &mut Vec<Elem>, s: &[u8]) -> usize {
        self.append_next_src(w, s)
    }

    fn domain(&self) -> Vec<String> {
        panic!("not implemented")
    }

    fn top(&self) -> u32 {
        self.variable_top
    }
}

const HANGUL_SIZE: usize = 3;
const FIRST_HANGUL: i32 = 0xAC00;
const LAST_HANGUL: i32 = 0xD7A3;

impl Table<'_> {
    fn append_next_src(&self, w: &mut Vec<Elem>, src: &[u8]) -> usize {
        let (mut ce, mut sz) = self.index.lookup(src);
        let tp = ce.ctype();
        if tp == CeType::Normal {
            if ce.0 == 0 {
                let (r, _) = utf8::decode_rune(src);
                if (FIRST_HANGUL..=LAST_HANGUL).contains(&r) {
                    let n = sz;
                    let buf = Form::Nfd.append_string(Vec::with_capacity(16), &src[..HANGUL_SIZE]);
                    let mut b: &[u8] = &buf;
                    while !b.is_empty() {
                        (ce, sz) = self.index.lookup(b);
                        w.push(ce);
                        b = &b[sz..];
                    }
                    return n;
                }
                ce = make_implicit_ce(implicit_primary(r));
            }
            w.push(ce);
        } else if tp == CeType::ExpansionIndex {
            self.append_expansion(w, ce);
        } else if tp == CeType::ContractionIndex {
            let n = self.match_contraction(w, ce, &src[sz..]);
            sz += n;
        } else if tp == CeType::Decompose {
            let (t1, t2) = split_decompose(ce);
            let mut i = w.len();
            let mut nfkd: &[u8] = Form::Nfkd.properties(src).decomposition().unwrap_or(&[]);
            while !nfkd.is_empty() {
                let p = self.append_next_src(w, nfkd);
                nfkd = &nfkd[p..];
            }
            w[i] = w[i].update_tertiary(t1);
            i += 1;
            if i < w.len() {
                w[i] = w[i].update_tertiary(t2);
                i += 1;
                while i < w.len() {
                    w[i] = w[i].update_tertiary(MAX_TERTIARY);
                    i += 1;
                }
            }
        }
        sz
    }

    fn append_expansion(&self, w: &mut Vec<Elem>, ce: Elem) {
        let mut i = split_expand_index(ce);
        let n = le_u32(self.expand_elem, i) as usize;
        i += 1;
        for k in i..i + n {
            w.push(Elem(le_u32(self.expand_elem, k)));
        }
    }

    fn match_contraction(&self, w: &mut Vec<Elem>, ce: Elem, suffix: &[u8]) -> usize {
        let (index, n, offset) = split_contract_index(ce);
        let mut scan = self.contract_tries.scanner(index, n, suffix);
        let mut buf = [0u8; norm::MAX_SEGMENT_SIZE];
        let mut bufp = 0;
        let mut p = scan.scan(0);
        if !scan.done && p < suffix.len() && suffix[p] >= utf8::RUNE_SELF {
            let mut p0 = p;
            let mut bufn = 0;
            let mut rune = Form::Nfd.properties(&suffix[p..]);
            p += rune.size();
            if rune.lead_ccc() != 0 {
                let mut prev_cc = rune.trail_ccc();
                let end = Form::Nfd.first_boundary(&suffix[p..]);
                if end != -1 {
                    scan.s = &suffix[..p + end as usize];
                }
                while p < suffix.len() && !scan.done && suffix[p] >= utf8::RUNE_SELF {
                    rune = Form::Nfd.properties(&suffix[p..]);
                    let ccc = rune.lead_ccc();
                    if ccc == 0 || prev_cc >= ccc {
                        break;
                    }
                    prev_cc = rune.trail_ccc();
                    let pp = scan.scan(p);
                    if pp != p {
                        let src = &suffix[p0..p];
                        let k = src.len().min(buf.len() - bufn);
                        buf[bufn..bufn + k].copy_from_slice(&src[..k]);
                        bufn += k;
                        if scan.pindex == pp {
                            bufp = bufn;
                        }
                        p = pp;
                        p0 = pp;
                    } else {
                        p += rune.size();
                    }
                }
            }
        }
        let (i, n) = scan.result();
        let ce = Elem(le_u32(self.contract_elem, i + offset));
        if ce.ctype() == CeType::Normal {
            w.push(ce);
        } else {
            self.append_expansion(w, ce);
        }
        let mut b: &[u8] = &buf[..bufp];
        while !b.is_empty() {
            let p = self.append_next_src(w, b);
            b = &b[p..];
        }
        n
    }
}
