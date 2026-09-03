//! Tests for the `time` shim.
//!
//! Not a port of a Go test: the Go original gets this behaviour from `time`, so the
//! hand-written replacement is the part of this crate with no upstream coverage.
//! Each case pins a documented Go semantic.

use crate::gotime::{
    add, after, date, now, sub, unix, unix_nano, Duration, HOUR, MICROSECOND, MILLISECOND, MINUTE,
    NANOSECOND, SECOND,
};

/// Go's `time.Duration` is an `int64` nanosecond count, so the unit constants are
/// exact multiples and the accessors are lossless.
#[test]
fn duration_units_match_go() {
    assert_eq!(NANOSECOND.nanoseconds(), 1);
    assert_eq!(MICROSECOND.nanoseconds(), 1_000);
    assert_eq!(MILLISECOND.nanoseconds(), 1_000_000);
    assert_eq!(SECOND.nanoseconds(), 1_000_000_000);
    assert_eq!(MINUTE.nanoseconds(), 60 * 1_000_000_000);
    assert_eq!(HOUR.nanoseconds(), 3_600 * 1_000_000_000);
    assert_eq!(Duration::from_nanos(42).nanoseconds(), 42);
    assert_eq!(Duration::default(), Duration(0));
}

/// Go's duration arithmetic is `int64` arithmetic: it wraps rather than panicking,
/// and a duration may be negative.
#[test]
fn duration_arithmetic_matches_go() {
    assert_eq!(SECOND + MILLISECOND, Duration(1_001_000_000));
    assert_eq!(SECOND - MILLISECOND, Duration(999_000_000));
    assert_eq!(-SECOND, Duration(-1_000_000_000));
    assert_eq!(MILLISECOND - SECOND, Duration(-999_000_000));

    // Go allows the count on either side of the unit.
    assert_eq!(3 * SECOND, Duration(3_000_000_000));
    assert_eq!(SECOND * 3, Duration(3_000_000_000));

    assert_eq!(Duration(1_234) % 1_000, Duration(234));

    // int64 wrap, not overflow panic.
    assert_eq!(Duration(i64::MAX) + Duration(1), Duration(i64::MIN));
    assert_eq!(Duration(i64::MIN) - Duration(1), Duration(i64::MAX));
}

/// A non-positive `time.Sleep` returns immediately in Go rather than erroring.
#[test]
fn sleeping_a_non_positive_duration_returns_immediately() {
    Duration(0).sleep();
    (-SECOND).sleep();
    MILLISECOND.sleep();
}

/// Go's `time.Date` with a UTC location, and `Time.UnixNano` against it.
#[test]
fn date_and_unix_nano_match_go() {
    assert_eq!(unix_nano(date(1970, 1, 1, 0, 0, 0, 0)), 0);
    assert_eq!(unix_nano(date(1970, 1, 1, 0, 0, 1, 0)), 1_000_000_000);
    assert_eq!(unix_nano(date(1969, 12, 31, 23, 59, 59, 0)), -1_000_000_000);

    // The Sonyflake v1 epoch.
    assert_eq!(
        unix_nano(date(2014, 9, 1, 0, 0, 0, 0)),
        1_409_529_600_000_000_000
    );
    // The Sonyflake v2 epoch.
    assert_eq!(
        unix_nano(date(2025, 1, 1, 0, 0, 0, 0)),
        1_735_689_600_000_000_000
    );

    assert_eq!(
        unix_nano(date(2014, 9, 1, 0, 0, 0, 123_456_789)) % 1_000_000_000,
        123_456_789
    );
}

/// Go documents `UnixNano` as undefined outside 1678-2262 and simply lets the
/// `int64` wrap. That wrap is observable — it is why `v2::Sonyflake::compose` on a
/// year-2400 timestamp reports a start time ahead rather than an overflow — so the
/// shim must reproduce it rather than panic or saturate.
#[test]
fn unix_nano_wraps_past_2262_like_go() {
    let far_future = unix_nano(date(2400, 1, 1, 0, 0, 0, 0));
    assert!(
        far_future < 0,
        "expected the int64 to wrap negative, got {far_future}"
    );

    let in_range = unix_nano(date(2200, 1, 1, 0, 0, 0, 0));
    assert!(in_range > 0, "year 2200 is still in range, got {in_range}");
}

/// Go's `time.Unix`, including a nanosecond argument outside `[0, 1e9)`.
#[test]
fn unix_matches_go() {
    assert_eq!(unix_nano(unix(0, 0)), 0);
    assert_eq!(unix_nano(unix(1, 500_000_000)), 1_500_000_000);
    assert_eq!(unix_nano(unix(0, 1_500_000_000)), 1_500_000_000);
    assert_eq!(unix_nano(unix(2, -500_000_000)), 1_500_000_000);
    assert_eq!(unix_nano(unix(-1, 0)), -1_000_000_000);
}

/// Go's `Time.Add`, `Time.Sub` and `Time.After`.
#[test]
fn time_arithmetic_matches_go() {
    let base = date(2025, 1, 1, 0, 0, 0, 0);

    assert_eq!(add(base, HOUR), date(2025, 1, 1, 1, 0, 0, 0));
    assert_eq!(add(base, -HOUR), date(2024, 12, 31, 23, 0, 0, 0));

    assert_eq!(sub(add(base, HOUR), base), HOUR);
    assert_eq!(sub(base, add(base, HOUR)), -HOUR);
    assert_eq!(sub(base, base), Duration(0));

    assert!(after(add(base, NANOSECOND), base));
    assert!(!after(base, base));
    assert!(!after(base, add(base, NANOSECOND)));
}

/// `now` must be a real clock: monotone across a sleep and near the fixed epochs.
#[test]
fn now_reads_a_real_clock() {
    let first = now();
    MILLISECOND.sleep();
    let second = now();
    assert!(after(second, first) || second == first);
    assert!(after(first, date(2020, 1, 1, 0, 0, 0, 0)));
}
