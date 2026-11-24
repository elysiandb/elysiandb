use super::block::{encrypt_block, expand_key, expand_key_with_salt};
use super::consts::{P, S0, S1, S2, S3};
use std::fmt;

pub const BLOCK_SIZE: usize = 8;

#[derive(Clone)]
pub struct Cipher {
    pub(super) p: [u32; 18],
    pub(super) s0: [u32; 256],
    pub(super) s1: [u32; 256],
    pub(super) s2: [u32; 256],
    pub(super) s3: [u32; 256],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySizeError(pub usize);

impl fmt::Display for KeySizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid blowfish key size {}", self.0)
    }
}

impl std::error::Error for KeySizeError {}

pub fn new_cipher(key: &[u8]) -> Result<Cipher, KeySizeError> {
    let k = key.len();
    if !(1..=56).contains(&k) {
        return Err(KeySizeError(k));
    }
    let mut result = init_cipher();
    expand_key(key, &mut result);
    Ok(result)
}

pub fn new_salted_cipher(key: &[u8], salt: &[u8]) -> Result<Cipher, KeySizeError> {
    if salt.is_empty() {
        return new_cipher(key);
    }
    let k = key.len();
    if k < 1 {
        return Err(KeySizeError(k));
    }
    let mut result = init_cipher();
    expand_key_with_salt(key, salt, &mut result);
    Ok(result)
}

impl Cipher {
    pub fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    pub fn encrypt(&self, dst: &mut [u8], src: &[u8]) {
        let l = u32::from_be_bytes([src[0], src[1], src[2], src[3]]);
        let r = u32::from_be_bytes([src[4], src[5], src[6], src[7]]);
        let (l, r) = encrypt_block(l, r, self);
        dst[0..4].copy_from_slice(&l.to_be_bytes());
        dst[4..8].copy_from_slice(&r.to_be_bytes());
    }

    pub fn encrypt_in_place(&self, block: &mut [u8]) {
        let l = u32::from_be_bytes([block[0], block[1], block[2], block[3]]);
        let r = u32::from_be_bytes([block[4], block[5], block[6], block[7]]);
        let (l, r) = encrypt_block(l, r, self);
        block[0..4].copy_from_slice(&l.to_be_bytes());
        block[4..8].copy_from_slice(&r.to_be_bytes());
    }
}

fn init_cipher() -> Cipher {
    Cipher {
        p: P,
        s0: S0,
        s1: S1,
        s2: S2,
        s3: S3,
    }
}
