# service

A centralized service for coin shuffling process

## Migrations

Install `sqlx-cli`:

```sh
cargo install sqlx-cli
```

Apply migrations:

```sh
sqlx migrate run
```

To generate new migrations:

```sh
sqlx migrate add <name>
```
