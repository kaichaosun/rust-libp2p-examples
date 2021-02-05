# Echo a string

This example implements the echo demo listed in [go-libp2p-examples](https://github.com/libp2p/go-libp2p-examples/tree/master/echo).

## Run

In first terminal,

```shell
RUST_LOG=info cargo run --bin echo
```

It should prints with different PeerId and listening port,

```shell
[2021-02-05T07:02:18Z INFO  echo] Local peer id: PeerId("12D3KooWCABE9r2EbEB3jCee5qoFRPSK5fQYTZ9nGcocTLNJkqWY")
[2021-02-05T07:02:18Z INFO  echo] Listening on /ip4/127.0.0.1/tcp/51819
[2021-02-05T07:02:18Z INFO  echo] Listening on /ip4/192.168.31.115/tcp/51819
```

In another terminal,

```shell
RUST_LOG=info cargo run --bin echo -- /ip4/127.0.0.1/tcp/<port>
```

You should see more info in first terminal,

```shell
[2021-02-05T07:03:40Z INFO  echo] recv_echo, waiting for echo...
[2021-02-05T07:03:43Z INFO  echo] recv_echo, receive echo request for payload: [104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100, 33]
[2021-02-05T07:03:43Z INFO  echo] recv_echo, echo back successfully for payload: [104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100, 33]
[2021-02-05T07:03:43Z INFO  echo] Get event: EchoBehaviourEvent { peer: PeerId("12D3KooWQi1MwNXMZ6Ui1PUbSnG7dhjKUbfBJTB5JJ2Sdxykt4ct"), result: Success }
[2021-02-05T07:03:43Z INFO  echo] recv_echo, waiting for echo...
```

And the logs in secod terminal looks like,

```shell
[2021-02-05T07:03:40Z INFO  echo] Local peer id: PeerId("12D3KooWQi1MwNXMZ6Ui1PUbSnG7dhjKUbfBJTB5JJ2Sdxykt4ct")
[2021-02-05T07:03:40Z INFO  echo] Dialed /ip4/127.0.0.1/tcp/51819
[2021-02-05T07:03:40Z INFO  echo] Listening on /ip4/127.0.0.1/tcp/51837
[2021-02-05T07:03:40Z INFO  echo] Listening on /ip4/192.168.31.115/tcp/51837
[2021-02-05T07:03:43Z INFO  echo] send_echo, preparing send payload: "hello world!", in bytes: [104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100, 33]
[2021-02-05T07:03:43Z INFO  echo] send_echo, awaiting echo for "hello world!"
[2021-02-05T07:03:43Z INFO  echo] send_echo, received echo: Ok("hello world!")
[2021-02-05T07:03:43Z INFO  echo] Get event: EchoBehaviourEvent { peer: PeerId("12D3KooWCABE9r2EbEB3jCee5qoFRPSK5fQYTZ9nGcocTLNJkqWY"), result: Success }
```

Change the log level with `RUST_LOG=debug` to get more information during the execution.

