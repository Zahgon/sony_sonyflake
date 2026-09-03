# Sonyflake

A Rust port of [github.com/sony/sonyflake](https://github.com/sony/sonyflake), a
distributed unique ID generator inspired by
[Twitter's Snowflake](https://blog.twitter.com/2010/announcing-snowflake).

Sonyflake focuses on lifetime and performance on many host/core environment.
So it has a different bit assignment from Snowflake.
By default, a Sonyflake ID is composed of

    39 bits for time in units of 10 msec
     8 bits for a sequence number
    16 bits for a machine id

As a result, Sonyflake has the following advantages and disadvantages:

- The lifetime (174 years) is longer than that of Snowflake (69 years)
- It can work in more distributed machines (2^16) than Snowflake (2^10)
- It can generate 2^8 IDs per 10 msec at most in a single instance (fewer than Snowflake)

However, if you want more generation rate in a single host,
you can easily run multiple Sonyflake instances parallelly using threads.

In addition, you can adjust the lifetime and generation rate of Sonyflake
by customizing the bit assignment and the time unit.

## Layout

The Go repository ships two independent modules, and this workspace keeps them
separate in the same way:

| Go module | Rust crate | Library name |
| --- | --- | --- |
| `github.com/sony/sonyflake` | [`sonyflake/`](sonyflake) | `sonyflake` |
| `github.com/sony/sonyflake/v2` | [`sonyflake-v2/`](sonyflake-v2) | `sonyflake_v2` |

The two crates do not depend on each other, so the small `gotime` and `gonet`
support modules are duplicated exactly as the Go source is.

## Installation

```toml
[dependencies]
sonyflake-v2 = { path = "sonyflake-v2" }
```

## Usage

`Sonyflake::new` creates a new Sonyflake instance.

```rust
pub fn new(st: Settings) -> Result<Sonyflake, Error>
```

You can configure Sonyflake by the struct `Settings`:

```rust
pub struct Settings {
    pub bits_sequence: i32,
    pub bits_machine_id: i32,
    pub time_unit: Duration,
    pub start_time: Option<Time>,
    pub machine_id: Option<Box<dyn Fn() -> Result<i32, Error>>>,
    pub check_machine_id: Option<Box<dyn Fn(i32) -> bool>>,
}
```

- `bits_sequence` is the bit length of a sequence number.
  If `bits_sequence` is 0, the default bit length is used, which is 8.
  If `bits_sequence` is 31 or more, an error is returned.

- `bits_machine_id` is the bit length of a machine ID.
  If `bits_machine_id` is 0, the default bit length is used, which is 16.
  If `bits_machine_id` is 31 or more, an error is returned.

- `time_unit` is the time unit of Sonyflake.
  If `time_unit` is 0, the default time unit is used, which is 10 msec.
  `time_unit` must be 1 msec or longer.

- `start_time` is the time since which the Sonyflake time is defined as the elapsed time.
  If `start_time` is `None`, the start time of the Sonyflake instance is set to "2025-01-01 00:00:00 +0000 UTC".
  `start_time` must be before the current time.

- `machine_id` returns the unique ID of a Sonyflake instance.
  If `machine_id` returns an error, the instance will not be created.
  If `machine_id` is `None`, the default `machine_id` is used, which returns the lower 16 bits of the private IP address.

- `check_machine_id` validates the uniqueness of a machine ID.
  If `check_machine_id` returns false, the instance will not be created.
  If `check_machine_id` is `None`, no validation is done.

Go signals "unset" with a zero value; the port uses `Option::None`, and
`Settings::default()` is the equivalent of Go's `Settings{}`.

The bit length of time is calculated by `63 - bits_sequence - bits_machine_id`.
If it is less than 32, an error is returned.

In order to get a new unique ID, you just have to call the method `next_id`.

```rust
pub fn next_id(&self) -> Result<i64, Error>
```

`next_id` can continue to generate IDs for about 174 years from `start_time` by
default. But after the Sonyflake time is over the limit, `next_id` returns an error.

`next_id` takes `&self` and locks internally, so one instance can be shared across
threads through an `Arc` without any external synchronisation.

```rust
use std::sync::Arc;
use sonyflake_v2::{Settings, Sonyflake};

let sf = Arc::new(Sonyflake::new(Settings::default())?);
let id = sf.next_id()?;
println!("{:?}", sf.decompose(id));
```

## AWS VPC and Docker

The [`awsutil`](sonyflake-v2/src/awsutil.rs) module provides the function
`amazon_ec2_machine_id` that returns the lower 16-bit private IP address of the
Amazon EC2 instance. It also works correctly on Docker by retrieving
[instance metadata](http://docs.aws.amazon.com/en_us/AWSEC2/latest/UserGuide/ec2-instance-metadata.html).

[AWS IPv4 VPC](https://docs.aws.amazon.com/vpc/latest/userguide/vpc-cidr-blocks.html)
is usually assigned a single CIDR with a netmask between /28 and /16.
So if each EC2 instance has a unique private IP address in AWS VPC,
the lower 16 bits of the address is also unique.
In this common case, you can use `amazon_ec2_machine_id` as `Settings::machine_id`.

See [example](sonyflake-v2/example) that runs Sonyflake on AWS Elastic Beanstalk.

## Building and testing

```bash
cargo build --all-targets
cargo test            # ~15s: the v1 suite reproduces Go's 10-second generation test
cargo clippy --all-targets
cargo fmt --all --check
```

## Migration notes

See [MIGRATION.md](MIGRATION.md) for how each Go construct maps onto Rust and how
behavioural parity with the Go implementation was verified.

## License

The MIT License (MIT)

See [LICENSE](LICENSE) for details.
