//! Tests for the `net` shim.
//!
//! Not a port of a Go test: the Go original gets this behaviour from `net`, so the
//! hand-written replacement is the part of this crate with no upstream coverage.
//! Each case pins a documented Go semantic.

use crate::gonet::{interface_addrs, ip_to_string, Addr, Ip, IpNet};

/// Go's `IP.To4`: a 4-byte address is itself, an IPv4-mapped 16-byte address is
/// its last four bytes, and anything else is nil.
#[test]
fn to4_matches_go() {
    assert_eq!(Ip::v4(192, 168, 0, 1).to4(), Some(Ip::v4(192, 168, 0, 1)));

    let mapped = Ip(vec![
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 192, 168, 0, 1,
    ]);
    assert_eq!(mapped.to4(), Some(Ip::v4(192, 168, 0, 1)));

    let v6 = Ip(vec![
        0x20, 0x01, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    assert_eq!(v6.to4(), None);

    assert_eq!(Ip(vec![1, 2, 3]).to4(), None);
}

/// Go's `IP.IsLoopback`, over both address widths.
#[test]
fn is_loopback_matches_go() {
    assert!(Ip::v4(127, 0, 0, 1).is_loopback());
    assert!(Ip::v4(127, 255, 255, 254).is_loopback());
    assert!(!Ip::v4(192, 168, 0, 1).is_loopback());

    let mapped_loopback = Ip(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 127, 0, 0, 1]);
    assert!(mapped_loopback.is_loopback());

    let mut v6_loopback = vec![0u8; 16];
    v6_loopback[15] = 1;
    assert!(Ip(v6_loopback).is_loopback());

    assert!(!Ip(vec![0u8; 16]).is_loopback());
}

/// Go's `IP.Equal` treats the 4-byte and IPv4-mapped 16-byte forms of one address
/// as equal, which plain byte equality would not.
#[test]
fn equal_matches_go() {
    let four = Ip::v4(192, 168, 0, 1);
    let mapped = Ip(vec![
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 192, 168, 0, 1,
    ]);

    assert!(four.equal(&four));
    assert!(four.equal(&mapped));
    assert!(mapped.equal(&four));
    assert!(!four.equal(&Ip::v4(192, 168, 0, 2)));

    // A 16-byte address that is not IPv4-mapped never equals a 4-byte one.
    let v6 = Ip(vec![
        0x20, 0x01, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 192, 168, 0, 1,
    ]);
    assert!(!four.equal(&v6));
}

/// Go's `net.ParseIP`: IPv4 comes back in its 16-byte mapped form, and the strict
/// field rules (no leading zeros, no out-of-range octet, exactly four fields)
/// reject everything else.
#[test]
fn parse_matches_go() {
    let parsed = Ip::parse("192.168.0.1").expect("should parse");
    assert_eq!(parsed.as_bytes().len(), 16);
    assert!(parsed.equal(&Ip::v4(192, 168, 0, 1)));

    assert_eq!(Ip::parse("010.1.1.1"), None, "leading zero is rejected");
    assert_eq!(Ip::parse("256.1.1.1"), None, "octet out of range");
    assert_eq!(Ip::parse("1.2.3"), None, "too few fields");
    assert_eq!(Ip::parse("1.2.3.4.5"), None, "too many fields");
    assert_eq!(Ip::parse("1.2.3."), None, "empty field");
    assert_eq!(Ip::parse("1.2.3.x"), None, "non-numeric field");
    assert_eq!(Ip::parse(""), None);
    assert_eq!(Ip::parse("not an ip"), None);

    assert!(Ip::parse("::1").is_some(), "IPv6 parses");
    assert_eq!(Ip::parse("::zz"), None);
}

/// Go's `%s` verb on a `net.IP`, including the nil rendering.
#[test]
fn display_matches_go() {
    assert_eq!(Ip::v4(192, 168, 0, 1).to_string(), "192.168.0.1");

    let mapped = Ip(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 10, 0, 0, 7]);
    assert_eq!(mapped.to_string(), "10.0.0.7");

    let mut v6 = vec![0u8; 16];
    v6[15] = 1;
    assert_eq!(Ip(v6).to_string(), "::1");

    // A length Go cannot interpret renders as its hex, prefixed with '?'.
    assert_eq!(Ip(vec![1, 2, 3]).to_string(), "?010203");

    assert_eq!(ip_to_string(&Some(Ip::v4(10, 0, 0, 1))), "10.0.0.1");
    assert_eq!(ip_to_string(&None), "<nil>");
}

/// The replacement for Go's `a.(*net.IPNet)` type assertion.
#[test]
fn addr_as_ip_net_matches_go_type_assertion() {
    let ipnet = IpNet {
        ip: Ip::v4(192, 168, 0, 1),
        mask: vec![255, 255, 255, 0],
    };
    let addr = Addr::IpNet(ipnet.clone());
    assert_eq!(addr.as_ip_net(), Some(&ipnet));

    assert_eq!(Addr::IpAddr(Ip::v4(192, 168, 0, 1)).as_ip_net(), None);
}

/// The `getifaddrs` replacement for `net.InterfaceAddrs` must at least enumerate
/// the loopback interface on any host this can run on.
#[test]
fn interface_addrs_enumerates_the_host() {
    let addrs = interface_addrs().expect("interface addresses should be readable");
    assert!(!addrs.is_empty(), "expected at least one interface address");
    assert!(
        addrs
            .iter()
            .filter_map(Addr::as_ip_net)
            .any(|net| net.ip.is_loopback()),
        "expected a loopback address among {addrs:?}"
    );
}
