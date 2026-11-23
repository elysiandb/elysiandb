use super::zoneinfo::{ALPHA, Location, LocationData, OMEGA, Zone, ZoneError, ZoneTrans, tzset};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FILE_SIZE: usize = 10 << 20;

const N_UTC_LOCAL: usize = 0;
const N_STD_WALL: usize = 1;
const N_LEAP: usize = 2;
const N_TIME: usize = 3;
const N_ZONE: usize = 4;
const N_CHAR: usize = 5;

struct DataIo<'a> {
    p: &'a [u8],
    error: bool,
}

impl<'a> DataIo<'a> {
    fn new(p: &'a [u8]) -> DataIo<'a> {
        DataIo { p, error: false }
    }

    fn read(&mut self, n: usize) -> &'a [u8] {
        if self.p.len() < n {
            self.p = &[];
            self.error = true;
            return &[];
        }
        let (head, tail) = self.p.split_at(n);
        self.p = tail;
        head
    }

    fn big4(&mut self) -> Option<u32> {
        let p = self.read(4);
        if p.len() < 4 {
            self.error = true;
            return None;
        }
        Some(u32::from_be_bytes([p[0], p[1], p[2], p[3]]))
    }

    fn big8(&mut self) -> Option<u64> {
        let n1 = self.big4();
        let n2 = self.big4();
        match (n1, n2) {
            (Some(n1), Some(n2)) => Some((u64::from(n1) << 32) | u64::from(n2)),
            _ => {
                self.error = true;
                None
            }
        }
    }

    fn byte(&mut self) -> Option<u8> {
        let p = self.read(1);
        if p.is_empty() {
            self.error = true;
            return None;
        }
        Some(p[0])
    }

    fn rest(&mut self) -> &'a [u8] {
        let r = self.p;
        self.p = &[];
        r
    }
}

fn byte_string(p: &[u8]) -> String {
    let end = p.iter().position(|&c| c == 0).unwrap_or(p.len());
    String::from_utf8_lossy(&p[..end]).into_owned()
}

fn now_unix() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64) - i64::from(e.duration().subsec_nanos() > 0),
    }
}

pub fn load_location_from_tzdata(name: &str, data: &[u8]) -> Result<Location, ZoneError> {
    parse_tzdata(name, data).map(Location::from_data)
}

pub fn load_location_from_file(name: &str, path: &Path) -> Result<Location, ZoneError> {
    let data = read_file(path.as_os_str())?;
    load_location_from_tzdata(name, &data)
}

pub(crate) fn parse_tzdata(name: &str, data: &[u8]) -> Result<LocationData, ZoneError> {
    let bad = || ZoneError::Malformed;
    let mut d = DataIo::new(data);
    if d.read(4) != b"TZif" {
        return Err(bad());
    }
    let p = d.read(16);
    if p.len() != 16 {
        return Err(bad());
    }
    let version = match p[0] {
        0 => 1,
        b'2' => 2,
        b'3' => 3,
        _ => return Err(bad()),
    };
    let mut n = [0usize; 6];
    for slot in n.iter_mut() {
        *slot = d.big4().ok_or_else(bad)? as usize;
    }
    let mut is64 = false;
    if version > 1 {
        let skip = n[N_TIME] * 4
            + n[N_TIME]
            + n[N_ZONE] * 6
            + n[N_CHAR]
            + n[N_LEAP] * 8
            + n[N_STD_WALL]
            + n[N_UTC_LOCAL]
            + 4
            + 16;
        d.read(skip);
        is64 = true;
        for slot in n.iter_mut() {
            *slot = d.big4().ok_or_else(bad)? as usize;
        }
    }
    let size = if is64 { 8 } else { 4 };
    let mut txtimes = DataIo::new(d.read(n[N_TIME] * size));
    let txzones = d.read(n[N_TIME]);
    let mut zonedata = DataIo::new(d.read(n[N_ZONE] * 6));
    let abbrev = d.read(n[N_CHAR]);
    d.read(n[N_LEAP] * (size + 4));
    d.read(n[N_STD_WALL]);
    d.read(n[N_UTC_LOCAL]);
    if d.error {
        return Err(bad());
    }
    let rest = d.rest();
    let extend = if rest.len() > 2 && rest[0] == b'\n' && rest[rest.len() - 1] == b'\n' {
        String::from_utf8_lossy(&rest[1..rest.len() - 1]).into_owned()
    } else {
        String::new()
    };
    let nzone = n[N_ZONE];
    if nzone == 0 {
        return Err(bad());
    }
    let mut zones = Vec::with_capacity(nzone);
    for _ in 0..nzone {
        let offset = zonedata.big4().ok_or_else(bad)? as i32;
        let is_dst = zonedata.byte().ok_or_else(bad)? != 0;
        let b = usize::from(zonedata.byte().ok_or_else(bad)?);
        if b >= abbrev.len() {
            return Err(bad());
        }
        zones.push(Zone {
            name: byte_string(&abbrev[b..]),
            offset,
            is_dst,
        });
    }
    let mut tx = Vec::with_capacity(n[N_TIME]);
    for &index in txzones {
        let when = if is64 {
            txtimes.big8().ok_or_else(bad)? as i64
        } else {
            i64::from(txtimes.big4().ok_or_else(bad)? as i32)
        };
        if usize::from(index) >= zones.len() {
            return Err(bad());
        }
        tx.push(ZoneTrans { when, index });
    }
    if tx.is_empty() {
        tx.push(ZoneTrans {
            when: ALPHA,
            index: 0,
        });
    }
    let mut l = LocationData {
        name: name.to_string(),
        zone: zones,
        tx,
        extend,
        cache_start: 0,
        cache_end: 0,
        cache_zone: None,
    };
    let sec = now_unix();
    let count = l.tx.len();
    let current =
        (0..count).find(|&i| l.tx[i].when <= sec && (i + 1 == count || sec < l.tx[i + 1].when));
    if let Some(i) = current {
        l.cache_start = l.tx[i].when;
        l.cache_end = OMEGA;
        l.cache_zone = Some(l.zone[usize::from(l.tx[i].index)].clone());
        if i + 1 < count {
            l.cache_end = l.tx[i + 1].when;
        } else if !l.extend.is_empty() {
            let found = tzset(&l.extend, l.cache_start, sec).map(|z| {
                (
                    Zone {
                        name: z.name.to_string(),
                        offset: z.offset,
                        is_dst: z.is_dst,
                    },
                    z.start,
                    z.end,
                )
            });
            if let Some((zone, start, end)) = found {
                l.cache_start = start;
                l.cache_end = end;
                let known = l.zone.iter().find(|z| **z == zone).cloned();
                l.cache_zone = Some(known.unwrap_or(zone));
            }
        }
    }
    Ok(l)
}

pub(crate) fn load_tzinfo_from_dir_or_zip(dir: &OsStr, name: &OsStr) -> Result<Vec<u8>, ZoneError> {
    let d = dir.as_bytes();
    if d.len() > 4 && d.ends_with(b".zip") {
        return load_tzinfo_from_zip(dir, name);
    }
    if d.is_empty() {
        return read_file(name);
    }
    let mut path = d.to_vec();
    path.push(b'/');
    path.extend_from_slice(name.as_bytes());
    read_file(&OsString::from_vec(path))
}

fn get4(b: &[u8]) -> usize {
    if b.len() < 4 {
        return 0;
    }
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
}

fn get2(b: &[u8]) -> usize {
    if b.len() < 2 {
        return 0;
    }
    usize::from(u16::from_le_bytes([b[0], b[1]]))
}

fn preadn(f: &mut File, buf: &mut [u8], off: i64) -> io::Result<()> {
    if off < 0 {
        f.seek(SeekFrom::End(off))?;
    } else {
        f.seek(SeekFrom::Start(off as u64))?;
    }
    f.read_exact(buf)
}

fn load_tzinfo_from_zip(zipfile: &OsStr, name: &OsStr) -> Result<Vec<u8>, ZoneError> {
    const ZECHEADER: usize = 0x06054b50;
    const ZCHEADER: usize = 0x02014b50;
    const ZTAILSIZE: usize = 22;
    const ZHEADERSIZE: usize = 30;
    const ZHEADER: usize = 0x04034b50;

    let path = PathBuf::from(zipfile);
    let mut f = File::open(&path).map_err(|error| ZoneError::Io {
        op: "open",
        path: path.clone(),
        error,
    })?;
    let corrupt = || ZoneError::CorruptZip(path.clone());
    let name = name.as_bytes();

    let mut buf = vec![0u8; ZTAILSIZE];
    if preadn(&mut f, &mut buf, -(ZTAILSIZE as i64)).is_err() || get4(&buf) != ZECHEADER {
        return Err(corrupt());
    }
    let n = get2(&buf[10..]);
    let size = get4(&buf[12..]);
    let off = get4(&buf[16..]);

    let mut dir = vec![0u8; size];
    if preadn(&mut f, &mut dir, off as i64).is_err() {
        return Err(corrupt());
    }
    let mut rest: &[u8] = &dir;
    for _ in 0..n {
        if get4(rest) != ZCHEADER {
            break;
        }
        if rest.len() < 46 {
            return Err(corrupt());
        }
        let meth = get2(&rest[10..]);
        let size = get4(&rest[24..]);
        let namelen = get2(&rest[28..]);
        let xlen = get2(&rest[30..]);
        let fclen = get2(&rest[32..]);
        let off = get4(&rest[42..]);
        let next = 46 + namelen + xlen + fclen;
        if rest.len() < next {
            return Err(corrupt());
        }
        let zname = &rest[46..46 + namelen];
        rest = &rest[next..];
        if zname != name {
            continue;
        }
        if meth != 0 {
            return Err(ZoneError::UnsupportedCompression {
                name: String::from_utf8_lossy(name).into_owned(),
                zip: path.clone(),
            });
        }
        let mut header = vec![0u8; ZHEADERSIZE + namelen];
        if preadn(&mut f, &mut header, off as i64).is_err()
            || get4(&header) != ZHEADER
            || get2(&header[8..]) != meth
            || get2(&header[26..]) != namelen
            || header[30..30 + namelen] != *name
        {
            return Err(corrupt());
        }
        let xlen = get2(&header[28..]);
        let mut data = vec![0u8; size];
        if preadn(&mut f, &mut data, (off + 30 + namelen + xlen) as i64).is_err() {
            return Err(corrupt());
        }
        return Ok(data);
    }
    Err(ZoneError::NotFound)
}

pub(crate) fn load_location_sources(
    name: &OsStr,
    sources: &[&OsStr],
) -> Result<LocationData, ZoneError> {
    let lname = name.to_string_lossy().into_owned();
    let mut first_err: Option<ZoneError> = None;
    for source in sources {
        let err = match load_tzinfo_from_dir_or_zip(source, name) {
            Ok(data) => match parse_tzdata(&lname, &data) {
                Ok(z) => return Ok(z),
                Err(e) => e,
            },
            Err(e) => e,
        };
        if first_err.is_none() && !err.is_not_found() {
            first_err = Some(err);
        }
    }
    Err(first_err.unwrap_or(ZoneError::Unknown(lname)))
}

fn read_file(name: &OsStr) -> Result<Vec<u8>, ZoneError> {
    let path = PathBuf::from(name);
    let mut f = File::open(&path).map_err(|error| ZoneError::Io {
        op: "open",
        path: path.clone(),
        error,
    })?;
    let mut buf = [0u8; 4096];
    let mut ret = Vec::new();
    loop {
        let n = match f.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(ZoneError::Io {
                    op: "read",
                    path,
                    error,
                });
            }
        };
        if n == 0 {
            break;
        }
        ret.extend_from_slice(&buf[..n]);
        if ret.len() > MAX_FILE_SIZE {
            return Err(ZoneError::TooLarge(path));
        }
    }
    Ok(ret)
}
