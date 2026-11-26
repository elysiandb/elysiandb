mod deflate_test;
mod dict_decoder_test;
mod flate_test;
mod huffman_bit_writer_test;
mod huffman_tokens;
mod inflate_test;
mod reader_test;
mod streams;
mod writer_test;

use elysiandb::archive::flate::{self, BEST_SPEED};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;

pub fn testdata(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/archive/flate_test/testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

pub fn inflate_all(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut r = flate::new_reader(data);
    let mut out = Vec::new();
    r.read_to_end(&mut out)?;
    Ok(out)
}

pub fn deflate_once(data: &[u8]) -> Vec<u8> {
    let mut w = flate::Writer::new(Vec::new(), BEST_SPEED).unwrap();
    w.write_all(data).unwrap();
    w.close().unwrap();
    w.get_ref().clone()
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
