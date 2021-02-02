# Echo once

This example try to implement the echo demo listed in [go-libp2p-examples]()

## Run

In first terminal, record the listening port,

```shell
RUST_LOG=info cargo run --bin echo
```

In another terminal,

```shell
RUST_LOG=info cargo run --bin echo -- /ip4/127.0.0.1/tcp/<port>
```