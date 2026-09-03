//! Type signatures used throughout sonyflake.
//! This provides the ability to mock out imports.

use crate::error::Error;
use crate::gonet::Addr;

/// Defines the interface used for retrieving network addresses.
///
/// Go declares this as the function type `func() ([]net.Addr, error)`; the Rust
/// equivalent is a boxed closure with the same signature. Any error a caller wants
/// to surface unchanged goes through [`Error::Other`].
pub type InterfaceAddrs = Box<dyn Fn() -> Result<Vec<Addr>, Error> + Send + Sync>;
