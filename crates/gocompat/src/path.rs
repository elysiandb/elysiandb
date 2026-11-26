struct LazyBuf {
    buf: Vec<u8>,
    w: usize,
}

impl LazyBuf {
    fn append(&mut self, c: u8) {
        if self.w < self.buf.len() {
            self.buf[self.w] = c;
        } else {
            self.buf.push(c);
        }
        self.w += 1;
    }

    fn index(&self, i: usize) -> u8 {
        self.buf[i]
    }
}

pub fn clean(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }

    let rooted = path[0] == b'/';
    let n = path.len();
    let mut out = LazyBuf {
        buf: Vec::with_capacity(n),
        w: 0,
    };
    let mut r = 0;
    let mut dotdot = 0;
    if rooted {
        out.append(b'/');
        r = 1;
        dotdot = 1;
    }

    while r < n {
        if path[r] == b'/' || (path[r] == b'.' && (r + 1 == n || path[r + 1] == b'/')) {
            r += 1;
        } else if path[r] == b'.' && path[r + 1] == b'.' && (r + 2 == n || path[r + 2] == b'/') {
            r += 2;
            if out.w > dotdot {
                out.w -= 1;
                while out.w > dotdot && out.index(out.w) != b'/' {
                    out.w -= 1;
                }
            } else if !rooted {
                if out.w > 0 {
                    out.append(b'/');
                }
                out.append(b'.');
                out.append(b'.');
                dotdot = out.w;
            }
        } else {
            if (rooted && out.w != 1) || (!rooted && out.w != 0) {
                out.append(b'/');
            }
            while r < n && path[r] != b'/' {
                out.append(path[r]);
                r += 1;
            }
        }
    }

    if out.w == 0 {
        return b".".to_vec();
    }

    out.buf.truncate(out.w);
    out.buf
}

pub fn ext(path: &[u8]) -> &[u8] {
    let mut i = path.len();
    while i > 0 && path[i - 1] != b'/' {
        i -= 1;
        if path[i] == b'.' {
            return &path[i..];
        }
    }
    &[]
}
