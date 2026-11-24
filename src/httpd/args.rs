use super::bytesconv::{append_quoted_arg, decode_arg_append, parse_uint};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ArgsKV {
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
    pub(crate) no_value: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    args: Vec<ArgsKV>,
}

impl Args {
    pub fn new() -> Args {
        Args::default()
    }

    pub fn reset(&mut self) {
        self.args.clear();
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    pub fn all(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.args
            .iter()
            .map(|kv| (kv.key.as_slice(), kv.value.as_slice()))
    }

    pub fn parse(&mut self, s: impl AsRef<[u8]>) {
        self.reset();
        let mut b = s.as_ref();
        while let Some(kv) = next_arg(&mut b) {
            if !kv.key.is_empty() || !kv.value.is_empty() {
                self.args.push(kv);
            }
        }
    }

    pub fn query_string(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        self.append_bytes(&mut dst);
        dst
    }

    pub fn append_bytes(&self, dst: &mut Vec<u8>) {
        for (i, kv) in self.args.iter().enumerate() {
            append_quoted_arg(dst, &kv.key);
            if !kv.no_value {
                dst.push(b'=');
                if !kv.value.is_empty() {
                    append_quoted_arg(dst, &kv.value);
                }
            }
            if i + 1 < self.args.len() {
                dst.push(b'&');
            }
        }
    }

    pub fn del(&mut self, key: impl AsRef<[u8]>) {
        del_all_args(&mut self.args, key.as_ref());
    }

    pub fn add(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        append_arg(&mut self.args, key.as_ref(), value.as_ref(), false);
    }

    pub fn add_no_value(&mut self, key: impl AsRef<[u8]>) {
        append_arg(&mut self.args, key.as_ref(), b"", true);
    }

    pub fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        set_arg(&mut self.args, key.as_ref(), value.as_ref(), false);
    }

    pub fn set_no_value(&mut self, key: impl AsRef<[u8]>) {
        set_arg(&mut self.args, key.as_ref(), b"", true);
    }

    pub fn peek(&self, key: impl AsRef<[u8]>) -> &[u8] {
        peek_arg(&self.args, key.as_ref()).unwrap_or(&[])
    }

    pub fn peek_opt(&self, key: impl AsRef<[u8]>) -> Option<&[u8]> {
        peek_arg(&self.args, key.as_ref())
    }

    pub fn peek_multi(&self, key: impl AsRef<[u8]>) -> Vec<&[u8]> {
        let key = key.as_ref();
        self.args
            .iter()
            .filter(|kv| kv.key == key)
            .map(|kv| kv.value.as_slice())
            .collect()
    }

    pub fn has(&self, key: impl AsRef<[u8]>) -> bool {
        let key = key.as_ref();
        self.args.iter().any(|kv| kv.key == key)
    }

    pub fn get_uint(&self, key: impl AsRef<[u8]>) -> Option<i64> {
        let value = self.peek(key);
        if value.is_empty() {
            return None;
        }
        parse_uint(value)
    }

    pub fn set_uint(&mut self, key: impl AsRef<[u8]>, value: i64) {
        let mut buf = Vec::new();
        super::bytesconv::append_uint(&mut buf, value);
        self.set(key, buf);
    }

    pub fn get_uint_or_zero(&self, key: impl AsRef<[u8]>) -> i64 {
        self.get_uint(key).unwrap_or(0)
    }

    pub fn get_bool(&self, key: impl AsRef<[u8]>) -> bool {
        matches!(
            self.peek(key),
            b"1" | b"t"
                | b"T"
                | b"true"
                | b"TRUE"
                | b"True"
                | b"y"
                | b"yes"
                | b"Y"
                | b"YES"
                | b"Yes"
        )
    }
}

fn next_arg(b: &mut &[u8]) -> Option<ArgsKV> {
    if b.is_empty() {
        return None;
    }
    let s = *b;
    let mut kv = ArgsKV::default();
    let mut is_key = true;
    let mut k = 0;
    for (i, &c) in s.iter().enumerate() {
        match c {
            b'=' => {
                if is_key {
                    is_key = false;
                    decode_arg_append(&mut kv.key, &s[..i]);
                    k = i + 1;
                }
            }
            b'&' => {
                if is_key {
                    decode_arg_append(&mut kv.key, &s[..i]);
                    kv.value.clear();
                    kv.no_value = true;
                } else {
                    decode_arg_append(&mut kv.value, &s[k..i]);
                }
                *b = &s[i + 1..];
                return Some(kv);
            }
            _ => {}
        }
    }
    if is_key {
        decode_arg_append(&mut kv.key, s);
        kv.value.clear();
        kv.no_value = true;
    } else {
        decode_arg_append(&mut kv.value, &s[k..]);
    }
    *b = &s[s.len()..];
    Some(kv)
}

pub(crate) fn del_all_args(args: &mut Vec<ArgsKV>, key: &[u8]) {
    let mut n = args.len();
    let mut i = 0;
    while i < n {
        if args[i].key == key {
            args.swap(i, n - 1);
            n -= 1;
        } else {
            i += 1;
        }
    }
    args.truncate(n);
}

pub(crate) fn set_arg(h: &mut Vec<ArgsKV>, key: &[u8], value: &[u8], no_value: bool) {
    for kv in h.iter_mut() {
        if kv.key == key {
            kv.value.clear();
            if !no_value {
                kv.value.extend_from_slice(value);
            }
            kv.no_value = no_value;
            return;
        }
    }
    append_arg(h, key, value, no_value);
}

pub(crate) fn append_arg(h: &mut Vec<ArgsKV>, key: &[u8], value: &[u8], no_value: bool) {
    h.push(ArgsKV {
        key: key.to_vec(),
        value: if no_value { Vec::new() } else { value.to_vec() },
        no_value,
    });
}

pub(crate) fn peek_arg<'a>(h: &'a [ArgsKV], key: &[u8]) -> Option<&'a [u8]> {
    h.iter()
        .find(|kv| kv.key == key)
        .map(|kv| kv.value.as_slice())
}

pub(crate) fn peek_all_args<'a>(h: &'a [ArgsKV], key: &[u8]) -> Vec<&'a [u8]> {
    h.iter()
        .filter(|kv| kv.key == key)
        .map(|kv| kv.value.as_slice())
        .collect()
}
