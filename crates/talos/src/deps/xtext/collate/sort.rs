use super::{Buffer, Collator};

pub trait Lister {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn swap(&mut self, i: usize, j: usize);

    fn bytes(&self, i: usize) -> &[u8];
}

fn sorted_order(keys: &Buffer, ranges: Vec<(usize, usize)>) -> Vec<usize> {
    let mut entries: Vec<(usize, usize, usize)> = ranges
        .into_iter()
        .enumerate()
        .map(|(i, (a, b))| (a, b, i))
        .collect();
    gocompat::sort::slice(&mut entries, |x, y| keys.key[x.0..x.1] < keys.key[y.0..y.1]);
    entries.into_iter().map(|e| e.2).collect()
}

fn apply_order(order: &[usize], mut swap: impl FnMut(usize, usize)) {
    let n = order.len();
    let mut pos: Vec<usize> = (0..n).collect();
    let mut at: Vec<usize> = (0..n).collect();
    for (k, &want) in order.iter().enumerate() {
        let cur = pos[want];
        if cur != k {
            swap(k, cur);
            let moved = at[k];
            at[k] = want;
            at[cur] = moved;
            pos[want] = k;
            pos[moved] = cur;
        }
    }
}

impl Collator {
    pub fn sort(&mut self, x: &mut dyn Lister) {
        let n = x.len();
        let mut buf = Buffer::default();
        let mut ranges = Vec::with_capacity(n);
        for i in 0..n {
            let start = buf.key.len();
            self.key(&mut buf, x.bytes(i));
            ranges.push((start, buf.key.len()));
        }
        let order = sorted_order(&buf, ranges);
        apply_order(&order, |i, j| x.swap(i, j));
    }

    pub fn sort_strings<T: AsRef<[u8]>>(&mut self, x: &mut [T]) {
        let mut buf = Buffer::default();
        let mut ranges = Vec::with_capacity(x.len());
        for s in x.iter() {
            let start = buf.key.len();
            self.key_from_string(&mut buf, s.as_ref());
            ranges.push((start, buf.key.len()));
        }
        let order = sorted_order(&buf, ranges);
        apply_order(&order, |i, j| x.swap(i, j));
    }
}
