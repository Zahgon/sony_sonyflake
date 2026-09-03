//! Port of `v2/sonyflake_test.go`.
//!
//! Unlike v1, every test here builds its own generator, so the tests are independent
//! and safe to run in parallel the way `cargo test` does.
//!
//! Go's `t.Run` sub-cases become one `#[test]` each, named `<parent>_<sub-case>`.
//! Rust has no sub-test concept, so a table driven from a single test function would
//! collapse every case into one pass/fail and would stop at the first failure; a
//! function per case keeps the individual reporting Go gives these tables.

// The `time` and `net` shims replace Go standard-library behaviour, so they carry
// their own tests; the Go suite has no counterpart to inherit.
// Suffixed so they do not shadow the `crate::error` / `crate::gonet` / `crate::gotime`
// names this module glob-imports from the crate root.
mod error_tests;
mod gonet_tests;
mod gotime_tests;

use std::collections::HashSet;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;

use super::*;
use crate::gonet::Ip;
use crate::gotime::{Duration, HOUR, MICROSECOND, MINUTE};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_sonyflake(st: Settings) -> Sonyflake {
    match Sonyflake::new(st) {
        Ok(sf) => sf,
        Err(err) => panic!("failed to create sonyflake: {err}"),
    }
}

fn next_id(sf: &Sonyflake) -> i64 {
    match sf.next_id() {
        Ok(id) => id,
        Err(err) => panic!("failed to generate id: {err}"),
    }
}

fn default_machine_id() -> i32 {
    match lower16_bit_private_ip(&default_interface_addrs()) {
        Ok(ip) => ip,
        Err(err) => panic!("failed to get private ip address: {err}"),
    }
}

// ---------------------------------------------------------------------------
// TestNew
// ---------------------------------------------------------------------------

/// One row of Go's `TestNew` table.
fn assert_new(name: &str, settings: Settings, want: Option<Error>) {
    let result = Sonyflake::new(settings);

    let err = result.as_ref().err().cloned();
    assert!(err == want, "{name}: unexpected error: {err:?}");

    // `Result` makes the "nil instance with nil error" case unrepresentable, but
    // the assertion is kept so the ported test covers the same condition.
    assert!(
        !(want.is_none() && result.is_err()),
        "{name}: sonyflake instance must be created"
    );
}

#[test]
fn test_new_invalid_bit_length_for_time() {
    assert_new(
        "invalid bit length for time",
        Settings {
            bits_sequence: 16,
            bits_machine_id: 16,
            ..Default::default()
        },
        Some(Error::InvalidBitsTime),
    );
}

#[test]
fn test_new_invalid_bit_length_for_sequence_number() {
    assert_new(
        "invalid bit length for sequence number",
        Settings {
            bits_sequence: -1,
            ..Default::default()
        },
        Some(Error::InvalidBitsSequence),
    );
}

#[test]
fn test_new_invalid_bit_length_for_machine_id() {
    assert_new(
        "invalid bit length for machine id",
        Settings {
            bits_machine_id: 31,
            ..Default::default()
        },
        Some(Error::InvalidBitsMachineId),
    );
}

#[test]
fn test_new_invalid_time_unit() {
    assert_new(
        "invalid time unit",
        Settings {
            time_unit: MICROSECOND,
            ..Default::default()
        },
        Some(Error::InvalidTimeUnit),
    );
}

#[test]
fn test_new_start_time_ahead() {
    assert_new(
        "start time ahead",
        Settings {
            start_time: Some(gotime::add(gotime::now(), MINUTE)),
            ..Default::default()
        },
        Some(Error::StartTimeAhead),
    );
}

#[test]
fn test_new_cannot_get_machine_id() {
    let err_get_machine_id = Error::message("failed to get machine id");
    assert_new(
        "cannot get machine id",
        Settings {
            machine_id: Some({
                let err = err_get_machine_id.clone();
                Box::new(move || Err(err.clone()))
            }),
            ..Default::default()
        },
        Some(err_get_machine_id),
    );
}

#[test]
fn test_new_too_large_machine_id() {
    assert_new(
        "too large machine id",
        Settings {
            machine_id: Some(Box::new(|| Ok(1 << DEFAULT_BITS_MACHINE))),
            ..Default::default()
        },
        Some(Error::InvalidMachineId),
    );
}

#[test]
fn test_new_negative_machine_id() {
    assert_new(
        "negative machine id",
        Settings {
            machine_id: Some(Box::new(|| Ok(-1))),
            ..Default::default()
        },
        Some(Error::InvalidMachineId),
    );
}

#[test]
fn test_new_invalid_machine_id() {
    assert_new(
        "invalid machine id",
        Settings {
            check_machine_id: Some(Box::new(|_| false)),
            ..Default::default()
        },
        Some(Error::InvalidMachineId),
    );
}

#[test]
fn test_new_success() {
    assert_new("success", Settings::default(), None);
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

#[test]
fn test_next_id() {
    let start = gotime::now();
    let sf = new_sonyflake(Settings {
        start_time: Some(start),
        ..Default::default()
    });

    let sleep_time: i64 = 50;
    let time_unit = sf.time_unit().nanoseconds();
    sf.set_now(Box::new(move || {
        gotime::add(start, Duration(sleep_time * time_unit))
    }));

    let id = next_id(&sf);

    let actual_time = sf.time_part(id);
    if actual_time != sleep_time {
        panic!("unexpected time: {actual_time}");
    }

    let actual_sequence = sf.sequence_part(id);
    if actual_sequence != 0 {
        panic!("unexpected sequence: {actual_sequence}");
    }

    let actual_machine = sf.machine_part(id);
    if actual_machine != default_machine_id() as i64 {
        panic!("unexpected machine: {actual_machine}");
    }

    println!("sonyflake id: {id}");
    println!("decompose: {}", format_decomposed(&sf.decompose(id)));
}

#[test]
fn test_next_id_in_sequence() {
    let start = gotime::now();
    let sf = new_sonyflake(Settings {
        time_unit: MILLISECOND,
        start_time: Some(start),
        ..Default::default()
    });
    let start_time = sf.to_internal_time(start);
    let machine_id = default_machine_id() as i64;

    let mut num_id: i32 = 0;
    let mut last_id: i64 = 0;
    let mut max_seq: i64 = 0;

    let mut current_time = start_time;
    while current_time - start_time < 100 {
        let id = next_id(&sf);
        current_time = sf.to_internal_time(gotime::now());
        num_id += 1;

        if id == last_id {
            panic!("duplicated id");
        }
        if id < last_id {
            panic!("must increase with time");
        }
        last_id = id;

        let parts = sf.decompose(id);

        let actual_time = parts["time"];
        let overtime = start_time + actual_time - current_time;
        if overtime > 0 {
            panic!("unexpected overtime: {overtime}");
        }

        let actual_sequence = parts["sequence"];
        if actual_sequence > max_seq {
            max_seq = actual_sequence;
        }

        let actual_machine = parts["machine"];
        if actual_machine != machine_id {
            panic!("unexpected machine: {actual_machine}");
        }
    }

    if max_seq > (1i64 << sf.bits_sequence()) - 1 {
        panic!("unexpected max sequence: {max_seq}");
    }
    println!("max sequence: {max_seq}");
    println!("number of id: {num_id}");
}

#[test]
fn test_next_id_in_parallel() {
    let sf1 = Arc::new(new_sonyflake(Settings {
        machine_id: Some(Box::new(|| Ok(1))),
        ..Default::default()
    }));
    let sf2 = Arc::new(new_sonyflake(Settings {
        machine_id: Some(Box::new(|| Ok(2))),
        ..Default::default()
    }));

    let num_cpu = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("number of cpu: {num_cpu}");

    // Go's `make(chan int64)` is unbuffered; a rendezvous channel matches it.
    let (tx, rx) = sync_channel::<i64>(0);

    const NUM_ID: usize = 1000;
    let generate = |sf: Arc<Sonyflake>, tx: std::sync::mpsc::SyncSender<i64>| {
        std::thread::spawn(move || {
            for _ in 0..NUM_ID {
                let id = next_id(&sf);
                if tx.send(id).is_err() {
                    return;
                }
            }
        })
    };

    let mut num_generator = 0usize;
    for _ in 0..num_cpu / 2 {
        generate(Arc::clone(&sf1), tx.clone());
        generate(Arc::clone(&sf2), tx.clone());
        num_generator += 2;
    }
    drop(tx);

    let mut set: HashSet<i64> = HashSet::new();
    for _ in 0..NUM_ID * num_generator {
        let id = rx.recv().expect("generator stopped early");
        if !set.insert(id) {
            panic!("duplicated id");
        }
    }
    println!("number of id: {}", set.len());
}

fn pseudo_sleep(sf: &Sonyflake, period: Duration) {
    let time_unit = sf.time_unit().nanoseconds();
    sf.sub_start_time(period.nanoseconds() / time_unit);
}

const YEAR: Duration = Duration(365 * 24 * HOUR.nanoseconds());

#[test]
fn test_next_id_returns_error() {
    let sf = new_sonyflake(Settings {
        start_time: Some(gotime::now()),
        ..Default::default()
    });

    pseudo_sleep(&sf, 174 * YEAR);
    next_id(&sf);

    pseudo_sleep(&sf, YEAR);
    let err = sf.next_id();
    if err.is_ok() {
        panic!("time is not over");
    }
}

// ---------------------------------------------------------------------------
// TestPrivateIPv4
// ---------------------------------------------------------------------------

/// One row of Go's `TestPrivateIPv4` table.
fn assert_private_ipv4(
    description: &str,
    interface_addrs: InterfaceAddrs,
    expected: Option<Ip>,
    error: Option<Error>,
) {
    let (actual, err) = match private_ipv4(&interface_addrs) {
        Ok(ip) => (Some(ip), None),
        Err(err) => (None, Some(err)),
    };

    assert!(err == error, "{description}: unexpected error: {err:?}");

    let equal = match (&actual, &expected) {
        (Some(a), Some(e)) => a.equal(e),
        (None, None) => true,
        _ => false,
    };
    if !equal {
        panic!(
            "{description}: unexpected ip: {}",
            gonet::ip_to_string(&actual)
        );
    }
}

#[test]
fn test_private_ipv4_returns_an_error() {
    assert_private_ipv4(
        "returns an error",
        mock::new_failing_interface_addrs(),
        None,
        Some(mock::ERR_FAILED_TO_GET_ADDRESSES.clone()),
    );
}

#[test]
fn test_private_ipv4_empty_address_list() {
    assert_private_ipv4(
        "empty address list",
        mock::new_nil_interface_addrs(),
        None,
        Some(Error::NoPrivateAddress),
    );
}

#[test]
fn test_private_ipv4_success() {
    assert_private_ipv4(
        "success",
        mock::new_successful_interface_addrs(),
        Some(Ip(vec![192, 168, 0, 1])),
        None,
    );
}

// ---------------------------------------------------------------------------
// TestLower16BitPrivateIP
// ---------------------------------------------------------------------------

/// One row of Go's `TestLower16BitPrivateIP` table.
fn assert_lower16_bit_private_ip(
    description: &str,
    interface_addrs: InterfaceAddrs,
    expected: i32,
    error: Option<Error>,
) {
    let (actual, err) = match lower16_bit_private_ip(&interface_addrs) {
        Ok(ip) => (ip, None),
        Err(err) => (0, Some(err)),
    };

    assert!(err == error, "{description}: unexpected error: {err:?}");

    if actual != expected {
        panic!("{description}: unexpected ip: {actual}");
    }
}

#[test]
fn test_lower16_bit_private_ip_returns_an_error() {
    assert_lower16_bit_private_ip(
        "returns an error",
        mock::new_failing_interface_addrs(),
        0,
        Some(mock::ERR_FAILED_TO_GET_ADDRESSES.clone()),
    );
}

#[test]
fn test_lower16_bit_private_ip_empty_address_list() {
    assert_lower16_bit_private_ip(
        "empty address list",
        mock::new_nil_interface_addrs(),
        0,
        Some(Error::NoPrivateAddress),
    );
}

#[test]
fn test_lower16_bit_private_ip_success() {
    assert_lower16_bit_private_ip("success", mock::new_successful_interface_addrs(), 1, None);
}

// ---------------------------------------------------------------------------
// TestToTime
// ---------------------------------------------------------------------------

#[test]
fn test_to_time() {
    let start = gotime::now();
    let sf = new_sonyflake(Settings {
        time_unit: MILLISECOND,
        start_time: Some(start),
        ..Default::default()
    });

    sf.set_now(Box::new(move || start));
    let id = next_id(&sf);

    let tm = sf.to_time(id);
    let diff = gotime::sub(tm, start);
    if diff < Duration(0) || diff >= sf.time_unit() {
        panic!("unexpected time: {diff}");
    }
}

// ---------------------------------------------------------------------------
// TestComposeAndDecompose
// ---------------------------------------------------------------------------

/// The fixture Go builds once above its `TestComposeAndDecompose` table.
fn compose_fixture() -> (Sonyflake, Time) {
    let now = gotime::now();
    let sf = new_sonyflake(Settings {
        time_unit: MILLISECOND,
        start_time: Some(now),
        ..Default::default()
    });
    (sf, now)
}

/// One row of Go's `TestComposeAndDecompose` table.
fn assert_compose_and_decompose(
    name: &str,
    sf: &Sonyflake,
    t: Time,
    sequence: i32,
    machine_id: i32,
) {
    let id = match sf.compose(t, sequence, machine_id) {
        Ok(id) => id,
        Err(err) => panic!("{name}: unexpected error: {err}"),
    };

    let parts = sf.decompose(id);

    // Verify time part
    let expected_time = sf.to_internal_time(t.to_utc()) - sf.start_time();
    if parts["time"] != expected_time {
        panic!(
            "{name}: time mismatch: got {}, want {}",
            parts["time"], expected_time
        );
    }

    // Verify sequence part
    if parts["sequence"] != sequence as i64 {
        panic!(
            "{name}: sequence mismatch: got {}, want {}",
            parts["sequence"], sequence
        );
    }

    // Verify machine id part
    if parts["machine"] != machine_id as i64 {
        panic!(
            "{name}: machine id mismatch: got {}, want {}",
            parts["machine"], machine_id
        );
    }

    // Verify id part
    if parts["id"] != id {
        panic!("{name}: id mismatch: got {}, want {}", parts["id"], id);
    }
}

#[test]
fn test_compose_and_decompose_zero_values() {
    let (sf, now) = compose_fixture();
    assert_compose_and_decompose("zero values", &sf, now, 0, 0);
}

#[test]
fn test_compose_and_decompose_max_sequence() {
    let (sf, now) = compose_fixture();
    let sequence = (1i32 << sf.bits_sequence()) - 1;
    assert_compose_and_decompose("max sequence", &sf, now, sequence, 0);
}

#[test]
fn test_compose_and_decompose_max_machine_id() {
    let (sf, now) = compose_fixture();
    let machine_id = (1i32 << sf.bits_machine()) - 1;
    assert_compose_and_decompose("max machine id", &sf, now, 0, machine_id);
}

#[test]
fn test_compose_and_decompose_future_time() {
    let (sf, now) = compose_fixture();
    assert_compose_and_decompose("future time", &sf, gotime::add(now, HOUR), 0, 0);
}

// ---------------------------------------------------------------------------
// TestCompose_ReturnsError
// ---------------------------------------------------------------------------

/// The fixture Go builds once above its `TestCompose_ReturnsError` table.
fn compose_error_fixture() -> (Sonyflake, Time) {
    let start = gotime::now();
    let sf = new_sonyflake(Settings {
        start_time: Some(start),
        ..Default::default()
    });
    (sf, start)
}

/// One row of Go's `TestCompose_ReturnsError` table.
fn assert_compose_error(
    name: &str,
    sf: &Sonyflake,
    t: Time,
    sequence: i32,
    machine_id: i32,
    want: Error,
) {
    let err = sf.compose(t, sequence, machine_id).err();
    if err != Some(want) {
        panic!("{name}: unexpected error: {err:?}");
    }
}

#[test]
fn test_compose_returns_error_start_time_ahead() {
    let (sf, start) = compose_error_fixture();
    assert_compose_error(
        "start time ahead",
        &sf,
        gotime::add(start, -gotime::SECOND),
        0,
        0,
        Error::StartTimeAhead,
    );
}

#[test]
fn test_compose_returns_error_over_time_limit() {
    let (sf, start) = compose_error_fixture();
    assert_compose_error(
        "over time limit",
        &sf,
        gotime::add(start, 175 * YEAR),
        0,
        0,
        Error::OverTimeLimit,
    );
}

#[test]
fn test_compose_returns_error_invalid_sequence() {
    let (sf, start) = compose_error_fixture();
    let sequence = 1 << sf.bits_sequence();
    assert_compose_error(
        "invalid sequence",
        &sf,
        start,
        sequence,
        0,
        Error::InvalidSequence,
    );
}

#[test]
fn test_compose_returns_error_invalid_machine_id() {
    let (sf, start) = compose_error_fixture();
    let machine_id = 1 << sf.bits_machine();
    assert_compose_error(
        "invalid machine id",
        &sf,
        start,
        0,
        machine_id,
        Error::InvalidMachineId,
    );
}
