use crate::gunzip_vectors::{GUNZIP_TESTS, UNEXPECTED};
use crate::{read_all, testdata};
use elysiandb::archive::gzip::{self, Reader};
use gocompat::base64::STD_ENCODING;
use std::io::{self, Read};
use std::time::{Duration, Instant};

fn err_text(r: &io::Result<()>) -> Option<String> {
    r.as_ref().err().map(|e| e.to_string())
}

#[test]
fn decompressor() {
    assert_eq!(GUNZIP_TESTS.len(), 15);
    let mut r1: Option<Reader<&[u8]>> = None;
    for tt in GUNZIP_TESTS {
        let mut r2 = match gzip::Reader::new(tt.gzip) {
            Ok(r) => r,
            Err(err) => panic!("{}: new: {err}", tt.desc),
        };
        assert_eq!(tt.name, r2.header.name, "{}: got name", tt.desc);
        let (b, res) = read_all(&mut r2);
        assert_eq!(err_text(&res).as_deref(), tt.err, "{}: copy error", tt.desc);
        assert_eq!(b, tt.raw, "{}", tt.desc);
        let _ = r2.close();

        match r1.as_mut() {
            None => r1 = Some(gzip::Reader::new(tt.gzip).unwrap()),
            Some(r) => r
                .reset(tt.gzip)
                .unwrap_or_else(|e| panic!("{}: reset: {e}", tt.desc)),
        }
        let r = r1.as_mut().unwrap();
        assert_eq!(tt.name, r.header.name, "{}: got name", tt.desc);
        let (b, res) = read_all(r);
        assert_eq!(err_text(&res).as_deref(), tt.err, "{}: copy error", tt.desc);
        assert_eq!(b, tt.raw, "{}", tt.desc);
    }
}

#[test]
fn header_with_all_fields_used() {
    let tt = GUNZIP_TESTS
        .iter()
        .find(|t| t.desc == "header with all fields used")
        .unwrap();
    let r = gzip::Reader::new(tt.gzip).unwrap();
    assert_eq!(r.header.name, "f1l3n4m3.tXt");
    let comment: String = (1..=255u8).map(char::from).collect();
    assert_eq!(r.header.comment, comment);
    assert_eq!(r.header.extra.as_deref(), Some(&b"zz\x05\x00abcde"[..]));
    assert_eq!(r.header.os, 0xaa);
    assert_eq!(r.header.mod_time.unix(), 0x4af9_f070);
}

#[test]
fn issue_6550() {
    let encoded: Vec<u8> = testdata("issue6550.gz.base64")
        .into_iter()
        .filter(|&b| b != b'\n' && b != b'\r')
        .collect();
    let data = STD_ENCODING.decode_string(&encoded).unwrap();
    let mut gz = gzip::new_reader(data.as_slice()).expect("new reader");
    let start = Instant::now();
    let res = io::copy(&mut gz, &mut io::sink());
    assert!(res.is_err(), "copy succeeded");
    assert!(start.elapsed() < Duration::from_secs(1), "copy hung");
}

#[test]
fn multistream_false() {
    let tt = GUNZIP_TESTS
        .iter()
        .find(|t| t.desc.ends_with(" x2"))
        .expect("cannot find hello.txt x2 in gunzip tests");

    let br: &[u8] = tt.gzip;
    let mut r = gzip::Reader::new(br).unwrap_or_else(|e| panic!("first reset: {e}"));

    let hello = b"hello world\n";

    r.multistream(false);
    let (data, res) = read_all(&mut r);
    assert!(
        data == hello && res.is_ok(),
        "first stream = {data:?}, {res:?}"
    );

    let br = r.into_inner();
    let mut r = gzip::Reader::new(br).unwrap_or_else(|e| panic!("second reset: {e}"));
    r.multistream(false);
    let (data, res) = read_all(&mut r);
    assert!(
        data == hello && res.is_ok(),
        "second stream = {data:?}, {res:?}"
    );

    let br = r.into_inner();
    let err = gzip::Reader::new(br).err().expect("third reset");
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(err.to_string(), UNEXPECTED);
}

#[test]
fn nil_stream() {
    let err = gzip::Reader::new(&[][..]).err().expect("empty stream");
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(err.to_string(), UNEXPECTED);
}

#[test]
fn truncated_streams() {
    let cases: [(&str, &[u8]); 3] = [
        (
            "original",
            b"\x1f\x8b\x08\x04\x00\tn\x88\x00\xff\x07\x00foo bar\xcbH\xcd\xc9\xc9\xd7Q(\xcf/\xcaI\x01\x04:r\xab\xff\x0c\x00\x00\x00",
        ),
        (
            "truncated name",
            &[
                0x1f, 0x8b, 0x08, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x01,
            ],
        ),
        (
            "truncated comment",
            &[
                0x1f, 0x8b, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x01,
            ],
        ),
    ];

    for (name, data) in cases {
        for i in 1..data.len() {
            let mut r = match gzip::Reader::new(&data[..i]) {
                Ok(r) => r,
                Err(err) => {
                    assert_eq!(
                        err.to_string(),
                        UNEXPECTED,
                        "new({name}-{i}) on truncated stream"
                    );
                    continue;
                }
            };
            let err = io::copy(&mut r, &mut io::sink()).expect_err("truncated stream");
            assert_eq!(
                err.to_string(),
                UNEXPECTED,
                "copy({name}-{i}) on truncated stream"
            );
        }
    }
}

#[test]
fn cve_2022_30631() {
    let empty: [u8; 20] = [
        0x1f, 0x8b, 0x08, 0x00, 0xa7, 0x8f, 0x43, 0x62, 0x00, 0x03, 0x03, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let r = empty.repeat(4_000_000);
    let mut z = gzip::Reader::new(r.as_slice()).expect("new reader");
    let mut buf = [0u8; 10];
    assert_eq!(z.read(&mut buf).unwrap(), 0);
}

#[test]
fn reserved_flags_are_ignored_like_go() {
    let mut data = vec![0x1f, 0x8b, 0x08, 0xe0, 0, 0, 0, 0, 0, 0xff];
    data.extend_from_slice(&[0x03, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut z = gzip::Reader::new(data.as_slice()).unwrap();
    let (out, res) = read_all(&mut z);
    assert!(out.is_empty());
    assert!(res.is_ok());
}

#[test]
fn header_crc_is_checked() {
    let mut data = vec![0x1f, 0x8b, 0x08, 0x02, 0, 0, 0, 0, 0, 0xff];
    let digest = elysiandb::hash::crc32::checksum_ieee(&data) as u16;
    data.extend_from_slice(&digest.to_le_bytes());
    data.extend_from_slice(&[0x03, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(gzip::Reader::new(data.as_slice()).is_ok());
    data[10] ^= 1;
    let err = gzip::Reader::new(data.as_slice()).err().unwrap();
    assert_eq!(err.to_string(), "invalid gzip header");
}

#[test]
fn latin1_name_is_read_as_unicode() {
    let mut data = vec![0x1f, 0x8b, 0x08, 0x08, 0, 0, 0, 0, 0, 0xff];
    data.extend_from_slice(&[0xc4, b'u', 0xdf, b'e', b'r', b'u', b'n', b'g', 0]);
    data.extend_from_slice(&[0x03, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    let r = gzip::Reader::new(data.as_slice()).unwrap();
    assert_eq!(r.header.name, "Äußerung");
}

#[test]
fn header_strings_are_bounded() {
    let mut data = vec![0x1f, 0x8b, 0x08, 0x08, 0, 0, 0, 0, 0, 0xff];
    data.extend(std::iter::repeat_n(b'a', 512));
    data.push(0);
    let err = gzip::Reader::new(data.as_slice()).err().unwrap();
    assert_eq!(err.to_string(), "invalid gzip header");
}
