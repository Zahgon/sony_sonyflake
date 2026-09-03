//! Sonyflake, a distributed unique ID generator inspired by Twitter's Snowflake.
//!
//! By default, a Sonyflake ID is composed of
//!
//! ```text
//! 39 bits for time in units of 10 msec
//!  8 bits for a sequence number
//! 16 bits for a machine id
//! ```

use std::collections::BTreeMap;
use std::sync::Mutex;

pub mod awsutil;
pub mod error;
pub mod gonet;
pub mod gotime;
pub mod mock;
pub mod types;

pub use error::Error;
use gonet::Ip;
use gotime::{Duration, Time, MILLISECOND};
use types::InterfaceAddrs;

/// nsec, i.e. 10 msec
const DEFAULT_TIME_UNIT: i64 = 10_000_000;

const DEFAULT_BITS_SEQUENCE: i32 = 8;
const DEFAULT_BITS_MACHINE: i32 = 16;

/// Configures [`Sonyflake`].
///
/// `bits_sequence` is the bit length of a sequence number.
/// If `bits_sequence` is 0, the default bit length is used, which is 8.
/// If `bits_sequence` is 31 or more, an error is returned.
///
/// `bits_machine_id` is the bit length of a machine ID.
/// If `bits_machine_id` is 0, the default bit length is used, which is 16.
/// If `bits_machine_id` is 31 or more, an error is returned.
///
/// `time_unit` is the time unit of Sonyflake.
/// If `time_unit` is 0, the default time unit is used, which is 10 msec.
/// `time_unit` must be 1 msec or longer.
///
/// `start_time` is the time since which the Sonyflake time is defined as the elapsed
/// time. If `start_time` is `None`, the start time of the Sonyflake instance is set to
/// "2025-01-01 00:00:00 +0000 UTC". `start_time` must be before the current time.
///
/// `machine_id` returns the unique ID of a Sonyflake instance.
/// If `machine_id` returns an error, the instance will not be created.
/// If `machine_id` is `None`, the default `machine_id` is used, which returns the
/// lower 16 bits of the private IP address.
///
/// `check_machine_id` validates the uniqueness of a machine ID.
/// If `check_machine_id` returns false, the instance will not be created.
/// If `check_machine_id` is `None`, no validation is done.
///
/// The bit length of time is calculated by `63 - bits_sequence - bits_machine_id`.
/// If it is less than 32, an error is returned.
#[derive(Default)]
pub struct Settings {
    pub bits_sequence: i32,
    pub bits_machine_id: i32,
    pub time_unit: Duration,
    pub start_time: Option<Time>,
    pub machine_id: Option<Box<dyn Fn() -> Result<i32, Error>>>,
    pub check_machine_id: Option<Box<dyn Fn(i32) -> bool>>,
}

/// The clock a [`Sonyflake`] reads, the equivalent of the Go struct's `now` field.
type NowFn = Box<dyn Fn() -> Time + Send + Sync>;

/// State guarded by the Sonyflake mutex.
///
/// Go protects the whole body of `NextID` with a single mutex; keeping every field
/// behind one lock reproduces that, and lets `&self` methods mutate state the way
/// Go's pointer receivers do.
struct Inner {
    bits_time: i32,
    bits_sequence: i32,
    bits_machine: i32,

    time_unit: i64,
    start_time: i64,
    elapsed_time: i64,

    sequence: i32,
    machine: i32,

    now: NowFn,
}

/// A distributed unique ID generator.
pub struct Sonyflake {
    mutex: Mutex<Inner>,
}

pub(crate) fn default_interface_addrs() -> InterfaceAddrs {
    Box::new(|| gonet::interface_addrs().map_err(Error::other))
}

impl Sonyflake {
    /// Returns a new Sonyflake configured with the given [`Settings`].
    ///
    /// Returns an error in the following cases:
    /// - `settings.bits_sequence` is less than 0 or greater than 30.
    /// - `settings.bits_machine_id` is less than 0 or greater than 30.
    /// - `settings.bits_sequence + settings.bits_machine_id` is 32 or more.
    /// - `settings.time_unit` is less than 1 msec.
    /// - `settings.start_time` is ahead of the current time.
    /// - `settings.machine_id` returns an error.
    /// - `settings.check_machine_id` returns false.
    pub fn new(st: Settings) -> Result<Sonyflake, Error> {
        if st.bits_sequence < 0 || st.bits_sequence > 30 {
            return Err(Error::InvalidBitsSequence);
        }
        if st.bits_machine_id < 0 || st.bits_machine_id > 30 {
            return Err(Error::InvalidBitsMachineId);
        }
        if st.time_unit < Duration(0) || (st.time_unit > Duration(0) && st.time_unit < MILLISECOND)
        {
            return Err(Error::InvalidTimeUnit);
        }
        if let Some(start_time) = st.start_time {
            if gotime::after(start_time, gotime::now()) {
                return Err(Error::StartTimeAhead);
            }
        }

        let bits_sequence = if st.bits_sequence == 0 {
            DEFAULT_BITS_SEQUENCE
        } else {
            st.bits_sequence
        };

        let bits_machine = if st.bits_machine_id == 0 {
            DEFAULT_BITS_MACHINE
        } else {
            st.bits_machine_id
        };

        let bits_time = 63 - bits_sequence - bits_machine;
        if bits_time < 32 {
            return Err(Error::InvalidBitsTime);
        }

        let time_unit = if st.time_unit == Duration(0) {
            DEFAULT_TIME_UNIT
        } else {
            st.time_unit.nanoseconds()
        };

        let start_time = match st.start_time {
            None => to_internal_time(time_unit, gotime::date(2025, 1, 1, 0, 0, 0, 0)),
            Some(t) => to_internal_time(time_unit, t),
        };

        let sequence = (1i32 << bits_sequence) - 1;

        let machine = match &st.machine_id {
            None => lower16_bit_private_ip(&default_interface_addrs())?,
            Some(machine_id) => machine_id()?,
        };

        if machine < 0 || machine as i64 >= 1i64 << bits_machine {
            return Err(Error::InvalidMachineId);
        }

        if let Some(check_machine_id) = &st.check_machine_id {
            if !check_machine_id(machine) {
                return Err(Error::InvalidMachineId);
            }
        }

        Ok(Sonyflake {
            mutex: Mutex::new(Inner {
                bits_time,
                bits_sequence,
                bits_machine,
                time_unit,
                start_time,
                elapsed_time: 0,
                sequence,
                machine,
                now: Box::new(gotime::now),
            }),
        })
    }

    /// Generates a next unique ID as `i64`.
    /// After the Sonyflake time overflows, `next_id` returns an error.
    pub fn next_id(&self) -> Result<i64, Error> {
        let mut sf = self.lock();
        let mask_sequence = (1i32 << sf.bits_sequence) - 1;

        let current = sf.current_elapsed_time();
        if sf.elapsed_time < current {
            sf.elapsed_time = current;
            sf.sequence = 0;
        } else {
            sf.sequence = sf.sequence.wrapping_add(1) & mask_sequence;
            if sf.sequence == 0 {
                sf.elapsed_time += 1;
                let overtime = sf.elapsed_time - current;
                // Go sleeps with the mutex still held (it is released by a deferred
                // unlock), so the guard is deliberately kept alive across the sleep.
                sf.sleep(overtime);
            }
        }

        sf.to_id()
    }

    /// Recovers the lock even if another thread panicked while holding it, which keeps
    /// behaviour equivalent to Go's `sync.Mutex` (Go has no poisoning).
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.mutex.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Returns the time when the given ID was generated.
    pub fn to_time(&self, id: i64) -> Time {
        let sf = self.lock();
        gotime::unix(0, (sf.start_time + sf.time_part(id)) * sf.time_unit)
    }

    /// Creates a Sonyflake ID from its components.
    /// The `t` parameter should be the time when the ID was generated.
    /// The `sequence` parameter should be between 0 and `2^bits_sequence-1` (inclusive).
    /// The `machine_id` parameter should be between 0 and `2^bits_machine_id-1` (inclusive).
    pub fn compose(&self, t: Time, sequence: i32, machine_id: i32) -> Result<i64, Error> {
        let sf = self.lock();
        let elapsed_time = to_internal_time(sf.time_unit, t.to_utc()) - sf.start_time;
        if elapsed_time < 0 {
            return Err(Error::StartTimeAhead);
        }
        if elapsed_time >= 1i64 << sf.bits_time {
            return Err(Error::OverTimeLimit);
        }

        if sequence < 0 || sequence as i64 >= 1i64 << sf.bits_sequence {
            return Err(Error::InvalidSequence);
        }

        if machine_id < 0 || machine_id as i64 >= 1i64 << sf.bits_machine {
            return Err(Error::InvalidMachineId);
        }

        Ok(elapsed_time << (sf.bits_sequence + sf.bits_machine)
            | (sequence as i64) << sf.bits_machine
            | machine_id as i64)
    }

    /// Returns a set of Sonyflake ID parts.
    ///
    /// A `BTreeMap` is used so that iteration and formatting are ordered by key,
    /// matching how Go's `fmt` and `encoding/json` render a map.
    pub fn decompose(&self, id: i64) -> BTreeMap<String, i64> {
        let sf = self.lock();
        let time = sf.time_part(id);
        let sequence = sf.sequence_part(id);
        let machine = sf.machine_part(id);
        BTreeMap::from([
            ("id".to_string(), id),
            ("time".to_string(), time),
            ("sequence".to_string(), sequence),
            ("machine".to_string(), machine),
        ])
    }

    /// The time unit this instance was configured with.
    pub fn time_unit(&self) -> Duration {
        Duration(self.lock().time_unit)
    }

    /// The bit length of the sequence number of this instance.
    pub fn bits_sequence(&self) -> i32 {
        self.lock().bits_sequence
    }

    /// The bit length of the machine ID of this instance.
    pub fn bits_machine(&self) -> i32 {
        self.lock().bits_machine
    }

    /// The bit length of the time part of this instance.
    pub fn bits_time(&self) -> i32 {
        self.lock().bits_time
    }

    #[cfg(test)]
    fn set_now(&self, now: NowFn) {
        self.lock().now = now;
    }

    #[cfg(test)]
    fn start_time(&self) -> i64 {
        self.lock().start_time
    }

    #[cfg(test)]
    fn sub_start_time(&self, units: i64) {
        self.lock().start_time -= units;
    }

    #[cfg(test)]
    fn to_internal_time(&self, t: Time) -> i64 {
        to_internal_time(self.lock().time_unit, t)
    }

    #[cfg(test)]
    fn time_part(&self, id: i64) -> i64 {
        self.lock().time_part(id)
    }

    #[cfg(test)]
    fn sequence_part(&self, id: i64) -> i64 {
        self.lock().sequence_part(id)
    }

    #[cfg(test)]
    fn machine_part(&self, id: i64) -> i64 {
        self.lock().machine_part(id)
    }
}

/// Go's `(*Sonyflake).toInternalTime`, lifted out so it can also run before the
/// instance exists.
fn to_internal_time(time_unit: i64, t: Time) -> i64 {
    gotime::unix_nano(t.to_utc()) / time_unit
}

impl Inner {
    fn current_elapsed_time(&self) -> i64 {
        to_internal_time(self.time_unit, (self.now)()) - self.start_time
    }

    fn sleep(&self, overtime: i64) {
        let sleep_time = Duration(overtime.wrapping_mul(self.time_unit))
            - Duration(gotime::unix_nano((self.now)().to_utc()) % self.time_unit);
        sleep_time.sleep();
    }

    fn to_id(&self) -> Result<i64, Error> {
        if self.elapsed_time >= 1i64 << self.bits_time {
            return Err(Error::OverTimeLimit);
        }

        Ok(
            self.elapsed_time << (self.bits_sequence + self.bits_machine)
                | (self.sequence as i64) << self.bits_machine
                | self.machine as i64,
        )
    }

    fn time_part(&self, id: i64) -> i64 {
        id >> (self.bits_sequence + self.bits_machine)
    }

    fn sequence_part(&self, id: i64) -> i64 {
        let mask_sequence = ((1i64 << self.bits_sequence) - 1) << self.bits_machine;
        (id & mask_sequence) >> self.bits_machine
    }

    fn machine_part(&self, id: i64) -> i64 {
        let mask_machine = (1i64 << self.bits_machine) - 1;
        id & mask_machine
    }
}

fn private_ipv4(interface_addrs: &InterfaceAddrs) -> Result<Ip, Error> {
    let as_ = interface_addrs()?;

    for a in &as_ {
        let ipnet = match a.as_ip_net() {
            Some(ipnet) if !ipnet.ip.is_loopback() => ipnet,
            _ => continue,
        };

        let ip = ipnet.ip.to4();
        if is_private_ipv4(ip.as_ref()) {
            // The guard above guarantees `to4` returned an address.
            return Ok(ip.unwrap());
        }
    }
    Err(Error::NoPrivateAddress)
}

fn is_private_ipv4(ip: Option<&Ip>) -> bool {
    // Allow private IP addresses (RFC1918) and link-local addresses (RFC3927)
    match ip {
        None => false,
        Some(ip) => {
            let ip = ip.as_bytes();
            ip[0] == 10
                || ip[0] == 172 && (ip[1] >= 16 && ip[1] < 32)
                || ip[0] == 192 && ip[1] == 168
                || ip[0] == 169 && ip[1] == 254
        }
    }
}

fn lower16_bit_private_ip(interface_addrs: &InterfaceAddrs) -> Result<i32, Error> {
    let ip = private_ipv4(interface_addrs)?;
    let ip = ip.as_bytes();

    Ok(((ip[2] as i32) << 8) + ip[3] as i32)
}

/// Renders decomposed parts the way Go's `fmt` prints a `map[string]int64`.
pub fn format_decomposed(parts: &BTreeMap<String, i64>) -> String {
    let body = parts
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("map[{body}]")
}

#[cfg(test)]
mod tests;
