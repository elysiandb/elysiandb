struct LazyBuf<'a> {
    s: &'a [u8],
    buf: Option<Vec<u8>>,
    w: usize,
}

impl LazyBuf<'_> {
    fn index(&self, i: usize) -> u8 {
        match &self.buf {
            Some(b) => b[i],
            None => self.s[i],
        }
    }

    fn append(&mut self, c: u8) {
        if self.buf.is_none() {
            if self.w < self.s.len() && self.s[self.w] == c {
                self.w += 1;
                return;
            }
            let mut b = vec![0u8; self.s.len()];
            b[..self.w].copy_from_slice(&self.s[..self.w]);
            self.buf = Some(b);
        }
        let w = self.w;
        if let Some(b) = &mut self.buf {
            b[w] = c;
        }
        self.w += 1;
    }

    fn string(&self) -> Vec<u8> {
        match &self.buf {
            Some(b) => b[..self.w].to_vec(),
            None => self.s[..self.w].to_vec(),
        }
    }
}

pub fn clean(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }
    let rooted = path[0] == b'/';
    let n = path.len();
    let mut out = LazyBuf {
        s: path,
        buf: None,
        w: 0,
    };
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.append(b'/');
        r = 1;
        dotdot = 1;
    }
    while r < n {
        if path[r] == b'/' || path[r] == b'.' && (r + 1 == n || path[r + 1] == b'/') {
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
            if rooted && out.w != 1 || !rooted && out.w != 0 {
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
    out.string()
}

pub fn split(path: &[u8]) -> (&[u8], &[u8]) {
    match path.iter().rposition(|&c| c == b'/') {
        Some(i) => (&path[..i + 1], &path[i + 1..]),
        None => (&path[..0], path),
    }
}

pub fn join(elem: &[&[u8]]) -> Vec<u8> {
    let size: usize = elem.iter().map(|e| e.len()).sum();
    if size == 0 {
        return Vec::new();
    }
    let mut buf: Vec<u8> = Vec::with_capacity(size + elem.len() - 1);
    for e in elem {
        if !buf.is_empty() || !e.is_empty() {
            if !buf.is_empty() {
                buf.push(b'/');
            }
            buf.extend_from_slice(e);
        }
    }
    clean(&buf)
}

pub fn is_abs(path: &[u8]) -> bool {
    !path.is_empty() && path[0] == b'/'
}

pub fn dir(path: &[u8]) -> Vec<u8> {
    let (dir, _) = split(path);
    clean(dir)
}
