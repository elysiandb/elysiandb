use gocompat::unicode::{MAX_LATIN1, MAX_RUNE, Range16, Range32, RangeTable};

const AT_END: i32 = MAX_RUNE + 1;

#[derive(Clone, Copy)]
struct TableIndex<'a> {
    t: &'a RangeTable,
    p: u32,
    next: i32,
}

fn sort_iter(t: &mut [TableIndex<'_>]) {
    for i in 0..t.len() {
        let mut j = i;
        while j > 0 && t[j - 1].next > t[j].next {
            t.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn next16(ti: &mut [TableIndex<'_>]) -> Range16 {
    sort_iter(ti);
    let t0 = ti[0];
    if t0.next == AT_END {
        return Range16::new(0, 0, 0);
    }
    let mut r0 = t0.t.r16[t0.p as usize];
    r0.lo = t0.next as u16;
    for tn in ti.iter() {
        if i32::from(r0.hi) <= tn.next {
            break;
        }
        let mut rn = tn.t.r16[tn.p as usize];
        rn.lo = tn.next as u16;
        let m = rn.lo.wrapping_sub(r0.lo) % r0.stride;
        if m == 0 && (rn.stride == r0.stride || rn.lo == rn.hi) {
            if r0.hi > rn.hi {
                r0.hi = rn.hi;
            }
        } else {
            let x = rn.lo.wrapping_sub(m);
            if r0.lo <= x {
                r0.hi = x;
            }
            break;
        }
    }
    for tn in ti.iter_mut() {
        if i32::from(r0.hi) < tn.next {
            break;
        }
        let rn = tn.t.r16[tn.p as usize];
        let stride = i32::from(rn.stride);
        tn.next += stride * (1 + ((i32::from(r0.hi) - tn.next) / stride));
        if i32::from(rn.hi) < tn.next {
            tn.p += 1;
            if tn.p as usize == tn.t.r16.len() {
                tn.next = AT_END;
            } else {
                tn.next = i32::from(tn.t.r16[tn.p as usize].lo);
            }
        }
    }
    if r0.lo == r0.hi {
        r0.stride = 1;
    }
    r0
}

fn next32(ti: &mut [TableIndex<'_>]) -> Range32 {
    sort_iter(ti);
    let t0 = ti[0];
    if t0.next == AT_END {
        return Range32::new(0, 0, 0);
    }
    let mut r0 = t0.t.r32[t0.p as usize];
    r0.lo = t0.next as u32;
    for tn in ti.iter() {
        if r0.hi as i32 <= tn.next {
            break;
        }
        let mut rn = tn.t.r32[tn.p as usize];
        rn.lo = tn.next as u32;
        let m = rn.lo.wrapping_sub(r0.lo) % r0.stride;
        if m == 0 && (rn.stride == r0.stride || rn.lo == rn.hi) {
            if r0.hi > rn.hi {
                r0.hi = rn.hi;
            }
        } else {
            let x = rn.lo.wrapping_sub(m);
            if r0.lo <= x {
                r0.hi = x;
            }
            break;
        }
    }
    for tn in ti.iter_mut() {
        if (r0.hi as i32) < tn.next {
            break;
        }
        let rn = tn.t.r32[tn.p as usize];
        let stride = rn.stride as i32;
        tn.next += stride * (1 + ((r0.hi as i32 - tn.next) / stride));
        if (rn.hi as i32) < tn.next {
            tn.p += 1;
            if tn.p as usize == tn.t.r32.len() {
                tn.next = AT_END;
            } else {
                tn.next = tn.t.r32[tn.p as usize].lo as i32;
            }
        }
    }
    if r0.lo == r0.hi {
        r0.stride = 1;
    }
    r0
}

pub fn merge(ranges: &[&RangeTable]) -> RangeTable {
    let mut r16: Vec<Range16> = Vec::new();
    let mut r32: Vec<Range32> = Vec::new();
    if !ranges.is_empty() {
        let mut iter: Vec<TableIndex<'_>> = ranges
            .iter()
            .map(|t| TableIndex {
                t,
                p: 0,
                next: t.r16.first().map_or(AT_END, |r| i32::from(r.lo)),
            })
            .collect();
        let mut r0 = next16(&mut iter);
        if r0.stride != 0 {
            loop {
                let mut r1 = next16(&mut iter);
                if r1.stride == 0 {
                    r16.push(r0);
                    break;
                }
                let stride = r1.lo.wrapping_sub(r0.hi);
                if (r1.lo == r1.hi || stride == r1.stride)
                    && (r0.lo == r0.hi || stride == r0.stride)
                {
                    r0.hi = r1.hi;
                    r0.stride = stride;
                    continue;
                } else if stride == r0.stride {
                    r0.hi = r1.lo;
                    r0.stride = stride;
                    r1.lo = r1.lo.wrapping_add(r1.stride);
                    if r1.lo > r1.hi {
                        continue;
                    }
                }
                r16.push(r0);
                r0 = r1;
            }
        }
        let mut iter: Vec<TableIndex<'_>> = ranges
            .iter()
            .map(|t| TableIndex {
                t,
                p: 0,
                next: t.r32.first().map_or(AT_END, |r| r.lo as i32),
            })
            .collect();
        let mut r0 = next32(&mut iter);
        if r0.stride != 0 {
            loop {
                let mut r1 = next32(&mut iter);
                if r1.stride == 0 {
                    r32.push(r0);
                    break;
                }
                let stride = r1.lo.wrapping_sub(r0.hi);
                if (r1.lo == r1.hi || stride == r1.stride)
                    && (r0.lo == r0.hi || stride == r0.stride)
                {
                    r0.hi = r1.hi;
                    r0.stride = stride;
                    continue;
                } else if stride == r0.stride {
                    r0.hi = r1.lo;
                    r1.lo = r1.lo.wrapping_add(r1.stride);
                    if r1.lo > r1.hi {
                        continue;
                    }
                }
                r32.push(r0);
                r0 = r1;
            }
        }
    }
    let mut latin_offset = 0;
    for (i, r) in r16.iter().enumerate() {
        if i32::from(r.hi) > MAX_LATIN1 {
            break;
        }
        latin_offset = i + 1;
    }
    RangeTable {
        r16: Box::leak(r16.into_boxed_slice()),
        r32: Box::leak(r32.into_boxed_slice()),
        latin_offset,
    }
}
