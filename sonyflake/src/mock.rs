//! Mock implementations of the interfaces defined in `types`.
//! This allows complete control over input / output for any given method that
//! consumes a given type.

use crate::error::Error;
use crate::gonet::{Addr, Ip, IpNet};
use crate::types::InterfaceAddrs;

/// Returns a single private IP address.
pub fn new_successful_interface_addrs() -> InterfaceAddrs {
    let ifat = vec![Addr::IpNet(IpNet {
        ip: Ip(vec![192, 168, 0, 1]),
        mask: vec![255, 0, 0, 0],
    })];

    Box::new(move || Ok(ifat.clone()))
}

/// Returns an error.
///
/// The Go original builds the error with `fmt.Errorf` on every call, so each result
/// is a distinct value carrying the same message.
pub fn new_failing_interface_addrs() -> InterfaceAddrs {
    Box::new(|| Err(Error::message("test error")))
}

/// Returns an empty slice of addresses.
pub fn new_nil_interface_addrs() -> InterfaceAddrs {
    Box::new(|| Ok(Vec::new()))
}
