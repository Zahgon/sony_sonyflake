#!/bin/sh
# The Go original cross-compiles with GOOS/GOARCH; cargo needs the target installed:
#   rustup target add x86_64-unknown-linux-gnu
set -e
cargo build --release --package sonyflake --example sonyflake_server --target x86_64-unknown-linux-gnu
cp ../../target/x86_64-unknown-linux-gnu/release/examples/sonyflake_server ./sonyflake_server
