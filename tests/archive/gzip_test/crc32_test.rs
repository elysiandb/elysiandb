use elysiandb::hash::crc32;

#[test]
fn ieee_check_values() {
    assert_eq!(crc32::checksum_ieee(b""), 0);
    assert_eq!(crc32::checksum_ieee(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32::checksum_ieee(b"x"), 0x8cdc_1683);
    let data: Vec<u8> = (0..10_000u32).map(|i| (i * 7 + 3) as u8).collect();
    let whole = crc32::checksum_ieee(&data);
    let mut crc = 0;
    for chunk in data.chunks(13) {
        crc = crc32::update(crc, chunk);
    }
    assert_eq!(crc, whole);
}
