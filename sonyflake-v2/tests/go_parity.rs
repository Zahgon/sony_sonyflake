//! Golden tests pinning this port to the Go implementation.
//!
//! Every expected value here was produced by running the equivalent calls against
//! `github.com/sony/sonyflake/v2` and copying its output verbatim, so a regression in
//! the bit layout, the time arithmetic or an error message fails the build.

use sonyflake_v2::gotime::{self, Duration, MILLISECOND, SECOND};
use sonyflake_v2::{format_decomposed, Error, Settings, Sonyflake};

/// The same fixture the Go reference run used: a fixed start time and machine ID,
/// so every composed ID is deterministic.
fn fixture(bits_sequence: i32, bits_machine_id: i32, time_unit: Duration) -> Sonyflake {
    Sonyflake::new(Settings {
        bits_sequence,
        bits_machine_id,
        time_unit,
        start_time: Some(gotime::date(2025, 1, 1, 0, 0, 0, 0)),
        machine_id: Some(Box::new(|| Ok(1))),
        ..Default::default()
    })
    .expect("failed to create sonyflake")
}

#[test]
fn compose_matches_go_across_bit_layouts() {
    let tm = gotime::date(2026, 3, 4, 5, 6, 7, 890_000_000);

    let cases = [
        (
            0,
            0,
            Duration(0),
            61_926_663_650_344_961i64,
            "map[id:61926663650344961 machine:1 sequence:1 time:3691116789]",
            gotime::date(2026, 3, 4, 5, 6, 7, 890_000_000),
        ),
        (
            10,
            12,
            MILLISECOND,
            154_816_659_125_702_657,
            "map[id:154816659125702657 machine:1 sequence:1 time:36911167890]",
            gotime::date(2026, 3, 4, 5, 6, 7, 890_000_000),
        ),
        (
            4,
            8,
            100 * MILLISECOND,
            1_511_881_433_345,
            "map[id:1511881433345 machine:1 sequence:1 time:369111678]",
            gotime::date(2026, 3, 4, 5, 6, 7, 800_000_000),
        ),
        (
            30,
            1,
            SECOND,
            79_266_127_561_097_219,
            "map[id:79266127561097219 machine:1 sequence:1 time:36911167]",
            gotime::date(2026, 3, 4, 5, 6, 7, 0),
        ),
    ];

    for (bs, bm, unit, want_id, want_parts, want_time) in cases {
        let sf = fixture(bs, bm, unit);
        let id = sf.compose(tm, 1, 1).expect("compose failed");
        assert_eq!(id, want_id, "id for ({bs},{bm},{unit})");
        assert_eq!(
            format_decomposed(&sf.decompose(id)),
            want_parts,
            "parts for ({bs},{bm},{unit})"
        );
        assert_eq!(sf.to_time(id), want_time, "to_time for ({bs},{bm},{unit})");
    }
}

#[test]
fn compose_errors_match_go() {
    let sf = fixture(0, 0, Duration(0));

    let cases = [
        (
            gotime::date(2024, 1, 1, 0, 0, 0, 0),
            0i32,
            0i32,
            Error::StartTimeAhead,
            "start time is ahead",
        ),
        // Go's UnixNano silently overflows past the year 2262, which turns a
        // far-future time into a negative elapsed time rather than a limit error.
        // The port reproduces the wrap, so it must report the same error.
        (
            gotime::date(2400, 1, 1, 0, 0, 0, 0),
            0,
            0,
            Error::StartTimeAhead,
            "start time is ahead",
        ),
        (
            gotime::date(2026, 1, 1, 0, 0, 0, 0),
            256,
            0,
            Error::InvalidSequence,
            "invalid sequence number",
        ),
        (
            gotime::date(2026, 1, 1, 0, 0, 0, 0),
            0,
            65536,
            Error::InvalidMachineId,
            "invalid machine id",
        ),
    ];

    for (t, seq, mid, want, want_message) in cases {
        let err = sf.compose(t, seq, mid).expect_err("expected an error");
        assert_eq!(err, want);
        assert_eq!(err.to_string(), want_message);
    }
}

#[test]
fn new_errors_match_go() {
    let cases: Vec<(Settings, Error, &str)> = vec![
        (
            Settings {
                bits_sequence: 16,
                bits_machine_id: 16,
                ..Default::default()
            },
            Error::InvalidBitsTime,
            "bit length for time must be 32 or more",
        ),
        (
            Settings {
                bits_sequence: 31,
                ..Default::default()
            },
            Error::InvalidBitsSequence,
            "invalid bit length for sequence number",
        ),
        (
            Settings {
                bits_machine_id: -1,
                ..Default::default()
            },
            Error::InvalidBitsMachineId,
            "invalid bit length for machine id",
        ),
        (
            Settings {
                time_unit: Duration(-1),
                ..Default::default()
            },
            Error::InvalidTimeUnit,
            "invalid time unit",
        ),
    ];

    for (settings, want, want_message) in cases {
        let err = Sonyflake::new(settings).err().expect("expected an error");
        assert_eq!(err, want);
        assert_eq!(err.to_string(), want_message);
    }

    assert_eq!(Error::NoPrivateAddress.to_string(), "no private ip address");
    assert_eq!(Error::OverTimeLimit.to_string(), "over the time limit");
}

/// A time unit of exactly 1 msec is the documented lower bound and must be accepted.
#[test]
fn time_unit_boundary_matches_go() {
    assert!(Sonyflake::new(Settings {
        time_unit: MILLISECOND,
        machine_id: Some(Box::new(|| Ok(1))),
        ..Default::default()
    })
    .is_ok());
    assert_eq!(
        Sonyflake::new(Settings {
            time_unit: MILLISECOND - Duration(1),
            machine_id: Some(Box::new(|| Ok(1))),
            ..Default::default()
        })
        .err(),
        Some(Error::InvalidTimeUnit)
    );
}

/// Go's default start time is "2025-01-01 00:00:00 +0000 UTC".
#[test]
fn default_start_time_matches_go() {
    let default = Sonyflake::new(Settings {
        machine_id: Some(Box::new(|| Ok(1))),
        ..Default::default()
    })
    .expect("failed to create sonyflake");

    let t = gotime::date(2026, 3, 4, 5, 6, 7, 890_000_000);
    assert_eq!(default.compose(t, 1, 1), Ok(61_926_663_650_344_961));
}

/// The default bit lengths are part of the wire format and must not drift.
#[test]
fn default_bit_lengths_match_go() {
    let sf = fixture(0, 0, Duration(0));
    assert_eq!(sf.bits_sequence(), 8);
    assert_eq!(sf.bits_machine(), 16);
    assert_eq!(sf.bits_time(), 39);
    assert_eq!(sf.time_unit(), 10 * MILLISECOND);
}
