# service

A centralized service for coin shuffling process


## Running

Example of config:

```toml
[service]
address                = "127.0.0.1:8080"
min_room_size          = 3
shuffle_round_deadline = 60

[logger]
level = "INFO"

[contract]
url     = "https://goerli.blockpi.network/v1/rpc/public"
address = "0x4C0d116d9d028E60904DCA468b9Fa7537Ef8Cd5f"

[signer]
private_key = "<here enter your ECDSA private key>"

[tokens]
sign_key = "some-long-sign-key"
```

To run:

```bash
cargo run -- --config ./config.toml run
```
