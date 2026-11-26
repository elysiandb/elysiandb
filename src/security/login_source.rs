use std::fmt;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use crate::globals;
use crate::gocompat::strings;
use crate::httpd::RequestCtx;

const IPV4_LEN: usize = 4;
const IPV6_LEN: usize = 16;
const V4_IN_V6_PREFIX: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff];
const FORWARDING_HEADERS: [&str; 3] = ["X-Forwarded-For", "X-Real-Ip", "Forwarded"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoginSource {
    pub ip: Option<IpAddr>,
    pub local: bool,
}

impl LoginSource {
    pub fn new(ip: Option<IpAddr>, local: bool) -> LoginSource {
        LoginSource {
            ip: ip.map(|ip| ip.to_canonical()),
            local,
        }
    }

    pub fn client_key(&self) -> String {
        match self.ip.map(|ip| ip.to_canonical()) {
            None => "unknown".to_string(),
            Some(IpAddr::V4(v4)) => v4.to_string(),
            Some(IpAddr::V6(v6)) => {
                let mut octets = v6.octets();
                for b in &mut octets[8..] {
                    *b = 0;
                }
                format!("{}/64", Ipv6Addr::from(octets))
            }
        }
    }
}

impl fmt::Display for LoginSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ip {
            None => f.write_str("unknown"),
            Some(ip) => write!(f, "{}", ip.to_canonical()),
        }
    }
}

pub fn parse_ip(s: &[u8]) -> Option<IpAddr> {
    let text = std::str::from_utf8(s).ok()?;
    let ip: IpAddr = text.parse().ok()?;
    Some(ip.to_canonical())
}

fn is_loopback(ip: IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

fn to4(ip: &[u8]) -> Option<&[u8]> {
    if ip.len() == IPV4_LEN {
        return Some(ip);
    }
    if ip.len() == IPV6_LEN && ip[..12] == V4_IN_V6_PREFIX {
        return Some(&ip[12..]);
    }
    None
}

fn cidr_mask(ones: usize, bits: usize) -> Vec<u8> {
    let mut mask = vec![0u8; bits / 8];
    let mut n = ones;
    for b in &mut mask {
        if n >= 8 {
            *b = 0xff;
            n -= 8;
            continue;
        }
        *b = !(0xffu8 >> n);
        n = 0;
    }
    mask
}

fn apply_mask(ip: &[u8], mask: &[u8]) -> Option<Vec<u8>> {
    let mut mask = mask;
    let mut ip = ip;
    if mask.len() == IPV6_LEN && ip.len() == IPV4_LEN && mask[..12].iter().all(|&b| b == 0xff) {
        mask = &mask[12..];
    }
    if mask.len() == IPV4_LEN && ip.len() == IPV6_LEN && ip[..12] == V4_IN_V6_PREFIX {
        ip = &ip[12..];
    }
    if ip.len() != mask.len() {
        return None;
    }
    Some(ip.iter().zip(mask).map(|(a, m)| a & m).collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpNet {
    pub ip: Vec<u8>,
    pub mask: Vec<u8>,
}

impl IpNet {
    fn network_number_and_mask(&self) -> Option<(&[u8], &[u8])> {
        let ip = match to4(&self.ip) {
            Some(ip) => ip,
            None => {
                if self.ip.len() != IPV6_LEN {
                    return None;
                }
                &self.ip[..]
            }
        };
        let mut m = &self.mask[..];
        match m.len() {
            IPV4_LEN => {
                if ip.len() != IPV4_LEN {
                    return None;
                }
            }
            IPV6_LEN => {
                if ip.len() == IPV4_LEN {
                    m = &m[12..];
                }
            }
            _ => return None,
        }
        Some((ip, m))
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let Some((nn, m)) = self.network_number_and_mask() else {
            return false;
        };
        let raw = match ip {
            IpAddr::V4(v4) => v4.octets().to_vec(),
            IpAddr::V6(v6) => v6.octets().to_vec(),
        };
        let ip = to4(&raw).unwrap_or(&raw);
        if ip.len() != nn.len() {
            return false;
        }
        (0..ip.len()).all(|i| nn[i] & m[i] == ip[i] & m[i])
    }
}

fn dtoi(s: &[u8]) -> Option<(usize, usize)> {
    const BIG: usize = 0xFFFFFF;
    let mut n = 0usize;
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        n = n * 10 + usize::from(s[i] - b'0');
        if n >= BIG {
            return None;
        }
        i += 1;
    }
    if i == 0 {
        return None;
    }
    Some((n, i))
}

fn parse_cidr(s: &[u8]) -> Option<IpNet> {
    let slash = s.iter().position(|&b| b == b'/')?;
    let (addr, mask) = (&s[..slash], &s[slash + 1..]);
    let addr: IpAddr = std::str::from_utf8(addr).ok()?.parse().ok()?;
    let bits = match addr {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    let (n, i) = dtoi(mask)?;
    if i != mask.len() || n > bits {
        return None;
    }
    let m = cidr_mask(n, bits);
    let addr16 = match addr {
        IpAddr::V4(v4) => v4.to_ipv6_mapped().octets(),
        IpAddr::V6(v6) => v6.octets(),
    };
    let ip = apply_mask(&addr16, &m)?;
    Some(IpNet { ip, mask: m })
}

pub fn parse_network(entry: &[u8]) -> Option<IpNet> {
    if let Some(network) = parse_cidr(entry) {
        return Some(network);
    }

    let ip = parse_ip(entry)?;
    let raw = match ip {
        IpAddr::V4(v4) => v4.to_ipv6_mapped().octets(),
        IpAddr::V6(v6) => v6.octets(),
    };
    if let Some(v4) = to4(&raw) {
        return Some(IpNet {
            ip: v4.to_vec(),
            mask: cidr_mask(32, 32),
        });
    }

    Some(IpNet {
        ip: raw.to_vec(),
        mask: cidr_mask(128, 128),
    })
}

fn ip_is_trusted(ip: Option<IpAddr>, proxies: &[IpNet]) -> bool {
    let Some(ip) = ip else {
        return false;
    };

    proxies.iter().any(|network| network.contains(ip))
}

fn trusted_proxies() -> Vec<IpNet> {
    let Some(cfg) = globals::config() else {
        return Vec::new();
    };

    cfg.server
        .http
        .trusted_proxies
        .0
        .iter()
        .flatten()
        .filter_map(|entry| parse_network(strings::trim_space(entry.as_bytes())))
        .collect()
}

pub fn login_source_from_request(ctx: &RequestCtx) -> LoginSource {
    let peer = ctx.remote_ip().to_canonical();
    let proxies = trusted_proxies();

    if ip_is_trusted(Some(peer), &proxies)
        && let Some(client) = forwarded_client_ip(ctx, &proxies)
    {
        return LoginSource::new(Some(client), is_loopback(client));
    }

    LoginSource::new(Some(peer), is_loopback(peer) && !request_is_forwarded(ctx))
}

pub fn login_source_from_ip(ip: Option<IpAddr>) -> LoginSource {
    match ip {
        Some(ip) => LoginSource::new(Some(ip), is_loopback(ip)),
        None => LoginSource::default(),
    }
}

pub fn login_source_from_addr(addr: Option<SocketAddr>) -> LoginSource {
    login_source_from_ip(addr.map(|a| a.ip()))
}

fn request_is_forwarded(ctx: &RequestCtx) -> bool {
    FORWARDING_HEADERS
        .iter()
        .any(|header| !ctx.request.header.peek(header).is_empty())
}

fn forwarded_client_ip(ctx: &RequestCtx, proxies: &[IpNet]) -> Option<IpAddr> {
    let mut hops: Vec<&[u8]> = Vec::new();
    for value in ctx.request.header.peek_all("X-Forwarded-For") {
        hops.extend(value.split(|&b| b == b','));
    }

    if hops.is_empty()
        && let Some(ip) = parse_ip(strings::trim_space(ctx.request.header.peek("X-Real-Ip")))
    {
        return Some(ip);
    }

    let mut leftmost = None;
    for hop in hops.iter().rev() {
        let ip = parse_ip(strings::trim_space(hop))?;

        if !ip_is_trusted(Some(ip), proxies) {
            return Some(ip);
        }

        leftmost = Some(ip);
    }

    leftmost
}
