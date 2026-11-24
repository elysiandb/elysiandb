mod crc32_test;
mod gunzip_test;
mod gunzip_vectors;
mod gzip_test;

use elysiandb::archive::gzip::{self, BEST_SPEED};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;

pub fn testdata(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/archive/gzip_test/testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

pub fn new_writer<W: Write>(w: W) -> gzip::Writer<W> {
    gzip::Writer::new_level(w, BEST_SPEED).unwrap()
}

pub fn read_all<R: Read>(r: &mut R) -> (Vec<u8>, io::Result<()>) {
    let mut out = Vec::new();
    let res = r.read_to_end(&mut out).map(|_| ());
    (out, res)
}

#[derive(Clone, Default)]
pub struct Shared(pub Rc<RefCell<Vec<u8>>>);

impl Shared {
    pub fn bytes(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }

    pub fn size(&self) -> usize {
        self.0.borrow().len()
    }
}

impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
