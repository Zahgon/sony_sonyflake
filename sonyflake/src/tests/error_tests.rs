//! Tests for the error type.
//!
//! Not a port of a Go test: Go gets sentinel identity from `errors.Is` on package
//! variables, and this enum has to reproduce it. The message texts are part of the
//! public behaviour and are asserted verbatim against the Go originals.

use std::error::Error as StdError;

use crate::error::{Error, MessageError};

/// The five package-level `error` variables of the Go implementation.
#[test]
fn messages_match_go() {
    assert_eq!(
        Error::StartTimeAhead.to_string(),
        "start time is ahead of now"
    );
    assert_eq!(Error::NoPrivateAddress.to_string(), "no private ip address");
    assert_eq!(Error::OverTimeLimit.to_string(), "over the time limit");
    assert_eq!(Error::InvalidMachineId.to_string(), "invalid machine id");
    assert_eq!(
        Error::InvalidSequence.to_string(),
        "invalid sequence number"
    );
}

/// `Debug` renders like `Display` so assertion output reads as the Go message.
#[test]
fn debug_renders_like_display() {
    assert_eq!(
        format!("{:?}", Error::OverTimeLimit),
        Error::OverTimeLimit.to_string()
    );
    let wrapped = Error::message("boom");
    assert_eq!(format!("{wrapped:?}"), "boom");
}

/// Go's `errors.Is` matches a sentinel by identity, and every value of one sentinel
/// is the same value.
#[test]
fn sentinels_compare_by_variant() {
    assert_eq!(Error::StartTimeAhead, Error::StartTimeAhead);
    assert_eq!(Error::InvalidSequence, Error::InvalidSequence);
    assert_ne!(Error::StartTimeAhead, Error::OverTimeLimit);
    assert_ne!(Error::InvalidMachineId, Error::InvalidSequence);
}

/// A wrapped error keeps Go's identity semantics: `errors.Is` is true only for the
/// very same error value, never for a distinct value carrying the same text.
#[test]
fn wrapped_errors_compare_by_identity() {
    let one = Error::message("failed to get machine id");
    let same = one.clone();
    let other = Error::message("failed to get machine id");

    assert_eq!(one, same, "a clone shares the underlying value");
    assert_ne!(
        one, other,
        "two distinct errors with equal text must not compare equal"
    );

    assert_ne!(one, Error::StartTimeAhead);
    assert_ne!(Error::StartTimeAhead, one);
}

/// `source` exposes a wrapped cause and nothing for the sentinels.
#[test]
fn source_exposes_only_the_wrapped_cause() {
    let wrapped = Error::message("inner");
    assert_eq!(
        wrapped.source().map(ToString::to_string),
        Some("inner".into())
    );
    assert!(Error::OverTimeLimit.source().is_none());
}

/// The three constructors all produce a wrapped error carrying the original text.
#[test]
fn constructors_preserve_the_message() {
    let io = std::io::Error::other("io failed");
    assert_eq!(Error::other(io).to_string(), "io failed");

    assert_eq!(Error::message("plain").to_string(), "plain");
    assert_eq!(MessageError("bare".into()).to_string(), "bare");
}

/// `?` on an io error must produce a wrapped sonyflake error.
#[test]
fn io_errors_convert() {
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    let converted: Error = io.into();
    assert_eq!(converted.to_string(), "missing");
    assert!(converted.source().is_some());
}
