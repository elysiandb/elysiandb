#[cfg(test)]
mod collate_test;
mod index;
mod option;
#[cfg(test)]
mod option_test;
mod sort;
#[cfg(test)]
mod sort_test;
mod tables;

use std::cmp::Ordering;

pub use option::{
    AlternateHandling, FORCE, IGNORE_CASE, IGNORE_DIACRITICS, IGNORE_WIDTH, LOOSE, Option,
};
pub use sort::Lister;
pub use tables::{CLDR_VERSION, UNICODE_VERSION};

use super::colltab::{self, Elem, Level, MAX_QUATERNARY, Table, Weighter, make_quaternary};
use super::language::Tag;
use index::{get_table, locale};
use option::Options;

pub struct Collator {
    options: Options,
    t: Table<'static>,
    elems: [Vec<Elem>; 2],
}

#[derive(Default)]
pub struct Buffer {
    key: Vec<u8>,
}

impl Buffer {
    fn init(&mut self) {
        if self.key.capacity() == 0 {
            self.key.reserve(4096);
        }
    }

    pub fn reset(&mut self) {
        self.key.clear();
    }
}

fn new_collator(t: Table<'static>) -> Collator {
    Collator {
        options: Options::new(),
        t,
        elems: [Vec::with_capacity(512), Vec::with_capacity(512)],
    }
}

fn compare_bytes(a: &[u8], b: &[u8]) -> i32 {
    match a.cmp(b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

impl Collator {
    pub fn new(t: Tag, o: &[Option]) -> Collator {
        let index = colltab::match_lang(t, &[]);
        let mut c = new_collator(get_table(locale(index)));
        c.options.set_from_tag(t);
        c.options.set_options(o);
        c
    }

    pub fn compare(&mut self, a: &[u8], b: &[u8]) -> i32 {
        let res = self.compare_elems(a, b);
        if res != 0 {
            return res;
        }
        if !self.options.ignore[Level::Identity as usize] {
            return compare_bytes(a, b);
        }
        0
    }

    pub fn compare_string(&mut self, a: &[u8], b: &[u8]) -> i32 {
        self.compare(a, b)
    }

    fn compare_elems(&mut self, a: &[u8], b: &[u8]) -> i32 {
        let t = &self.t;
        let mut ia = CollIter::new(t, std::mem::take(&mut self.elems[0]), a);
        let mut ib = CollIter::new(t, std::mem::take(&mut self.elems[1]), b);
        let res = self.options.compare(&mut ia, &mut ib);
        self.elems[0] = ia.it.elems;
        self.elems[1] = ib.it.elems;
        res
    }

    pub fn key<'b>(&mut self, buf: &'b mut Buffer, s: &[u8]) -> &'b [u8] {
        buf.init();
        let w = self.get_col_elems(s);
        self.key_from(buf, w)
    }

    pub fn key_from_string<'b>(&mut self, buf: &'b mut Buffer, s: &[u8]) -> &'b [u8] {
        self.key(buf, s)
    }

    fn key_from<'b>(&mut self, buf: &'b mut Buffer, mut w: Vec<Elem>) -> &'b [u8] {
        process_weights(self.options.alternate, self.t.top(), &mut w);
        let kn = buf.key.len();
        self.options.key_from_elems(buf, &w);
        self.elems[0] = w;
        &buf.key[kn..]
    }

    fn get_col_elems(&mut self, s: &[u8]) -> Vec<Elem> {
        let mut i = colltab::Iter::new(&self.t, std::mem::take(&mut self.elems[0]));
        i.set_input(s);
        while i.next() {}
        i.elems
    }
}

struct CollIter<'a> {
    it: colltab::Iter<'a, Table<'static>>,
    pce: usize,
}

impl<'a> CollIter<'a> {
    fn new(t: &'a Table<'static>, elems: Vec<Elem>, s: &'a [u8]) -> Self {
        let mut it = colltab::Iter::new(t, elems);
        it.set_input(s);
        CollIter { it, pce: 0 }
    }

    fn next_primary(&mut self) -> i32 {
        loop {
            while self.pce < self.it.n {
                let v = self.it.elems[self.pce].primary();
                if v != 0 {
                    self.pce += 1;
                    return v;
                }
                self.pce += 1;
            }
            if !self.it.next() {
                return 0;
            }
        }
    }

    fn next_secondary(&mut self) -> i32 {
        while self.pce < self.it.elems.len() {
            let v = self.it.elems[self.pce].secondary();
            if v != 0 {
                self.pce += 1;
                return v;
            }
            self.pce += 1;
        }
        0
    }

    fn prev_secondary(&mut self) -> i32 {
        while self.pce < self.it.elems.len() {
            let v = self.it.elems[self.it.elems.len() - self.pce - 1].secondary();
            if v != 0 {
                self.pce += 1;
                return v;
            }
            self.pce += 1;
        }
        0
    }

    fn next_tertiary(&mut self) -> i32 {
        while self.pce < self.it.elems.len() {
            let v = self.it.elems[self.pce].tertiary();
            if v != 0 {
                self.pce += 1;
                return i32::from(v);
            }
            self.pce += 1;
        }
        0
    }

    fn next_quaternary(&mut self) -> i32 {
        while self.pce < self.it.elems.len() {
            let v = self.it.elems[self.pce].quaternary();
            if v != 0 {
                self.pce += 1;
                return v;
            }
            self.pce += 1;
        }
        0
    }
}

fn compare_level<'c>(
    f: fn(&mut CollIter<'c>) -> i32,
    a: &mut CollIter<'c>,
    b: &mut CollIter<'c>,
) -> i32 {
    a.pce = 0;
    b.pce = 0;
    loop {
        let va = f(a);
        let vb = f(b);
        if va != vb {
            if va < vb {
                return -1;
            }
            return 1;
        } else if va == 0 {
            break;
        }
    }
    0
}

impl Options {
    fn compare<'c>(&self, ia: &mut CollIter<'c>, ib: &mut CollIter<'c>) -> i32 {
        if self.alternate != AlternateHandling::Shifted {
            let res = compare_level(CollIter::next_primary, ia, ib);
            if res != 0 {
                return res;
            }
        }
        if !self.ignore[Level::Secondary as usize] {
            let f: fn(&mut CollIter<'c>) -> i32 = if self.backwards {
                CollIter::prev_secondary
            } else {
                CollIter::next_secondary
            };
            let res = compare_level(f, ia, ib);
            if res != 0 {
                return res;
            }
        }
        if !self.ignore[Level::Tertiary as usize] || self.case_level {
            let res = compare_level(CollIter::next_tertiary, ia, ib);
            if res != 0 {
                return res;
            }
            if !self.ignore[Level::Quaternary as usize] {
                let res = compare_level(CollIter::next_quaternary, ia, ib);
                if res != 0 {
                    return res;
                }
            }
        }
        0
    }

    fn key_from_elems(&self, buf: &mut Buffer, ws: &[Elem]) {
        for v in ws {
            let w = v.primary();
            if w > 0 {
                append_primary(&mut buf.key, w);
            }
        }
        if !self.ignore[Level::Secondary as usize] {
            buf.key.extend_from_slice(&[0, 0]);
            if !self.backwards {
                for v in ws {
                    let w = v.secondary();
                    if w > 0 {
                        buf.key.extend_from_slice(&[(w >> 8) as u8, w as u8]);
                    }
                }
            } else {
                for v in ws.iter().rev() {
                    let w = v.secondary();
                    if w > 0 {
                        buf.key.extend_from_slice(&[(w >> 8) as u8, w as u8]);
                    }
                }
            }
        } else if self.case_level {
            buf.key.extend_from_slice(&[0, 0]);
        }
        if !self.ignore[Level::Tertiary as usize] || self.case_level {
            buf.key.extend_from_slice(&[0, 0]);
            for v in ws {
                let w = v.tertiary();
                if w > 0 {
                    buf.key.push(w);
                }
            }
            if !self.ignore[Level::Quaternary as usize]
                && self.alternate >= AlternateHandling::Shifted
            {
                if self.alternate == AlternateHandling::ShiftTrimmed {
                    let mut last_non_ffff = buf.key.len();
                    buf.key.push(0);
                    for v in ws {
                        let w = v.quaternary();
                        if w == MAX_QUATERNARY {
                            buf.key.push(0xFF);
                        } else if w > 0 {
                            append_primary(&mut buf.key, w);
                            last_non_ffff = buf.key.len();
                        }
                    }
                    buf.key.truncate(last_non_ffff);
                } else {
                    buf.key.push(0);
                    for v in ws {
                        let w = v.quaternary();
                        if w == MAX_QUATERNARY {
                            buf.key.push(0xFF);
                        } else if w > 0 {
                            append_primary(&mut buf.key, w);
                        }
                    }
                }
            }
        }
    }
}

fn append_primary(key: &mut Vec<u8>, p: i32) {
    if p <= 0x7FFF {
        key.extend_from_slice(&[(p >> 8) as u8, p as u8]);
    } else {
        key.extend_from_slice(&[((p >> 16) as u8) | 0x80, (p >> 8) as u8, p as u8]);
    }
}

fn process_weights(vw: AlternateHandling, top: u32, wa: &mut [Elem]) {
    let mut ignore = false;
    let vtop = top as i32;
    match vw {
        AlternateHandling::Shifted | AlternateHandling::ShiftTrimmed => {
            for w in wa.iter_mut() {
                let p = w.primary();
                if p <= vtop && p != 0 {
                    *w = make_quaternary(p);
                    ignore = true;
                } else if p == 0 {
                    if ignore {
                        *w = colltab::IGNORE;
                    }
                } else {
                    ignore = false;
                }
            }
        }
        AlternateHandling::Blanked => {
            for w in wa.iter_mut() {
                let p = w.primary();
                if p <= vtop && (ignore || p != 0) {
                    *w = colltab::IGNORE;
                    ignore = true;
                } else {
                    ignore = false;
                }
            }
        }
        AlternateHandling::NonIgnorable => {}
    }
}
