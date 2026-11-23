pub(super) const DECIMAL_DIGITS: usize = 800;

pub(super) struct Decimal {
    pub(super) d: [u8; DECIMAL_DIGITS],
    pub(super) nd: isize,
    pub(super) dp: isize,
    pub(super) neg: bool,
    pub(super) trunc: bool,
}

fn trim(a: &mut Decimal) {
    while a.nd > 0 && a.d[(a.nd - 1) as usize] == b'0' {
        a.nd -= 1;
    }
    if a.nd == 0 {
        a.dp = 0;
    }
}

const UINT_SIZE: u32 = 64;
const MAX_SHIFT: u32 = UINT_SIZE - 4;

fn right_shift(a: &mut Decimal, k: u32) {
    let mut r: isize = 0;
    let mut w: usize = 0;

    let mut n: u64 = 0;
    while n >> k == 0 {
        if r >= a.nd {
            if n == 0 {
                a.nd = 0;
                return;
            }
            while n >> k == 0 {
                n *= 10;
                r += 1;
            }
            break;
        }
        let c = u64::from(a.d[r as usize]);
        n = n * 10 + c - u64::from(b'0');
        r += 1;
    }
    a.dp -= r - 1;

    let mask: u64 = (1u64 << k) - 1;

    while r < a.nd {
        let c = u64::from(a.d[r as usize]);
        let dig = n >> k;
        n &= mask;
        a.d[w] = dig as u8 + b'0';
        w += 1;
        n = n * 10 + c - u64::from(b'0');
        r += 1;
    }

    while n > 0 {
        let dig = n >> k;
        n &= mask;
        if w < DECIMAL_DIGITS {
            a.d[w] = dig as u8 + b'0';
            w += 1;
        } else if dig > 0 {
            a.trunc = true;
        }
        n *= 10;
    }

    a.nd = w as isize;
    trim(a);
}

static LEFTCHEATS: [(isize, &[u8]); 61] = [
    (0, b""),
    (1, b"5"),
    (1, b"25"),
    (1, b"125"),
    (2, b"625"),
    (2, b"3125"),
    (2, b"15625"),
    (3, b"78125"),
    (3, b"390625"),
    (3, b"1953125"),
    (4, b"9765625"),
    (4, b"48828125"),
    (4, b"244140625"),
    (4, b"1220703125"),
    (5, b"6103515625"),
    (5, b"30517578125"),
    (5, b"152587890625"),
    (6, b"762939453125"),
    (6, b"3814697265625"),
    (6, b"19073486328125"),
    (7, b"95367431640625"),
    (7, b"476837158203125"),
    (7, b"2384185791015625"),
    (7, b"11920928955078125"),
    (8, b"59604644775390625"),
    (8, b"298023223876953125"),
    (8, b"1490116119384765625"),
    (9, b"7450580596923828125"),
    (9, b"37252902984619140625"),
    (9, b"186264514923095703125"),
    (10, b"931322574615478515625"),
    (10, b"4656612873077392578125"),
    (10, b"23283064365386962890625"),
    (10, b"116415321826934814453125"),
    (11, b"582076609134674072265625"),
    (11, b"2910383045673370361328125"),
    (11, b"14551915228366851806640625"),
    (12, b"72759576141834259033203125"),
    (12, b"363797880709171295166015625"),
    (12, b"1818989403545856475830078125"),
    (13, b"9094947017729282379150390625"),
    (13, b"45474735088646411895751953125"),
    (13, b"227373675443232059478759765625"),
    (13, b"1136868377216160297393798828125"),
    (14, b"5684341886080801486968994140625"),
    (14, b"28421709430404007434844970703125"),
    (14, b"142108547152020037174224853515625"),
    (15, b"710542735760100185871124267578125"),
    (15, b"3552713678800500929355621337890625"),
    (15, b"17763568394002504646778106689453125"),
    (16, b"88817841970012523233890533447265625"),
    (16, b"444089209850062616169452667236328125"),
    (16, b"2220446049250313080847263336181640625"),
    (16, b"11102230246251565404236316680908203125"),
    (17, b"55511151231257827021181583404541015625"),
    (17, b"277555756156289135105907917022705078125"),
    (17, b"1387778780781445675529539585113525390625"),
    (18, b"6938893903907228377647697925567626953125"),
    (18, b"34694469519536141888238489627838134765625"),
    (18, b"173472347597680709441192448139190673828125"),
    (19, b"867361737988403547205962240695953369140625"),
];

fn prefix_is_less_than(b: &[u8], s: &[u8]) -> bool {
    for (i, &c) in s.iter().enumerate() {
        if i >= b.len() {
            return true;
        }
        if b[i] != c {
            return b[i] < c;
        }
    }
    false
}

fn left_shift(a: &mut Decimal, k: u32) {
    let (mut delta, cutoff) = LEFTCHEATS[k as usize];
    if prefix_is_less_than(&a.d[0..a.nd as usize], cutoff) {
        delta -= 1;
    }

    let mut r = a.nd;
    let mut w = a.nd + delta;

    let mut n: u64 = 0;
    r -= 1;
    while r >= 0 {
        n += u64::from(a.d[r as usize] - b'0') << k;
        let quo = n / 10;
        let rem = n - 10 * quo;
        w -= 1;
        if w < DECIMAL_DIGITS as isize {
            a.d[w as usize] = rem as u8 + b'0';
        } else if rem != 0 {
            a.trunc = true;
        }
        n = quo;
        r -= 1;
    }

    while n > 0 {
        let quo = n / 10;
        let rem = n - 10 * quo;
        w -= 1;
        if w < DECIMAL_DIGITS as isize {
            a.d[w as usize] = rem as u8 + b'0';
        } else if rem != 0 {
            a.trunc = true;
        }
        n = quo;
    }

    a.nd += delta;
    if a.nd >= DECIMAL_DIGITS as isize {
        a.nd = DECIMAL_DIGITS as isize;
    }
    a.dp += delta;
    trim(a);
}

fn should_round_up(a: &Decimal, nd: isize) -> bool {
    if nd < 0 || nd >= a.nd {
        return false;
    }
    let i = nd as usize;
    if a.d[i] == b'5' && nd + 1 == a.nd {
        if a.trunc {
            return true;
        }
        return nd > 0 && !(a.d[i - 1] - b'0').is_multiple_of(2);
    }
    a.d[i] >= b'5'
}

impl Decimal {
    pub(super) fn new() -> Decimal {
        Decimal {
            d: [0; DECIMAL_DIGITS],
            nd: 0,
            dp: 0,
            neg: false,
            trunc: false,
        }
    }

    pub(super) fn assign(&mut self, v: u64) {
        let mut buf = [0u8; 24];

        let mut n = 0usize;
        let mut v = v;
        while v > 0 {
            let v1 = v / 10;
            v -= 10 * v1;
            buf[n] = v as u8 + b'0';
            n += 1;
            v = v1;
        }

        self.nd = 0;
        while n > 0 {
            n -= 1;
            self.d[self.nd as usize] = buf[n];
            self.nd += 1;
        }
        self.dp = self.nd;
        trim(self);
    }

    pub(super) fn shift(&mut self, k: isize) {
        if self.nd == 0 {
            return;
        }
        let mut k = k;
        if k > 0 {
            while k > MAX_SHIFT as isize {
                left_shift(self, MAX_SHIFT);
                k -= MAX_SHIFT as isize;
            }
            left_shift(self, k as u32);
        } else if k < 0 {
            while k < -(MAX_SHIFT as isize) {
                right_shift(self, MAX_SHIFT);
                k += MAX_SHIFT as isize;
            }
            right_shift(self, (-k) as u32);
        }
    }

    pub(super) fn round(&mut self, nd: isize) {
        if nd < 0 || nd >= self.nd {
            return;
        }
        if should_round_up(self, nd) {
            self.round_up(nd);
        } else {
            self.round_down(nd);
        }
    }

    pub(super) fn round_down(&mut self, nd: isize) {
        if nd < 0 || nd >= self.nd {
            return;
        }
        self.nd = nd;
        trim(self);
    }

    pub(super) fn round_up(&mut self, nd: isize) {
        if nd < 0 || nd >= self.nd {
            return;
        }

        let mut i = nd - 1;
        while i >= 0 {
            let c = self.d[i as usize];
            if c < b'9' {
                self.d[i as usize] += 1;
                self.nd = i + 1;
                return;
            }
            i -= 1;
        }

        self.d[0] = b'1';
        self.nd = 1;
        self.dp += 1;
    }

    pub(super) fn rounded_integer(&self) -> u64 {
        if self.dp > 20 {
            return 0xFFFFFFFFFFFFFFFF;
        }
        let mut i: isize = 0;
        let mut n: u64 = 0;
        while i < self.dp && i < self.nd {
            n = n
                .wrapping_mul(10)
                .wrapping_add(u64::from(self.d[i as usize] - b'0'));
            i += 1;
        }
        while i < self.dp {
            n = n.wrapping_mul(10);
            i += 1;
        }
        if should_round_up(self, self.dp) {
            n = n.wrapping_add(1);
        }
        n
    }
}
