//! The error values returned by sonyflake, mirroring the package-level `error`
//! variables of the Go implementation.

use std::fmt;
use std::sync::Arc;

/// Equivalent of the errors sonyflake returns.
///
/// Go compares these with `errors.Is`, which for the package sentinels is pointer
/// identity on a singleton. The unit variants give the same semantics here.
/// [`Error::Other`] carries an error produced by caller-supplied code (a
/// `Settings::machine_id` callback, or the OS); it compares by pointer identity so
/// that `Other == Other` holds only for the very same error value, as in Go.
#[derive(Clone)]
pub enum Error {
    /// Go's `ErrStartTimeAhead`.
    StartTimeAhead,
    /// Go's `ErrNoPrivateAddress`.
    NoPrivateAddress,
    /// Go's `ErrOverTimeLimit`.
    OverTimeLimit,
    /// Go's `ErrInvalidMachineID`.
    InvalidMachineId,
    /// Go's `ErrInvalidSequence`.
    InvalidSequence,
    /// An error originating outside sonyflake, wrapped so it can be returned unchanged.
    Other(Arc<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Wraps an arbitrary error, as Go returns a caller's `error` untouched.
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Error {
        Error::Other(Arc::new(err))
    }

    /// Wraps a plain message, the equivalent of Go's `errors.New` / `fmt.Errorf`.
    pub fn message(msg: impl Into<String>) -> Error {
        Error::Other(Arc::new(MessageError(msg.into())))
    }
}

/// Mirrors Go's `errors.Is`: sentinels match by identity, wrapped errors by the
/// identity of the underlying value.
impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Error::StartTimeAhead, Error::StartTimeAhead)
            | (Error::NoPrivateAddress, Error::NoPrivateAddress)
            | (Error::OverTimeLimit, Error::OverTimeLimit)
            | (Error::InvalidMachineId, Error::InvalidMachineId)
            | (Error::InvalidSequence, Error::InvalidSequence) => true,
            (Error::Other(a), Error::Other(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::StartTimeAhead => f.write_str("start time is ahead of now"),
            Error::NoPrivateAddress => f.write_str("no private ip address"),
            Error::OverTimeLimit => f.write_str("over the time limit"),
            Error::InvalidMachineId => f.write_str("invalid machine id"),
            Error::InvalidSequence => f.write_str("invalid sequence number"),
            Error::Other(err) => fmt::Display::fmt(err, f),
        }
    }
}

/// `Debug` renders like `Display` so that `{:?}` in assertions reads as the Go message.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Other(err) => Some(&**err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::other(err)
    }
}

/// A bare error message, the equivalent of the value `errors.New` returns.
#[derive(Debug)]
pub struct MessageError(pub String);

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MessageError {}
