# Intraprocess benchmark

To run, preallocate huge pages and then run as sudo.

From the root of the repo,

```
RUSTFLAGS="-C target-cpu=native" cargo build --release --bin benchmark
sudo ./target/release/benchmark
```