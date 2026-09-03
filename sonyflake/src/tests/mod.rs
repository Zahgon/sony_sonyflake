//! Port of `sonyflake_test.go`.
//!
//! Go runs the tests of one package sequentially, and several of these tests share a
//! single package-level generator whose state one of them deliberately destroys
//! (`test_next_id_error`). Rust runs `#[test]` functions in parallel threads and in an
//! arbitrary order, so the four tests that share [`SHARED`] route their bodies through
//! [`run_shared_step`], which replays them in the order they appear in the Go file.
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
use std::sync::{LazyLock, Mutex};

use super::*;
use crate::gonet::{ip_to_string, Ip};
use crate::gotime::{Duration, HOUR, MILLISECOND, MINUTE};

/// The equivalent of the Go test file's package-level `sf`, `startTime` and
/// `machineID`, initialised by `init()`.
struct Shared {
    sf: Sonyflake,
    start_time: i64,
    machine_id: u64,
}

static SHARED: LazyLock<Shared> = LazyLock::new(|| {
    let start = gotime::now();
    let st = Settings {
        start_time: Some(start),
        ..Default::default()
    };

    let start_time = to_sonyflake_time(start);

    let sf = match Sonyflake::new_sonyflake(st) {
        Some(sf) => sf,
        None => panic!("sonyflake not created"),
    };

    let ip = lower16_bit_private_ip(&default_interface_addrs()).unwrap_or(0);
    let machine_id = ip as u64;

    Shared {
        sf,
        start_time,
        machine_id,
    }
});

// ---------------------------------------------------------------------------
// Ordering gate
// ---------------------------------------------------------------------------

/// Index of the next shared-generator step that still has to run.
static NEXT_STEP: Mutex<usize> = Mutex::new(0);

/// Runs the shared-generator steps in Go's source order, up to and including `step`.
///
/// A gate that simply *waited* for its turn would deadlock under
/// `--test-threads=1`, where the harness may well start with the last step and no
/// other thread is left to run the earlier ones. Running any outstanding
/// predecessors instead keeps the Go ordering under any thread count and any test
/// filter. A step is marked done before it runs, so a panic cannot replay it; the
/// panic still propagates and fails whichever test triggered the step.
fn run_shared_step(step: usize) {
    // Go runs `init()` before any test body, so `startTime` is fixed at process
    // start. Forcing the lazy value here rather than at first use keeps that
    // ordering: otherwise `sonyflake_once` would sleep first and only then pin the
    // start time, measuring an elapsed time of ~0.
    LazyLock::force(&SHARED);

    let mut next = NEXT_STEP.lock().unwrap_or_else(|err| err.into_inner());
    while *next <= step {
        let current = *next;
        *next += 1;
        match current {
            0 => sonyflake_once(),
            1 => sonyflake_for_10_sec(),
            2 => sonyflake_in_parallel(),
            3 => next_id_error(),
            _ => unreachable!("unknown shared step {current}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Go's `nextID(t)`; `t.Fatal` becomes a panic, which fails the test the same way.
fn next_id(sf: &Sonyflake) -> u64 {
    match sf.next_id() {
        Ok(id) => id,
        Err(_) => panic!("id not generated"),
    }
}

fn current_time() -> i64 {
    to_sonyflake_time(gotime::now())
}

fn pseudo_sleep(sf: &Sonyflake, period: Duration) {
    sf.sub_start_time(period.nanoseconds() / SONYFLAKE_TIME_UNIT);
}

// ---------------------------------------------------------------------------
// TestNew
// ---------------------------------------------------------------------------

/// One row of Go's `TestNew` table.
fn assert_new(name: &str, settings: Settings, want: Option<Error>) {
    let result = Sonyflake::new(settings);

    let err = result.as_ref().err().cloned();
    assert!(
        err == want,
        "{name}: unexpected value, want {want:?}, got {err:?}"
    );

    // `Result` makes the "nil instance with nil error" case unrepresentable, but
    // the assertion is kept so the ported test covers the same condition.
    assert!(
        !(result.is_err() && want.is_none()),
        "{name}: unexpected value, sonyflake should not be nil"
    );
}

#[test]
fn test_new_failure_time_ahead() {
    assert_new(
        "failure: time ahead",
        Settings {
            start_time: Some(gotime::add(gotime::now(), MINUTE)),
            ..Default::default()
        },
        Some(Error::StartTimeAhead),
    );
}

#[test]
fn test_new_failure_machine_id() {
    let gen_error = Error::message("an error occurred while generating ID");
    assert_new(
        "failure: machine ID",
        Settings {
            machine_id: Some({
                let gen_error = gen_error.clone();
                Box::new(move || Err(gen_error.clone()))
            }),
            ..Default::default()
        },
        Some(gen_error),
    );
}

#[test]
fn test_new_failure_invalid_machine_id() {
    assert_new(
        "failure: invalid machine ID",
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
// Shared-generator tests, in Go source order
// ---------------------------------------------------------------------------

#[test]
fn test_sonyflake_once() {
    run_shared_step(0);
}

fn sonyflake_once() {
    let sleep_time = Duration(50 * SONYFLAKE_TIME_UNIT);
    sleep_time.sleep();

    let id = next_id(&SHARED.sf);

    let actual_time = elapsed_time(id);
    if actual_time < sleep_time || actual_time > sleep_time + Duration(SONYFLAKE_TIME_UNIT) {
        panic!("unexpected time: {}", actual_time.nanoseconds());
    }

    let actual_sequence = sequence_number(id);
    if actual_sequence != 0 {
        panic!("unexpected sequence: {actual_sequence}");
    }

    let actual_machine_id = machine_id(id);
    if actual_machine_id != SHARED.machine_id {
        panic!("unexpected machine id: {actual_machine_id}");
    }

    println!("sonyflake id: {id}");
    println!("decompose: {}", format_decomposed(&decompose(id)));
}

#[test]
fn test_sonyflake_for_10_sec() {
    run_shared_step(1);
}

fn sonyflake_for_10_sec() {
    let mut num_id: u32 = 0;
    let mut last_id: u64 = 0;
    let mut max_sequence: u64 = 0;

    let initial = current_time();
    let mut current = initial;
    while current - initial < 1000 {
        let id = next_id(&SHARED.sf);
        let parts = decompose(id);
        num_id += 1;

        if id == last_id {
            panic!("duplicated id");
        }
        if id < last_id {
            panic!("must increase with time");
        }
        last_id = id;

        current = current_time();

        let actual_msb = parts["msb"];
        if actual_msb != 0 {
            panic!("unexpected msb: {actual_msb}");
        }

        let actual_time = parts["time"] as i64;
        let overtime = SHARED.start_time + actual_time - current;
        if overtime > 0 {
            panic!("unexpected overtime: {overtime}");
        }

        let actual_sequence = parts["sequence"];
        if max_sequence < actual_sequence {
            max_sequence = actual_sequence;
        }

        let actual_machine_id = parts["machine-id"];
        if actual_machine_id != SHARED.machine_id {
            panic!("unexpected machine id: {actual_machine_id}");
        }
    }

    if max_sequence != (1u64 << BIT_LEN_SEQUENCE) - 1 {
        panic!("unexpected max sequence: {max_sequence}");
    }
    println!("max sequence: {max_sequence}");
    println!("number of id: {num_id}");
}

#[test]
fn test_sonyflake_in_parallel() {
    run_shared_step(2);
}

fn sonyflake_in_parallel() {
    let num_cpu = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("number of cpu: {num_cpu}");

    // Go's `make(chan uint64)` is unbuffered; a rendezvous channel matches it.
    let (tx, rx) = sync_channel::<u64>(0);

    const NUM_ID: usize = 10000;
    const NUM_GENERATOR: usize = 10;
    for _ in 0..NUM_GENERATOR {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for _ in 0..NUM_ID {
                let id = next_id(&SHARED.sf);
                if tx.send(id).is_err() {
                    return;
                }
            }
        });
    }
    drop(tx);

    let mut set: HashSet<u64> = HashSet::new();
    for _ in 0..NUM_ID * NUM_GENERATOR {
        let id = rx.recv().expect("generator stopped early");
        if !set.insert(id) {
            panic!("duplicated id");
        }
    }
    println!("number of id: {}", set.len());
}

#[test]
fn test_next_id_error() {
    run_shared_step(3);
}

fn next_id_error() {
    let year = 365 * 24 * HOUR;
    pseudo_sleep(&SHARED.sf, 174 * year);
    next_id(&SHARED.sf);

    pseudo_sleep(&SHARED.sf, year);
    let err = SHARED.sf.next_id();
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
    error: &str,
) {
    let (actual, err) = match private_ipv4(&interface_addrs) {
        Ok(ip) => (Some(ip), None),
        Err(err) => (None, Some(err)),
    };

    if let Some(err) = err {
        if error.is_empty() {
            panic!("{description}: expected no error, but got: {err}");
        }
        return;
    }

    let equal = match (&actual, &expected) {
        (Some(a), Some(e)) => a.equal(e),
        (None, None) => true,
        _ => false,
    };
    if !equal {
        panic!(
            "{description}: error: expected: {}, but got: {}",
            ip_to_string(&expected),
            ip_to_string(&actual)
        );
    }
}

#[test]
fn test_private_ipv4_interface_addrs_returns_an_error() {
    assert_private_ipv4(
        "InterfaceAddrs returns an error",
        mock::new_failing_interface_addrs(),
        None,
        "test error",
    );
}

#[test]
fn test_private_ipv4_interface_addrs_returns_an_empty_or_nil_list() {
    assert_private_ipv4(
        "InterfaceAddrs returns an empty or nil list",
        mock::new_nil_interface_addrs(),
        None,
        "no private ip address",
    );
}

#[test]
fn test_private_ipv4_interface_addrs_returns_one_or_more_ips() {
    assert_private_ipv4(
        "InterfaceAddrs returns one or more IPs",
        mock::new_successful_interface_addrs(),
        Some(Ip(vec![192, 168, 0, 1])),
        "",
    );
}

// ---------------------------------------------------------------------------
// TestLower16BitPrivateIP
// ---------------------------------------------------------------------------

/// One row of Go's `TestLower16BitPrivateIP` table.
fn assert_lower16_bit_private_ip(
    description: &str,
    interface_addrs: InterfaceAddrs,
    expected: u16,
    error: &str,
) {
    let (actual, err) = match lower16_bit_private_ip(&interface_addrs) {
        Ok(ip) => (ip, None),
        Err(err) => (0, Some(err)),
    };

    if let Some(err) = err {
        if error.is_empty() {
            panic!("{description}: expected no error, but got: {err}");
        }
        return;
    }

    if actual != expected {
        panic!("{description}: error: expected: {expected}, but got: {actual}");
    }
}

#[test]
fn test_lower16_bit_private_ip_interface_addrs_returns_an_empty_or_nil_list() {
    assert_lower16_bit_private_ip(
        "InterfaceAddrs returns an empty or nil list",
        mock::new_nil_interface_addrs(),
        0,
        "no private ip address",
    );
}

#[test]
fn test_lower16_bit_private_ip_interface_addrs_returns_one_or_more_ips() {
    assert_lower16_bit_private_ip(
        "InterfaceAddrs returns one or more IPs",
        mock::new_successful_interface_addrs(),
        1,
        "",
    );
}

// ---------------------------------------------------------------------------
// Remaining top-level tests
// ---------------------------------------------------------------------------

#[test]
fn test_sonyflake_time_unit() {
    if Duration(SONYFLAKE_TIME_UNIT) != 10 * MILLISECOND {
        panic!("unexpected time unit");
    }
}

#[test]
fn test_compose() {
    let start_time = gotime::date(2023, 1, 1, 0, 0, 0, 0);
    let st = Settings {
        start_time: Some(start_time),
        ..Default::default()
    };
    let sf = match Sonyflake::new(st) {
        Ok(sf) => sf,
        Err(err) => panic!("{err}"),
    };

    let now = gotime::now();
    let sequence: u16 = 123;
    let machine_id: u16 = 456;

    let id = match compose(&sf, now, sequence, machine_id) {
        Ok(id) => id,
        Err(err) => panic!("{err}"),
    };

    let parts = decompose(id);

    let actual_time = to_sonyflake_time(now) - to_sonyflake_time(start_time);
    if parts["time"] != actual_time as u64 {
        panic!("unexpected time: {}", parts["time"]);
    }

    if parts["sequence"] != sequence as u64 {
        panic!("unexpected sequence: {}", parts["sequence"]);
    }

    if parts["machine-id"] != machine_id as u64 {
        panic!("unexpected machine id: {}", parts["machine-id"]);
    }
}

/// Not present in the Go suite: guards the hand-written port of Go's
/// `time.Duration.String`, which the ported assertions use for their messages.
#[test]
fn test_duration_display() {
    assert_eq!(Duration(0).to_string(), "0s");
    assert_eq!(Duration(1).to_string(), "1ns");
    assert_eq!(Duration(1_500).to_string(), "1.5µs");
    assert_eq!(Duration(1_500_000).to_string(), "1.5ms");
    assert_eq!((10 * MILLISECOND).to_string(), "10ms");
    assert_eq!((90 * gotime::SECOND).to_string(), "1m30s");
    assert_eq!((3 * HOUR + 25 * MINUTE).to_string(), "3h25m0s");
    assert_eq!((-(2 * gotime::SECOND)).to_string(), "-2s");
}
