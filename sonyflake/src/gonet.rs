//! Minimal equivalents of the parts of Go's `net` package that sonyflake relies on.
//!
//! Go models an IP address as a byte slice that may hold either 4 or 16 bytes, and
//! sonyflake depends on that duality (`To4`, `Equal`, indexing `ip[0]`). [`Ip`] keeps
//! the same representation rather than using `std::net::IpAddr`.

use std::fmt;

/// Prefix of an IPv4-mapped IPv6 address, i.e. Go's `v4InV6Prefix`.
const V4_IN_V6_PREFIX: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff];

/// Equivalent of Go's `net.IP`. A Go nil `net.IP` is represented by `Option::None`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ip(pub Vec<u8>);

impl Ip {
    /// Go's `net.IPv4(a, b, c, d)`, but kept in the 4-byte form the mocks use.
    pub fn v4(a: u8, b: u8, c: u8, d: u8) -> Ip {
        Ip(vec![a, b, c, d])
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Go's `IP.To4`: returns the 4-byte form, or `None` when the address is not IPv4.
    pub fn to4(&self) -> Option<Ip> {
        match self.0.len() {
            4 => Some(self.clone()),
            16 if self.0[..12] == V4_IN_V6_PREFIX => Some(Ip(self.0[12..].to_vec())),
            _ => None,
        }
    }

    /// Go's `IP.IsLoopback`.
    pub fn is_loopback(&self) -> bool {
        match self.to4() {
            Some(ip4) => ip4.0[0] == 127,
            None => self.0.len() == 16 && self.0[..15].iter().all(|b| *b == 0) && self.0[15] == 1,
        }
    }

    /// Go's `IP.Equal`: byte equality, plus equality across the 4-byte and
    /// IPv4-mapped 16-byte forms of the same address.
    pub fn equal(&self, other: &Ip) -> bool {
        if self.0 == other.0 {
            return true;
        }
        match (self.0.len(), other.0.len()) {
            (4, 16) => other.0[..12] == V4_IN_V6_PREFIX && self.0[..] == other.0[12..],
            (16, 4) => self.0[..12] == V4_IN_V6_PREFIX && self.0[12..] == other.0[..],
            _ => false,
        }
    }

    /// Go's `net.ParseIP`. Returns `None` for anything it cannot parse, as Go returns nil.
    pub fn parse(s: &str) -> Option<Ip> {
        if s.contains('.') && !s.contains(':') {
            let mut octets = Vec::with_capacity(4);
            for part in s.split('.') {
                // Go rejects empty fields, over-long fields and leading zeros.
                if part.is_empty() || part.len() > 3 {
                    return None;
                }
                if part.len() > 1 && part.starts_with('0') {
                    return None;
                }
                let v: u32 = part.parse().ok()?;
                if v > 255 {
                    return None;
                }
                octets.push(v as u8);
            }
            if octets.len() != 4 {
                return None;
            }
            // net.ParseIP returns IPv4 addresses in their 16-byte mapped form.
            let mut ip = V4_IN_V6_PREFIX.to_vec();
            ip.extend_from_slice(&octets);
            return Some(Ip(ip));
        }
        if s.contains(':') {
            let addr: std::net::Ipv6Addr = s.parse().ok()?;
            return Some(Ip(addr.octets().to_vec()));
        }
        None
    }
}

/// Go's `%s` formatting of an `net.IP`.
impl fmt::Display for Ip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ip4) = self.to4() {
            return write!(f, "{}.{}.{}.{}", ip4.0[0], ip4.0[1], ip4.0[2], ip4.0[3]);
        }
        if self.0.len() == 16 {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&self.0);
            return write!(f, "{}", std::net::Ipv6Addr::from(octets));
        }
        write!(f, "?{}", hex(&self.0))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Formats an optional IP the way Go's `%s` verb renders a possibly-nil `net.IP`.
pub fn ip_to_string(ip: &Option<Ip>) -> String {
    match ip {
        Some(ip) => ip.to_string(),
        None => "<nil>".to_string(),
    }
}

/// Equivalent of Go's `net.IPNet`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpNet {
    pub ip: Ip,
    pub mask: Vec<u8>,
}

/// Equivalent of Go's `net.Addr` interface, restricted to the concrete types that
/// `net.InterfaceAddrs` can yield. Matching on the variant replaces Go's
/// `a.(*net.IPNet)` type assertion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Addr {
    IpNet(IpNet),
    IpAddr(Ip),
}

impl Addr {
    /// Go's `a.(*net.IPNet)`, returning the second `ok` result as an `Option`.
    pub fn as_ip_net(&self) -> Option<&IpNet> {
        match self {
            Addr::IpNet(n) => Some(n),
            _ => None,
        }
    }
}

/// Equivalent of Go's `net.InterfaceAddrs`, enumerating the addresses of every
/// interface on the host via `getifaddrs(3)`.
pub fn interface_addrs() -> Result<Vec<Addr>, std::io::Error> {
    // SAFETY: getifaddrs allocates a list we own; every pointer is null-checked
    // before use and the list is released with freeifaddrs on every exit path.
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut addrs = Vec::new();
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            if let Some(addr) = addr_from_ifaddr(ifa) {
                addrs.push(addr);
            }
            cur = ifa.ifa_next;
        }

        libc::freeifaddrs(head);
        Ok(addrs)
    }
}

/// Converts one `getifaddrs` entry into an [`Addr`], skipping families other than
/// AF_INET/AF_INET6 exactly as Go's netlink/route parsers do.
unsafe fn addr_from_ifaddr(ifa: &libc::ifaddrs) -> Option<Addr> {
    if ifa.ifa_addr.is_null() {
        return None;
    }
    let family = (*ifa.ifa_addr).sa_family as i32;

    match family {
        libc::AF_INET => {
            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = Ip(sin.sin_addr.s_addr.to_ne_bytes().to_vec());
            let mask = if ifa.ifa_netmask.is_null() {
                Vec::new()
            } else {
                let m = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                m.sin_addr.s_addr.to_ne_bytes().to_vec()
            };
            Some(Addr::IpNet(IpNet { ip, mask }))
        }
        libc::AF_INET6 => {
            let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
            let ip = Ip(sin6.sin6_addr.s6_addr.to_vec());
            let mask = if ifa.ifa_netmask.is_null() {
                Vec::new()
            } else {
                let m = &*(ifa.ifa_netmask as *const libc::sockaddr_in6);
                m.sin6_addr.s6_addr.to_vec()
            };
            Some(Addr::IpNet(IpNet { ip, mask }))
        }
        _ => None,
    }
}
