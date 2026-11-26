use super::base64::{base64_decode, base64_encode};
use crate::crypto::blowfish::{self, Cipher, KeySizeError};
use crate::crypto::{rand, subtle};
use gocompat::base64::CorruptInputError;
use gocompat::strconv::{self, NumError};
use std::{fmt, io};

pub const MIN_COST: i64 = 4;
pub const MAX_COST: i64 = 31;
pub const DEFAULT_COST: i64 = 10;

const MAJOR_VERSION: u8 = b'2';
const MINOR_VERSION: u8 = b'a';
const MAX_SALT_SIZE: usize = 16;
const MAX_CRYPTED_HASH_SIZE: usize = 23;
const ENCODED_SALT_SIZE: usize = 22;
const ENCODED_HASH_SIZE: usize = 31;
const MIN_HASH_SIZE: usize = 59;
const MAX_PASSWORD_SIZE: usize = 72;

const MAGIC_CIPHER_DATA: [u8; 24] = *b"OrpheanBeholderScryDoubt";

#[derive(Debug)]
pub enum Error {
    MismatchedHashAndPassword,
    HashTooShort,
    HashVersionTooNew(u8),
    InvalidHashPrefix(u8),
    InvalidCost(i64),
    PasswordTooLong,
    CostSyntax(NumError<i64>),
    Salt(CorruptInputError),
    KeySize(KeySizeError),
    Random(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MismatchedHashAndPassword => {
                f.write_str("the password does not match the bcrypt hash")
            }
            Error::HashTooShort => f.write_str("the bcrypt hash is too short"),
            Error::HashVersionTooNew(v) => write!(
                f,
                "bcrypt version '{}' is newer than the supported version '{}'",
                char::from(*v),
                char::from(MAJOR_VERSION)
            ),
            Error::InvalidHashPrefix(c) => write!(
                f,
                "a bcrypt hash must start with '$', not '{}'",
                char::from(*c)
            ),
            Error::InvalidCost(cost) => write!(
                f,
                "bcrypt cost {cost} is outside the range {MIN_COST} to {MAX_COST}"
            ),
            Error::PasswordTooLong => {
                write!(f, "the password is longer than {MAX_PASSWORD_SIZE} bytes")
            }
            Error::CostSyntax(e) => write!(f, "invalid bcrypt cost: {e}"),
            Error::Salt(e) => write!(f, "invalid bcrypt salt: {e}"),
            Error::KeySize(e) => e.fmt(f),
            Error::Random(e) => write!(f, "cannot read random bytes: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl PartialEq for Error {
    fn eq(&self, other: &Error) -> bool {
        match (self, other) {
            (Error::MismatchedHashAndPassword, Error::MismatchedHashAndPassword)
            | (Error::HashTooShort, Error::HashTooShort)
            | (Error::PasswordTooLong, Error::PasswordTooLong) => true,
            (Error::HashVersionTooNew(a), Error::HashVersionTooNew(b))
            | (Error::InvalidHashPrefix(a), Error::InvalidHashPrefix(b)) => a == b,
            (Error::InvalidCost(a), Error::InvalidCost(b)) => a == b,
            (Error::CostSyntax(a), Error::CostSyntax(b)) => a == b,
            (Error::Salt(a), Error::Salt(b)) => a == b,
            (Error::KeySize(a), Error::KeySize(b)) => a == b,
            _ => false,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hashed {
    pub hash: Vec<u8>,
    pub salt: Vec<u8>,
    pub cost: i64,
    pub major: u8,
    pub minor: u8,
}

pub fn generate_from_password(password: &[u8], cost: i64) -> Result<Vec<u8>, Error> {
    if password.len() > MAX_PASSWORD_SIZE {
        return Err(Error::PasswordTooLong);
    }
    let p = new_from_password(password, cost)?;
    Ok(p.hash())
}

pub fn compare_hash_and_password(hashed_password: &[u8], password: &[u8]) -> Result<(), Error> {
    let p = new_from_hash(hashed_password)?;

    let other_hash = bcrypt(password, p.cost, &p.salt)?;

    let other_p = Hashed {
        hash: other_hash,
        salt: p.salt.clone(),
        cost: p.cost,
        major: p.major,
        minor: p.minor,
    };
    if subtle::constant_time_compare(&p.hash(), &other_p.hash()) == 1 {
        return Ok(());
    }

    Err(Error::MismatchedHashAndPassword)
}

pub fn cost(hashed_password: &[u8]) -> Result<i64, Error> {
    let p = new_from_hash(hashed_password)?;
    Ok(p.cost)
}

#[doc(hidden)]
pub fn new_from_password(password: &[u8], cost: i64) -> Result<Hashed, Error> {
    let cost = if cost < MIN_COST { DEFAULT_COST } else { cost };
    let mut p = Hashed {
        major: MAJOR_VERSION,
        minor: MINOR_VERSION,
        ..Hashed::default()
    };

    check_cost(cost)?;
    p.cost = cost;

    let mut unencoded_salt = [0u8; MAX_SALT_SIZE];
    rand::os_random(&mut unencoded_salt).map_err(Error::Random)?;

    p.salt = base64_encode(&unencoded_salt);
    p.hash = bcrypt(password, p.cost, &p.salt)?;
    Ok(p)
}

#[doc(hidden)]
pub fn new_from_hash(hashed_secret: &[u8]) -> Result<Hashed, Error> {
    if hashed_secret.len() < MIN_HASH_SIZE {
        return Err(Error::HashTooShort);
    }
    let mut p = Hashed::default();
    let n = p.decode_version(hashed_secret)?;
    let hashed_secret = &hashed_secret[n..];
    let n = p.decode_cost(hashed_secret)?;
    let hashed_secret = &hashed_secret[n..];

    p.salt = hashed_secret[..ENCODED_SALT_SIZE].to_vec();

    p.hash = hashed_secret[ENCODED_SALT_SIZE..].to_vec();

    Ok(p)
}

#[doc(hidden)]
pub fn bcrypt(password: &[u8], cost: i64, salt: &[u8]) -> Result<Vec<u8>, Error> {
    let mut cipher_data = MAGIC_CIPHER_DATA;

    let c = expensive_blowfish_setup(password, cost as u32, salt)?;

    for i in (0..24).step_by(8) {
        for _ in 0..64 {
            c.encrypt_in_place(&mut cipher_data[i..i + 8]);
        }
    }

    Ok(base64_encode(&cipher_data[..MAX_CRYPTED_HASH_SIZE]))
}

fn expensive_blowfish_setup(key: &[u8], cost: u32, salt: &[u8]) -> Result<Cipher, Error> {
    let csalt = base64_decode(salt).map_err(Error::Salt)?;

    let mut ckey = Vec::with_capacity(key.len() + 1);
    ckey.extend_from_slice(key);
    ckey.push(0);

    let mut c = blowfish::new_salted_cipher(&ckey, &csalt).map_err(Error::KeySize)?;

    let rounds = 1u64.checked_shl(cost).unwrap_or(0);
    for _ in 0..rounds {
        blowfish::expand_key(&ckey, &mut c);
        blowfish::expand_key(&csalt, &mut c);
    }

    Ok(c)
}

impl Hashed {
    pub fn hash(&self) -> Vec<u8> {
        let mut arr = [0u8; 60];
        arr[0] = b'$';
        arr[1] = self.major;
        let mut n = 2;
        if self.minor != 0 {
            arr[2] = self.minor;
            n = 3;
        }
        arr[n] = b'$';
        n += 1;
        let cost = format!("{:02}", self.cost);
        copy_into(&mut arr[n..], cost.as_bytes());
        n += 2;
        arr[n] = b'$';
        n += 1;
        copy_into(&mut arr[n..], &self.salt);
        n += ENCODED_SALT_SIZE;
        copy_into(&mut arr[n..], &self.hash);
        n += ENCODED_HASH_SIZE;
        arr[..n].to_vec()
    }

    fn decode_version(&mut self, sbytes: &[u8]) -> Result<usize, Error> {
        if sbytes[0] != b'$' {
            return Err(Error::InvalidHashPrefix(sbytes[0]));
        }
        if sbytes[1] > MAJOR_VERSION {
            return Err(Error::HashVersionTooNew(sbytes[1]));
        }
        self.major = sbytes[1];
        let mut n = 3;
        if sbytes[2] != b'$' {
            self.minor = sbytes[2];
            n += 1;
        }
        Ok(n)
    }

    fn decode_cost(&mut self, sbytes: &[u8]) -> Result<usize, Error> {
        let cost = strconv::atoi(&sbytes[0..2]).map_err(Error::CostSyntax)?;
        check_cost(cost)?;
        self.cost = cost;
        Ok(3)
    }
}

fn copy_into(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}

fn check_cost(cost: i64) -> Result<(), Error> {
    if !(MIN_COST..=MAX_COST).contains(&cost) {
        return Err(Error::InvalidCost(cost));
    }
    Ok(())
}
