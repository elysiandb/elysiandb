use gocompat::utf8;

#[derive(Clone, Copy, Debug)]
pub struct ContractTrieSet<'a>(pub &'a [u8]);

#[derive(Clone, Copy)]
struct Entry {
    l: u8,
    h: u8,
    n: u8,
    i: u8,
}

fn entry(states: &[u8], i: usize) -> Entry {
    Entry {
        l: states[4 * i],
        h: states[4 * i + 1],
        n: states[4 * i + 2],
        i: states[4 * i + 3],
    }
}

pub(super) struct CtScanner<'a, 's> {
    states: &'a [u8],
    pub(super) s: &'s [u8],
    n: usize,
    index: usize,
    pub(super) pindex: usize,
    pub(super) done: bool,
}

const FINAL: u8 = 0;
const NO_INDEX: u8 = 0xFF;

impl<'a> ContractTrieSet<'a> {
    pub(super) fn scanner<'s>(&self, index: usize, n: usize, b: &'s [u8]) -> CtScanner<'a, 's> {
        CtScanner {
            states: &self.0[4 * index..],
            s: b,
            n,
            index: 0,
            pindex: 0,
            done: false,
        }
    }
}

impl CtScanner<'_, '_> {
    pub(super) fn result(&self) -> (usize, usize) {
        (self.index, self.pindex)
    }

    pub(super) fn scan(&mut self, mut p: usize) -> usize {
        let mut pr = p;
        let str = self.s;
        let mut states = self.states;
        let mut n = self.n;
        let mut i = 0;
        while i < n && p < str.len() {
            let e = entry(states, i);
            let c = str[p];
            if c >= e.l {
                if e.l == c {
                    p += 1;
                    if e.i != NO_INDEX {
                        self.index = usize::from(e.i);
                        self.pindex = p;
                    }
                    if e.n != FINAL {
                        states = &states[4 * (usize::from(e.h) + n)..];
                        n = usize::from(e.n);
                        i = 0;
                        if p >= str.len() || utf8::rune_start(str[p]) {
                            self.states = states;
                            self.n = n;
                            pr = p;
                        }
                    } else {
                        self.done = true;
                        return p;
                    }
                    continue;
                } else if e.n == FINAL && c <= e.h {
                    p += 1;
                    self.done = true;
                    self.index = usize::from(c - e.l) + usize::from(e.i);
                    self.pindex = p;
                    return p;
                }
            }
            i += 1;
        }
        pr
    }
}
