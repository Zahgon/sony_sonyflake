#!/bin/sh
# The Go original cross-compiles with GOOS/GOARCH; cargo needs the target installed:
#   rustup target add x86_64-unknown-linux-gnu
set -e
cargo build --release --package sonyflake-v2 --example sonyflake_server_v2 --target x86_64-unknown-linux-gnu
cp ../../target/x86_64-unknown-linux-gnu/release/examples/sonyflake_server_v2 ./sonyflake_server_v2
