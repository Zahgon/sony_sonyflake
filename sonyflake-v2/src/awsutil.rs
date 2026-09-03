//! Utility functions for using Sonyflake on AWS.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;

use crate::error::{Error, MessageError};
use crate::gonet::Ip;
use crate::gotime::{Duration, MILLISECOND};

const EC2_METADATA_HOST: &str = "169.254.169.254:80";
const EC2_METADATA_PATH: &str = "/latest/meta-data/local-ipv4";

fn amazon_ec2_private_ipv4() -> Result<Option<Ip>, Error> {
    let body = http_get(EC2_METADATA_HOST, EC2_METADATA_PATH)?;

    let ip = match Ip::parse(&body) {
        Some(ip) => ip,
        None => return Err(Error::message("invalid ip address")),
    };
    Ok(ip.to4())
}

/// Retrieves the private IP address of the Amazon EC2 instance and returns its
/// lower 16 bits. It works correctly on Docker as well.
pub fn amazon_ec2_machine_id() -> Result<i32, Error> {
    let ip = amazon_ec2_private_ipv4()?;
    // Go indexes the result of To4 directly, so a non-IPv4 answer is a hard failure.
    let ip = match ip {
        Some(ip) => ip,
        None => return Err(Error::message("invalid ip address")),
    };
    let ip = ip.as_bytes();

    Ok(((ip[2] as i32) << 8) + ip[3] as i32)
}

/// Returns the time difference between the localhost and the given NTP server.
pub fn time_difference(server: &str) -> Result<Duration, Error> {
    let output = Command::new("/usr/sbin/ntpdate")
        .arg("-q")
        .arg(server)
        .output()
        .map_err(Error::other)?;
    if !output.status.success() {
        return Err(Error::message(format!(
            "exit status {}",
            output.status.code().unwrap_or(-1)
        )));
    }
    // Go uses CombinedOutput, which interleaves stdout and stderr.
    let mut combined = output.stdout;
    combined.extend_from_slice(&output.stderr);
    let combined = String::from_utf8_lossy(&combined).into_owned();

    let submatched = match find_offset_seconds(&combined) {
        Some(s) => s,
        None => return Err(Error::message("invalid ntpdate output")),
    };

    let f: f64 = submatched.parse().map_err(|_| {
        Error::message(format!(
            "strconv.ParseFloat: parsing {submatched:?}: invalid syntax"
        ))
    })?;
    Ok((f * 1000.0) as i64 * MILLISECOND)
}

/// Equivalent of Go's `regexp.MustCompile("offset (.*) sec").FindSubmatch`.
///
/// RE2's `.` does not match a newline and `(.*)` is greedy, so the match is the
/// longest run within a single line between `offset ` and a following ` sec`.
fn find_offset_seconds(output: &str) -> Option<&str> {
    for line in output.split('\n') {
        let Some(start) = line.find("offset ") else {
            continue;
        };
        let rest = &line[start + "offset ".len()..];
        let end = rest.rfind(" sec")?;
        return Some(&rest[..end]);
    }
    None
}

/// A minimal HTTP/1.1 GET, standing in for Go's `http.Get`.
///
/// The only endpoint this is used against is the EC2 instance metadata service,
/// which answers a plain body over cleartext HTTP.
fn http_get(host: &str, path: &str) -> Result<String, Error> {
    let mut stream = TcpStream::connect(host).map_err(Error::other)?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: sonyflake\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).map_err(Error::other)?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(Error::other)?;

    let separator = b"\r\n\r\n";
    let body_start = response
        .windows(separator.len())
        .position(|w| w == separator)
        .map(|i| i + separator.len())
        .ok_or_else(|| Error::other(MessageError("malformed HTTP response".into())))?;

    Ok(String::from_utf8_lossy(&response[body_start..])
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Go original delegates this to `regexp`; the hand-written matcher needs
    /// its own coverage of the greedy, single-line semantics it reproduces.
    #[test]
    fn test_find_offset_seconds() {
        assert_eq!(
            find_offset_seconds("server 1.2.3.4, stratum 2, offset -0.012345 sec\n"),
            Some("-0.012345")
        );
        assert_eq!(
            find_offset_seconds("offset 1 sec and 2 sec"),
            Some("1 sec and 2")
        );
        assert_eq!(find_offset_seconds("offset 1\nsec"), None);
        assert_eq!(find_offset_seconds("no match here"), None);
    }
}
