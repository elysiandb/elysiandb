mod deflate;
mod deflatefast;
mod dict_decoder;
mod huffman_bit_writer;
mod huffman_code;
mod inflate;
mod token;

pub use deflate::{BEST_SPEED, Writer};
pub use inflate::{CorruptInputError, Reader, new_reader};

pub(crate) use huffman_bit_writer::write_once;
pub(crate) use inflate::Decompressor;

#[doc(hidden)]
pub use deflatefast::{BUFFER_RESET, DeflateFast, INPUT_MARGIN};
#[doc(hidden)]
pub use dict_decoder::DictDecoder;
#[doc(hidden)]
pub use huffman_bit_writer::HuffmanBitWriter;
#[doc(hidden)]
pub use huffman_code::reverse_bits;
#[doc(hidden)]
pub use inflate::{HuffmanDecoder, decode_symbol};
#[doc(hidden)]
pub use token::Token;
#[doc(hidden)]
pub const MAX_MATCH_LENGTH: usize = deflate::MAX_MATCH_LENGTH;
#[doc(hidden)]
pub const MAX_MATCH_OFFSET: usize = deflate::MAX_MATCH_OFFSET;
#[doc(hidden)]
pub const MAX_STORE_BLOCK_SIZE: usize = deflate::MAX_STORE_BLOCK_SIZE;
#[doc(hidden)]
pub const WINDOW_SIZE: usize = 1 << 15;
