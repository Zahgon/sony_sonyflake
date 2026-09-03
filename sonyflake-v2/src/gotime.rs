//! Minimal equivalents of the parts of Go's `time` package that sonyflake relies on.
//!
//! Go's `time.Duration` is an `int64` count of nanoseconds, and sonyflake depends on
//! that representation directly (negative durations, integer truncation, silent
//! overflow). `std::time::Duration` is unsigned and saturating, so it cannot express
//! the same behaviour; [`Duration`] mirrors Go's type instead.

use std::fmt;
use std::ops::{Add, Mul, Neg, Rem, Sub};

use chrono::{DateTime, TimeZone, Utc};

/// Equivalent of Go's `time.Time`, always held in UTC.
///
/// Every use in sonyflake normalises through `.UTC()` before reading `UnixNano`, so
/// carrying a location would be unobservable.
pub type Time = DateTime<Utc>;

pub const NANOSECOND: Duration = Duration(1);
pub const MICROSECOND: Duration = Duration(1_000);
pub const MILLISECOND: Duration = Duration(1_000_000);
pub const SECOND: Duration = Duration(1_000_000_000);
pub const MINUTE: Duration = Duration(60 * 1_000_000_000);
pub const HOUR: Duration = Duration(60 * 60 * 1_000_000_000);

/// Equivalent of Go's `time.Duration`: a nanosecond count stored as `i64`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(pub i64);

impl Duration {
    /// The underlying nanosecond count, i.e. Go's `int64(d)`.
    pub const fn nanoseconds(self) -> i64 {
        self.0
    }

    pub const fn from_nanos(n: i64) -> Self {
        Duration(n)
    }

    /// Go's `time.Sleep`. A non-positive duration returns immediately, as in Go.
    pub fn sleep(self) {
        if self.0 > 0 {
            std::thread::sleep(std::time::Duration::from_nanos(self.0 as u64));
        }
    }
}

impl Add for Duration {
    type Output = Duration;
    fn add(self, rhs: Duration) -> Duration {
        Duration(self.0.wrapping_add(rhs.0))
    }
}

impl Sub for Duration {
    type Output = Duration;
    fn sub(self, rhs: Duration) -> Duration {
        Duration(self.0.wrapping_sub(rhs.0))
    }
}

impl Neg for Duration {
    type Output = Duration;
    fn neg(self) -> Duration {
        Duration(self.0.wrapping_neg())
    }
}

/// Go allows `time.Duration(n) * time.Millisecond`; both operands are the same type there.
impl Mul<Duration> for i64 {
    type Output = Duration;
    fn mul(self, rhs: Duration) -> Duration {
        Duration(self.wrapping_mul(rhs.0))
    }
}

impl Mul<i64> for Duration {
    type Output = Duration;
    fn mul(self, rhs: i64) -> Duration {
        Duration(self.0.wrapping_mul(rhs))
    }
}

impl Rem<i64> for Duration {
    type Output = Duration;
    fn rem(self, rhs: i64) -> Duration {
        Duration(self.0 % rhs)
    }
}

/// Reproduces Go's `Duration.String()` so that formatted output matches the original.
impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Ported from Go's time.Duration.String: a leading sign, then either a
        // sub-second value carrying an SI prefix, or an h/m/s breakdown.
        let mut buf = [0u8; 32];
        let mut w = buf.len();
        let neg = self.0 < 0;
        // Work in u64 so that the most negative duration does not overflow on negation.
        let mut u = self.0.unsigned_abs();

        if u < SECOND.0 as u64 {
            let prec;
            w -= 1;
            buf[w] = b's';
            w -= 1;
            if u == 0 {
                return f.write_str("0s");
            } else if u < MICROSECOND.0 as u64 {
                prec = 0;
                buf[w] = b'n';
            } else if u < MILLISECOND.0 as u64 {
                prec = 3;
                // U+00B5 'µ' is two bytes in UTF-8.
                w -= 1;
                buf[w..w + 2].copy_from_slice("µ".as_bytes());
            } else {
                prec = 6;
                buf[w] = b'm';
            }
            let (nw, nu) = fmt_frac(&mut buf, w, u, prec);
            w = nw;
            u = nu;
            w = fmt_int(&mut buf, w, u);
        } else {
            w -= 1;
            buf[w] = b's';
            let (nw, nu) = fmt_frac(&mut buf, w, u, 9);
            w = nw;
            u = nu;
            w = fmt_int(&mut buf, w, u % 60);
            u /= 60;

            if u > 0 {
                w -= 1;
                buf[w] = b'm';
                w = fmt_int(&mut buf, w, u % 60);
                u /= 60;

                if u > 0 {
                    w -= 1;
                    buf[w] = b'h';
                    w = fmt_int(&mut buf, w, u);
                }
            }
        }

        if neg {
            w -= 1;
            buf[w] = b'-';
        }
        f.write_str(std::str::from_utf8(&buf[w..]).unwrap())
    }
}

/// Writes the fraction of `v` with `prec` digits, omitting trailing zeros and the
/// point itself when nothing remains. Returns the new offset and the truncated value.
fn fmt_frac(buf: &mut [u8], mut w: usize, mut v: u64, prec: usize) -> (usize, u64) {
    let mut print = false;
    for _ in 0..prec {
        let digit = v % 10;
        print = print || digit != 0;
        if print {
            w -= 1;
            buf[w] = b'0' + digit as u8;
        }
        v /= 10;
    }
    if print {
        w -= 1;
        buf[w] = b'.';
    }
    (w, v)
}

fn fmt_int(buf: &mut [u8], mut w: usize, mut v: u64) -> usize {
    if v == 0 {
        w -= 1;
        buf[w] = b'0';
    } else {
        while v > 0 {
            w -= 1;
            buf[w] = b'0' + (v % 10) as u8;
            v /= 10;
        }
    }
    w
}

/// Go's `time.Now()`.
pub fn now() -> Time {
    Utc::now()
}

/// Go's `time.Date(y, m, d, h, min, s, nsec, time.UTC)`.
pub fn date(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32, nsec: u32) -> Time {
    Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
        .single()
        .expect("invalid date")
        + chrono::Duration::nanoseconds(nsec as i64)
}

/// Go's `time.Unix(sec, nsec)`, normalised to UTC.
pub fn unix(sec: i64, nsec: i64) -> Time {
    let total = (sec as i128) * 1_000_000_000 + nsec as i128;
    // Euclidean division so that a negative nanosecond remainder stays in [0, 1e9).
    let secs = total.div_euclid(1_000_000_000) as i64;
    let sub = total.rem_euclid(1_000_000_000) as u32;
    Utc.timestamp_opt(secs, sub)
        .single()
        .expect("timestamp out of range")
}

/// Go's `Time.UnixNano()`.
///
/// Go documents this as undefined for times outside of 1678-2262 and simply lets the
/// `int64` overflow; the `as i64` cast reproduces that wrap rather than panicking.
pub fn unix_nano(t: Time) -> i64 {
    let total = (t.timestamp() as i128) * 1_000_000_000 + t.timestamp_subsec_nanos() as i128;
    total as i64
}

/// Go's `Time.Add(d)`.
pub fn add(t: Time, d: Duration) -> Time {
    t + chrono::Duration::nanoseconds(d.0)
}

/// Go's `Time.Sub(u)`.
pub fn sub(t: Time, u: Time) -> Duration {
    Duration(unix_nano(t).wrapping_sub(unix_nano(u)))
}

/// Go's `Time.After(u)`.
pub fn after(t: Time, u: Time) -> bool {
    t > u
}
