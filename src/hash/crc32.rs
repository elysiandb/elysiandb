use super::crc32c::{make_tables, raw_update};

pub const IEEE: u32 = 0xedb8_8320;

pub const SIZE: usize = 4;

static IEEE_TABLES: [[u32; 256]; 8] = make_tables(IEEE);

pub fn update(crc: u32, p: &[u8]) -> u32 {
    !raw_update(&IEEE_TABLES, !crc, p)
}

pub fn checksum_ieee(data: &[u8]) -> u32 {
    update(0, data)
}
