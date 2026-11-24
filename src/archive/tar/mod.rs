mod common;
mod format;
mod reader;
mod strconv;
mod writer;

use std::io;

pub use common::{
    Header, HeaderError, Records, TYPE_BLOCK, TYPE_CHAR, TYPE_CONT, TYPE_DIR, TYPE_FIFO,
    TYPE_GNU_LONG_LINK, TYPE_GNU_LONG_NAME, TYPE_GNU_SPARSE, TYPE_LINK, TYPE_REG, TYPE_REG_A,
    TYPE_SYMLINK, TYPE_X_GLOBAL_HEADER, TYPE_X_HEADER,
};
pub use format::{FORMAT_GNU, FORMAT_PAX, FORMAT_UNKNOWN, FORMAT_USTAR, Format};
pub use reader::Reader;
pub use writer::Writer;

pub(crate) const ERR_HEADER: &str = "invalid tar header";
pub(crate) const ERR_WRITE_TOO_LONG: &str = "write longer than the tar entry size";
pub(crate) const ERR_FIELD_TOO_LONG: &str = "tar header field too long";
pub(crate) const ERR_WRITE_AFTER_CLOSE: &str = "write after the tar stream was closed";
pub(crate) const ERR_MISS_DATA: &str = "the sparse tar entry references missing data";
pub(crate) const ERR_UNREF_DATA: &str = "the sparse tar entry holds unreferenced data";

pub(crate) fn tar_error(kind: io::ErrorKind, msg: &str) -> io::Error {
    io::Error::new(kind, msg.to_string())
}

#[doc(hidden)]
pub mod internals {
    use super::common::{self, Header, HeaderError, Records, SparseEntry};
    use super::format::Format;
    use super::strconv::{self, Formatter, Parser};
    use gocompat::time::Time;

    pub fn fits_in_base256(n: usize, x: i64) -> bool {
        strconv::fits_in_base256(n, x)
    }

    pub fn fits_in_octal(n: usize, x: i64) -> bool {
        strconv::fits_in_octal(n, x)
    }

    pub fn parse_numeric(b: &[u8]) -> (i64, bool) {
        let mut p = Parser::default();
        let x = p.parse_numeric(b);
        (x, !p.err)
    }

    pub fn format_numeric(b: &mut [u8], x: i64) -> bool {
        let mut f = Formatter::default();
        f.format_numeric(b, x);
        !f.err
    }

    pub fn parse_pax_time(s: &[u8]) -> Option<Time> {
        strconv::parse_pax_time(s).ok()
    }

    pub fn format_pax_time(t: &Time) -> Vec<u8> {
        strconv::format_pax_time(t)
    }

    pub fn parse_pax_record(s: &[u8]) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        strconv::parse_pax_record(s)
            .ok()
            .map(|(k, v, rest)| (k, v, rest.to_vec()))
    }

    pub fn format_pax_record(k: &[u8], v: &[u8]) -> Option<Vec<u8>> {
        strconv::format_pax_record(k, v).ok()
    }

    pub fn split_ustar_path(name: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
        super::writer::split_ustar_path(name).map(|(p, s)| (p.to_vec(), s.to_vec()))
    }

    pub fn allowed_formats(h: &Header) -> (Format, Records, Option<HeaderError>) {
        h.allowed_formats()
    }

    pub fn validate_sparse_entries(sp: &[(i64, i64)], size: i64) -> bool {
        let sp: Vec<SparseEntry> = sp
            .iter()
            .map(|&(offset, length)| SparseEntry { offset, length })
            .collect();
        common::validate_sparse_entries(&sp, size)
    }

    pub fn invert_sparse_entries(sp: &[(i64, i64)], size: i64) -> Vec<(i64, i64)> {
        let sp: Vec<SparseEntry> = sp
            .iter()
            .map(|&(offset, length)| SparseEntry { offset, length })
            .collect();
        common::invert_sparse_entries(&sp, size)
            .into_iter()
            .map(|e| (e.offset, e.length))
            .collect()
    }

    pub fn merge_pax(hdr: &mut Header, records: Records) -> bool {
        super::reader::merge_pax(hdr, Some(records)).is_ok()
    }

    pub fn parse_pax(data: &[u8]) -> Option<Records> {
        super::reader::parse_pax(data).ok()
    }
}
