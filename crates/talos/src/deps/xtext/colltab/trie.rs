use super::collelem::Elem;
use crate::deps::xtext::colltab::{le_u16, le_u32};

#[derive(Clone, Copy, Debug)]
pub struct Trie<'a> {
    pub index0: &'a [u8],
    pub values0: &'a [u8],
    pub index: &'a [u8],
    pub values: &'a [u8],
}

const TX: u8 = 0x80;
const T2: u8 = 0xC0;
const T3: u8 = 0xE0;
const T4: u8 = 0xF0;
const T5: u8 = 0xF8;

impl Trie<'_> {
    fn lookup_value(&self, n: u16, b: u8) -> Elem {
        Elem(le_u32(self.values, (usize::from(n) << 6) + usize::from(b)))
    }

    pub(super) fn lookup(&self, s: &[u8]) -> (Elem, usize) {
        let c0 = s[0];
        if c0 < TX {
            return (Elem(le_u32(self.values0, usize::from(c0))), 1);
        }
        if c0 < T2 {
            return (Elem(0), 1);
        }
        if c0 < T3 {
            if s.len() < 2 {
                return (Elem(0), 0);
            }
            let i = le_u16(self.index0, usize::from(c0));
            let c1 = s[1];
            if !(TX..T2).contains(&c1) {
                return (Elem(0), 1);
            }
            return (self.lookup_value(i, c1), 2);
        }
        if c0 < T4 {
            if s.len() < 3 {
                return (Elem(0), 0);
            }
            let mut i = le_u16(self.index0, usize::from(c0));
            let c1 = s[1];
            if !(TX..T2).contains(&c1) {
                return (Elem(0), 1);
            }
            let o = (usize::from(i) << 6) + usize::from(c1);
            i = le_u16(self.index, o);
            let c2 = s[2];
            if !(TX..T2).contains(&c2) {
                return (Elem(0), 2);
            }
            return (self.lookup_value(i, c2), 3);
        }
        if c0 < T5 {
            if s.len() < 4 {
                return (Elem(0), 0);
            }
            let mut i = le_u16(self.index0, usize::from(c0));
            let c1 = s[1];
            if !(TX..T2).contains(&c1) {
                return (Elem(0), 1);
            }
            let mut o = (usize::from(i) << 6) + usize::from(c1);
            i = le_u16(self.index, o);
            let c2 = s[2];
            if !(TX..T2).contains(&c2) {
                return (Elem(0), 2);
            }
            o = (usize::from(i) << 6) + usize::from(c2);
            i = le_u16(self.index, o);
            let c3 = s[3];
            if !(TX..T2).contains(&c3) {
                return (Elem(0), 3);
            }
            return (self.lookup_value(i, c3), 4);
        }
        (Elem(0), 1)
    }
}
