const HEX: &[u8; 16] = b"0123456789abcdef";

pub fn random_bytes() -> [u8; 16] {
    let mut b = [0u8; 16];
    if let Err(e) = crate::crypto::rand::os_random(&mut b) {
        panic!("cannot read random bytes: {e}");
    }

    b
}

pub fn format(b: &[u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (i, &c) in b.iter().enumerate() {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            out.push('-');
        }
        out.push(HEX[(c >> 4) as usize] as char);
        out.push(HEX[(c & 0x0f) as usize] as char);
    }

    out
}

pub fn new_v4() -> String {
    let mut b = random_bytes();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;

    format(&b)
}
