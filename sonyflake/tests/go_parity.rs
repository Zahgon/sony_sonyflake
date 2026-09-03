//! Golden tests pinning this port to the Go implementation.
//!
//! Every expected value here was produced by running the equivalent calls against
//! `github.com/sony/sonyflake` and copying its output verbatim, so a regression in
//! the bit layout, the time arithmetic or an error message fails the build.

use sonyflake::gotime;
use sonyflake::{compose, decompose, elapsed_time, format_decomposed, machine_id, sequence_number};
use sonyflake::{Error, Settings, Sonyflake};

/// The same fixture the Go reference run used: a fixed start time and machine ID,
/// so every composed ID is deterministic.
fn fixture() -> Sonyflake {
    Sonyflake::new(Settings {
        start_time: Some(gotime::date(2014, 9, 1, 0, 0, 0, 0)),
        machine_id: Some(Box::new(|| Ok(0xabcd))),
        ..Default::default()
    })
    .expect("failed to create sonyflake")
}

#[test]
fn compose_matches_go() {
    let sf = fixture();

    let cases = [
        (
            gotime::date(2014, 9, 1, 0, 0, 0, 0),
            0u16,
            0u16,
            0u64,
            "map[id:0 machine-id:0 msb:0 sequence:0 time:0]",
            0i64,
        ),
        (
            gotime::date(2020, 1, 2, 3, 4, 5, 678_000_000),
            255,
            65535,
            282_536_111_597_682_687,
            "map[id:282536111597682687 machine-id:65535 msb:0 sequence:255 time:16840464567]",
            168_404_645_670_000_000,
        ),
        (
            gotime::date(2023, 6, 15, 12, 0, 0, 0),
            123,
            456,
            465_233_541_865_341_384,
            "map[id:465233541865341384 machine-id:456 msb:0 sequence:123 time:27730080000]",
            277_300_800_000_000_000,
        ),
        (
            gotime::date(2188, 1, 1, 0, 0, 0, 0),
            1,
            1,
            9_176_965_353_308_225_537,
            "map[id:9176965353308225537 machine-id:1 msb:0 sequence:1 time:546989760000]",
            5_469_897_600_000_000_000,
        ),
    ];

    for (t, seq, mid, want_id, want_parts, want_elapsed) in cases {
        let id = compose(&sf, t, seq, mid).expect("compose failed");
        assert_eq!(id, want_id, "id for {t}");
        assert_eq!(
            format_decomposed(&decompose(id)),
            want_parts,
            "parts for {t}"
        );
        assert_eq!(
            elapsed_time(id).nanoseconds(),
            want_elapsed,
            "elapsed for {t}"
        );
        assert_eq!(sequence_number(id), seq as u64, "sequence for {t}");
        assert_eq!(machine_id(id), mid as u64, "machine id for {t}");
    }
}

#[test]
fn compose_errors_match_go() {
    let sf = fixture();

    let cases = [
        (
            gotime::date(2014, 1, 1, 0, 0, 0, 0),
            0u16,
            Error::StartTimeAhead,
            "start time is ahead of now",
        ),
        (
            gotime::date(2200, 1, 1, 0, 0, 0, 0),
            0,
            Error::OverTimeLimit,
            "over the time limit",
        ),
        (
            gotime::date(2020, 1, 1, 0, 0, 0, 0),
            256,
            Error::InvalidSequence,
            "invalid sequence number",
        ),
    ];

    for (t, seq, want, want_message) in cases {
        let err = compose(&sf, t, seq, 0).expect_err("expected an error");
        assert_eq!(err, want);
        assert_eq!(err.to_string(), want_message);
    }
}

#[test]
fn error_messages_match_go() {
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

/// Go's default start time is "2014-09-01 00:00:00 +0000 UTC".
#[test]
fn default_start_time_matches_go() {
    let default = Sonyflake::new(Settings {
        machine_id: Some(Box::new(|| Ok(0))),
        ..Default::default()
    })
    .expect("failed to create sonyflake");
    let explicit = Sonyflake::new(Settings {
        start_time: Some(gotime::date(2014, 9, 1, 0, 0, 0, 0)),
        machine_id: Some(Box::new(|| Ok(0))),
        ..Default::default()
    })
    .expect("failed to create sonyflake");

    let t = gotime::date(2020, 1, 2, 3, 4, 5, 678_000_000);
    assert_eq!(compose(&default, t, 0, 0), compose(&explicit, t, 0, 0));
}

/// The bit lengths are part of the wire format and must not drift.
#[test]
fn bit_lengths_match_go() {
    assert_eq!(sonyflake::BIT_LEN_TIME, 39);
    assert_eq!(sonyflake::BIT_LEN_SEQUENCE, 8);
    assert_eq!(sonyflake::BIT_LEN_MACHINE_ID, 16);
}
