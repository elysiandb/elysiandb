use gocompat::base64::{CorruptInputError, Encoding};

const ALPHABET: &[u8; 64] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

static BC_ENCODING: Encoding = Encoding::new(ALPHABET);

#[doc(hidden)]
pub fn base64_encode(src: &[u8]) -> Vec<u8> {
    let n = BC_ENCODING.encoded_len(src.len());
    let mut dst = vec![0u8; n];
    BC_ENCODING.encode(&mut dst, src);
    let mut n = n;
    while dst[n - 1] == b'=' {
        n -= 1;
    }
    dst.truncate(n);
    dst
}

#[doc(hidden)]
pub fn base64_decode(src: &[u8]) -> Result<Vec<u8>, CorruptInputError> {
    let num_of_equals = 4 - (src.len() % 4);
    let mut padded = Vec::with_capacity(src.len() + num_of_equals);
    padded.extend_from_slice(src);
    padded.extend(std::iter::repeat_n(b'=', num_of_equals));

    let mut dst = vec![0u8; BC_ENCODING.decoded_len(padded.len())];
    let (n, result) = BC_ENCODING.decode(&mut dst, &padded);
    result?;
    dst.truncate(n);
    Ok(dst)
}
