pub const CASTAGNOLI: u32 = 0x82f6_3b78;

pub const SIZE: usize = 4;

static TABLES: [[u32; 256]; 8] = make_tables(CASTAGNOLI);

const fn make_tables(poly: u32) -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ poly;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        t[0][i] = crc;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut crc = t[0][i];
        let mut j = 1;
        while j < 8 {
            crc = t[0][(crc & 0xff) as usize] ^ (crc >> 8);
            t[j][i] = crc;
            j += 1;
        }
        i += 1;
    }
    t
}

fn raw_update(mut crc: u32, mut p: &[u8]) -> u32 {
    let t = &TABLES;
    while p.len() >= 8 {
        let lo = u32::from_le_bytes([p[0], p[1], p[2], p[3]]) ^ crc;
        crc = t[0][p[7] as usize]
            ^ t[1][p[6] as usize]
            ^ t[2][p[5] as usize]
            ^ t[3][p[4] as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[7][(lo & 0xff) as usize];
        p = &p[8..];
    }
    for &b in p {
        crc = t[0][((crc as u8) ^ b) as usize] ^ (crc >> 8);
    }
    crc
}

pub fn update(crc: u32, p: &[u8]) -> u32 {
    !raw_update(!crc, p)
}

pub fn checksum(data: &[u8]) -> u32 {
    update(0, data)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Digest {
    crc: u32,
}

impl Digest {
    pub const fn new() -> Digest {
        Digest { crc: 0 }
    }

    pub fn reset(&mut self) {
        self.crc = 0;
    }

    pub fn write(&mut self, p: &[u8]) {
        self.crc = update(self.crc, p);
    }

    pub fn sum32(&self) -> u32 {
        self.crc
    }
}

pub fn new() -> Digest {
    Digest::new()
}
