mod allowed_vectors;
mod reader_test;
mod reader_vectors;
mod strconv_test;
mod tar_test;
mod writer_test;
mod writer_vectors;

pub use elysiandb::archive::tar::{self, Format, Header, Records};
pub use gocompat::time::Time;
use std::path::PathBuf;

pub const HEADER: &str = "invalid tar header";
pub const UNEXPECTED: &str = "unexpected end of data";
pub const FIELD_TOO_LONG: &str = "tar header field too long";
pub const WRITE_TOO_LONG: &str = "write longer than the tar entry size";
pub const WRITE_AFTER_CLOSE: &str = "write after the tar stream was closed";
pub const HEADER_ERROR: &str = "cannot encode the tar header";

pub struct ReaderVector {
    pub file: &'static str,
    pub headers: Vec<Header>,
    pub chksums: Option<Vec<&'static str>>,
    pub err: Option<&'static str>,
}

pub enum Op {
    Header(Box<Header>, Option<&'static str>),
    Write(Vec<u8>, usize, Option<&'static str>),
    Close(Option<&'static str>),
}

pub struct WriterVector {
    pub file: &'static str,
    pub ops: Vec<Op>,
}

pub struct AllowedVector {
    pub header: Header,
    pub pax_hdrs: Records,
    pub formats: Format,
}

pub fn unix(sec: i64, nsec: i64) -> Time {
    Time::from_unix(sec, nsec)
}

pub fn rec(k: &[u8], v: &[u8]) -> (Vec<u8>, Vec<u8>) {
    (k.to_vec(), v.to_vec())
}

pub fn recs(pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Records {
    pairs.into_iter().collect()
}

pub fn testdata(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/archive/tar_test/testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn same_time(a: &Time, b: &Time) -> bool {
    a.is_zero() == b.is_zero() && a.unix() == b.unix() && a.nanosecond() == b.nanosecond()
}

pub fn header_eq(got: &Header, want: &Header) -> bool {
    got.typeflag == want.typeflag
        && got.name == want.name
        && got.linkname == want.linkname
        && got.size == want.size
        && got.mode == want.mode
        && got.uid == want.uid
        && got.gid == want.gid
        && got.uname == want.uname
        && got.gname == want.gname
        && same_time(&got.mod_time, &want.mod_time)
        && same_time(&got.access_time, &want.access_time)
        && same_time(&got.change_time, &want.change_time)
        && got.devmajor == want.devmajor
        && got.devminor == want.devminor
        && got.xattrs == want.xattrs
        && got.pax_records == want.pax_records
        && got.format == want.format
}

pub fn error_matches(got: Option<&std::io::Error>, want: Option<&str>) -> bool {
    match (got, want) {
        (None, None) => true,
        (Some(e), Some(HEADER_ERROR)) => e.to_string().starts_with(HEADER_ERROR),
        (Some(e), Some(w)) => e.to_string() == w,
        _ => false,
    }
}
