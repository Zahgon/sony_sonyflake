//! Sonyflake, a distributed unique ID generator inspired by Twitter's Snowflake.
//!
//! A Sonyflake ID is composed of
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
use gotime::{Duration, Time};
use types::InterfaceAddrs;

/// Bit length of time.
pub const BIT_LEN_TIME: u32 = 39;
/// Bit length of sequence number.
pub const BIT_LEN_SEQUENCE: u32 = 8;
/// Bit length of machine id.
pub const BIT_LEN_MACHINE_ID: u32 = 63 - BIT_LEN_TIME - BIT_LEN_SEQUENCE;

/// Configures [`Sonyflake`].
///
/// `start_time` is the time since which the Sonyflake time is defined as the elapsed
/// time. If `start_time` is `None`, the start time of the Sonyflake is set to
/// "2014-09-01 00:00:00 +0000 UTC". If `start_time` is ahead of the current time,
/// Sonyflake is not created.
///
/// `machine_id` returns the unique ID of the Sonyflake instance. If `machine_id`
/// returns an error, Sonyflake is not created. If `machine_id` is `None`, the default
/// machine ID is used, which returns the lower 16 bits of the private IP address.
///
/// `check_machine_id` validates the uniqueness of the machine ID. If
/// `check_machine_id` returns false, Sonyflake is not created. If `check_machine_id`
/// is `None`, no validation is done.
///
/// The Go original uses zero values to mean "unset"; `Option::None` carries the same
/// meaning here, and `Settings::default()` is the equivalent of `Settings{}`.
#[derive(Default)]
pub struct Settings {
    pub start_time: Option<Time>,
    pub machine_id: Option<Box<dyn Fn() -> Result<u16, Error>>>,
    pub check_machine_id: Option<Box<dyn Fn(u16) -> bool>>,
}

/// State guarded by the Sonyflake mutex.
///
/// Go protects the whole body of `NextID` with a single mutex; keeping every mutable
/// field behind one lock reproduces that, and lets `&self` methods mutate state the
/// way Go's pointer receivers do.
struct Inner {
    start_time: i64,
    elapsed_time: i64,
    sequence: u16,
    machine_id: u16,
}

/// A distributed unique ID generator.
pub struct Sonyflake {
    mutex: Mutex<Inner>,
}

/// nsec, i.e. 10 msec
pub(crate) const SONYFLAKE_TIME_UNIT: i64 = 10_000_000;

pub(crate) fn default_interface_addrs() -> InterfaceAddrs {
    Box::new(|| gonet::interface_addrs().map_err(Error::other))
}

impl Sonyflake {
    /// Returns a new Sonyflake configured with the given [`Settings`].
    ///
    /// Returns an error in the following cases:
    /// - `settings.start_time` is ahead of the current time.
    /// - `settings.machine_id` returns an error.
    /// - `settings.check_machine_id` returns false.
    pub fn new(st: Settings) -> Result<Sonyflake, Error> {
        if let Some(start_time) = st.start_time {
            if gotime::after(start_time, gotime::now()) {
                return Err(Error::StartTimeAhead);
            }
        }

        let start_time = match st.start_time {
            None => to_sonyflake_time(gotime::date(2014, 9, 1, 0, 0, 0, 0)),
            Some(t) => to_sonyflake_time(t),
        };

        let machine_id = match &st.machine_id {
            None => lower16_bit_private_ip(&default_interface_addrs())?,
            Some(machine_id) => machine_id()?,
        };

        if let Some(check_machine_id) = &st.check_machine_id {
            if !check_machine_id(machine_id) {
                return Err(Error::InvalidMachineId);
            }
        }

        Ok(Sonyflake {
            mutex: Mutex::new(Inner {
                start_time,
                elapsed_time: 0,
                sequence: ((1u32 << BIT_LEN_SEQUENCE) - 1) as u16,
                machine_id,
            }),
        })
    }

    /// Returns a new Sonyflake configured with the given [`Settings`], or `None`.
    ///
    /// Returns `None` in the following cases:
    /// - `settings.start_time` is ahead of the current time.
    /// - `settings.machine_id` returns an error.
    /// - `settings.check_machine_id` returns false.
    ///
    /// This is the equivalent of Go's `NewSonyflake`, which returns a nil pointer.
    pub fn new_sonyflake(st: Settings) -> Option<Sonyflake> {
        Sonyflake::new(st).ok()
    }

    /// Generates a next unique ID.
    /// After the Sonyflake time overflows, `next_id` returns an error.
    pub fn next_id(&self) -> Result<u64, Error> {
        const MASK_SEQUENCE: u16 = ((1u32 << BIT_LEN_SEQUENCE) - 1) as u16;

        let mut sf = self.lock();

        let current = current_elapsed_time(sf.start_time);
        if sf.elapsed_time < current {
            sf.elapsed_time = current;
            sf.sequence = 0;
        } else {
            // sf.elapsed_time >= current
            sf.sequence = sf.sequence.wrapping_add(1) & MASK_SEQUENCE;
            if sf.sequence == 0 {
                sf.elapsed_time += 1;
                let overtime = sf.elapsed_time - current;
                // Go sleeps with the mutex still held (it is released by a deferred
                // unlock), so the guard is deliberately kept alive across the sleep.
                sleep_time(overtime).sleep();
            }
        }

        sf.to_id()
    }

    /// Recovers the lock even if another test thread panicked while holding it, which
    /// keeps behaviour equivalent to Go's `sync.Mutex` (Go has no poisoning).
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.mutex.lock().unwrap_or_else(|err| err.into_inner())
    }

    #[cfg(test)]
    fn sub_start_time(&self, units: i64) {
        self.lock().start_time -= units;
    }
}

impl Inner {
    fn to_id(&self) -> Result<u64, Error> {
        if self.elapsed_time >= 1i64 << BIT_LEN_TIME {
            return Err(Error::OverTimeLimit);
        }

        Ok(
            (self.elapsed_time as u64) << (BIT_LEN_SEQUENCE + BIT_LEN_MACHINE_ID)
                | (self.sequence as u64) << BIT_LEN_MACHINE_ID
                | self.machine_id as u64,
        )
    }
}

fn to_sonyflake_time(t: Time) -> i64 {
    gotime::unix_nano(t.to_utc()) / SONYFLAKE_TIME_UNIT
}

fn current_elapsed_time(start_time: i64) -> i64 {
    to_sonyflake_time(gotime::now()) - start_time
}

fn sleep_time(overtime: i64) -> Duration {
    Duration(overtime.wrapping_mul(SONYFLAKE_TIME_UNIT))
        - Duration(gotime::unix_nano(gotime::now().to_utc()) % SONYFLAKE_TIME_UNIT)
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

fn lower16_bit_private_ip(interface_addrs: &InterfaceAddrs) -> Result<u16, Error> {
    let ip = private_ipv4(interface_addrs)?;
    let ip = ip.as_bytes();

    Ok(((ip[2] as u16) << 8) + ip[3] as u16)
}

/// Returns the elapsed time when the given Sonyflake ID was generated.
pub fn elapsed_time(id: u64) -> Duration {
    // Go converts a uint64 product into an int64 Duration, which wraps rather than
    // panicking for very large IDs.
    Duration(elapsed_time_part(id).wrapping_mul(SONYFLAKE_TIME_UNIT as u64) as i64)
}

fn elapsed_time_part(id: u64) -> u64 {
    id >> (BIT_LEN_SEQUENCE + BIT_LEN_MACHINE_ID)
}

/// Returns the sequence number of a Sonyflake ID.
pub fn sequence_number(id: u64) -> u64 {
    const MASK_SEQUENCE: u64 = ((1u64 << BIT_LEN_SEQUENCE) - 1) << BIT_LEN_MACHINE_ID;
    (id & MASK_SEQUENCE) >> BIT_LEN_MACHINE_ID
}

/// Returns the machine ID of a Sonyflake ID.
pub fn machine_id(id: u64) -> u64 {
    const MASK_MACHINE_ID: u64 = (1u64 << BIT_LEN_MACHINE_ID) - 1;
    id & MASK_MACHINE_ID
}

/// Creates a Sonyflake ID from its parts.
pub fn compose(sf: &Sonyflake, t: Time, sequence: u16, machine_id: u16) -> Result<u64, Error> {
    let start_time = sf.lock().start_time;
    let elapsed_time = to_sonyflake_time(t.to_utc()) - start_time;
    if elapsed_time < 0 {
        return Err(Error::StartTimeAhead);
    }
    if elapsed_time >= 1i64 << BIT_LEN_TIME {
        return Err(Error::OverTimeLimit);
    }

    if sequence as u32 >= 1u32 << BIT_LEN_SEQUENCE {
        return Err(Error::InvalidSequence);
    }

    Ok(
        (elapsed_time as u64) << (BIT_LEN_SEQUENCE + BIT_LEN_MACHINE_ID)
            | (sequence as u64) << BIT_LEN_MACHINE_ID
            | machine_id as u64,
    )
}

/// Returns a set of Sonyflake ID parts.
///
/// A `BTreeMap` is used so that iteration and formatting are ordered by key, matching
/// how Go's `fmt` and `encoding/json` render a map.
pub fn decompose(id: u64) -> BTreeMap<String, u64> {
    let msb = id >> 63;
    let time = elapsed_time_part(id);
    let sequence = sequence_number(id);
    let machine_id = machine_id(id);
    BTreeMap::from([
        ("id".to_string(), id),
        ("msb".to_string(), msb),
        ("time".to_string(), time),
        ("sequence".to_string(), sequence),
        ("machine-id".to_string(), machine_id),
    ])
}

/// Renders decomposed parts the way Go's `fmt` prints a `map[string]uint64`.
pub fn format_decomposed(parts: &BTreeMap<String, u64>) -> String {
    let body = parts
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("map[{body}]")
}

#[cfg(test)]
mod tests;
