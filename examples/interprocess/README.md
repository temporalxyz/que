# Intraprocess example

Producer process sends value to a consumer process. The producer will wait until the consumer joins to publish a value, and then wait for the consumer to ack the message before cleaning up.

To run, preallocate huge pages and then run the producer and consumer processes as sudo.

From the root of the repo,

```
RUSTFLAGS="-C target-cpu=native" cargo build --release --bin producer
RUSTFLAGS="-C target-cpu=native" cargo build --release --bin consumer
```

**The producer process must be run first in order to initialize the SPSC,** 

In terminal 1 
```
sudo ./target/release/producer
```

In terminal 2
```
sudo ./target/release/producer
```