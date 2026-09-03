//! Port of `v2/example/sonyflake_server.go`.
//!
//! Serves one decomposed Sonyflake ID as JSON on every request.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::LazyLock;

use sonyflake_v2::{awsutil, Settings, Sonyflake};

static SF: LazyLock<Sonyflake> = LazyLock::new(|| {
    let st = Settings {
        machine_id: Some(Box::new(awsutil::amazon_ec2_machine_id)),
        ..Default::default()
    };

    match Sonyflake::new(st) {
        Ok(sf) => sf,
        Err(err) => panic!("{err}"),
    }
});

fn handler(stream: &mut TcpStream) -> std::io::Result<()> {
    // Consume the request head; the handler ignores its contents, as the Go original does.
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if line == "\r\n" || line == "\n" {
            break;
        }
        line.clear();
    }

    let id = match SF.next_id() {
        Ok(id) => id,
        Err(err) => return http_error(stream, 500, &err.to_string()),
    };

    let body = to_json(&SF.decompose(id));

    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Serialises the parts map the way `encoding/json` renders a `map[string]uint64`.
fn to_json(parts: &BTreeMap<String, i64>) -> String {
    let body = parts
        .iter()
        .map(|(k, v)| format!("\"{k}\":{v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

fn http_error(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} Internal Server Error\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}\n",
        message.len() + 1
    )
}

fn main() {
    LazyLock::force(&SF);

    let listener = TcpListener::bind("0.0.0.0:8080").expect("failed to listen on :8080");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                std::thread::spawn(move || {
                    let _ = handler(&mut stream);
                });
            }
            Err(err) => panic!("{err}"),
        }
    }
}
