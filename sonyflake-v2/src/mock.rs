//! Mock implementations of the interfaces defined in `types`.
//! This allows complete control over input / output for any given method that
//! consumes a given type.

use std::sync::LazyLock;

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

/// Go's `mock.ErrFailedToGetAddresses`.
///
/// A single lazily-created value, so that cloning it preserves the pointer identity
/// `errors.Is` relies on.
pub static ERR_FAILED_TO_GET_ADDRESSES: LazyLock<Error> =
    LazyLock::new(|| Error::message("failed to get addresses"));

/// Returns an error.
pub fn new_failing_interface_addrs() -> InterfaceAddrs {
    Box::new(|| Err(ERR_FAILED_TO_GET_ADDRESSES.clone()))
}

/// Returns an empty slice of addresses.
pub fn new_nil_interface_addrs() -> InterfaceAddrs {
    Box::new(|| Ok(Vec::new()))
}
