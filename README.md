# Rust Key-Value-Database

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Redis Protocol](https://img.shields.io/badge/Protocol-RESP-2B2B2B.svg)](https://redis.io/docs/latest/develop/reference/protocol-spec/)
[![GitHub stars](https://img.shields.io/github/stars/butaraul/Rust-KVdb.svg?style=social)](https://github.com/butaraul/Rust-KVdb/stargazers)


A high-performance in-memory key-value database, written from scratch in Rust,
with a live React dashboard.

- **Storage engine**: a B-Tree implemented from scratch (no tree crates) over a
  custom first-fit, coalescing arena allocator backed by a single `Vec<u8>`.
- **Persistence**: write-ahead log (`appendonly.aof`) + periodic snapshotting
  (`dump.rdb`), with crash-consistent recovery on restart.
- **Network layer**: a non-blocking RESP (Redis protocol) server built directly
  on `mio` (epoll/kqueue) — no async runtime on the hot path.
- **Dashboard**: an `axum` HTTP server exposing a `/ws` live-metrics feed, a
  Prometheus `/metrics` endpoint, and a dark-mode React + Vite + recharts
  frontend, embedded straight into the server binary.

```
                    ┌─────────────┐
   RESP clients ───▶│  mio event  │───┐
   (redis-cli,      │    loop     │   │
    custom apps)    └─────────────┘   │
                                       ▼
                              ┌─────────────────┐        ┌───────────────┐
                              │  persistence::   │───────▶│ storage::BTree │
                              │     Engine        │       │  (arena +      │
                              │ (WAL + snapshot)   │◀──────│   node pool)   │
                              └─────────────────┘        └───────────────┘
                                       ▲
                                       │
   Browser ───▶ /ws, /metrics, / ───▶ axum HTTP server (crates/web)
```

## Quickstart

```bash
# 1. Build the dashboard (only needed once, or after changing dashboard/src)
cd dashboard && npm install && npm run build && cd ..

# 2. Build and run the server
cargo run --release -p server -- \
  --data-dir ./data \
  --resp-addr 127.0.0.1:6380 \
  --http-addr 127.0.0.1:8080

# 3. Talk to it over RESP
redis-cli -p 6380 SET foo bar
redis-cli -p 6380 GET foo
redis-cli -p 6380 KEYS '*'

# 4. Open the dashboard
open http://127.0.0.1:8080/
```

The dashboard must be built (`npm run build`) *before* `cargo build`, since the
`web` crate embeds `dashboard/dist` into the binary at compile time via
`rust-embed`.

### CLI flags

| Flag | Default | Description |
|---|---|---|
| `--data-dir` | `./data` | Directory holding `appendonly.aof` and `dump.rdb` |
| `--resp-addr` | `127.0.0.1:6380` | RESP protocol listener |
| `--http-addr` | `127.0.0.1:8080` | HTTP dashboard / `/metrics` / `/ws` listener |
| `--snapshot-interval-secs` | `60` | Seconds between automatic snapshots |

## Supported commands

`SET key value` · `GET key` · `DEL key` · `KEYS pattern` (glob: `*`, `?`,
`[abc]`, `[^abc]`, `[a-z]`) · `DBSIZE` · `PING`

## Project layout

```
crates/
  storage/       from-scratch B-Tree + arena allocator, no external tree crates
  persistence/   write-ahead log + snapshotting
  protocol/      zero-copy RESP parser/encoder
  server/        mio event loop + CLI entrypoint (kvdb-server binary)
  web/           axum dashboard server, WebSocket metrics broadcast, Prometheus
dashboard/       React + TypeScript + Vite frontend (recharts, dark mode)
```

## How persistence stays crash-consistent

Every write acquires the WAL lock *before* appending, and holds it across both
the WAL append and the B-Tree apply. The snapshot loop acquires the same lock
before reading the tree and truncating the WAL. Because lock acquisition order
is always WAL-then-store on every path, by the time a snapshot starts, every
write that was logged has also already been applied — and no write can begin
applying while a snapshot is in progress. That's what makes "read the tree,
write `dump.rdb`, truncate the WAL" safe without losing or double-applying a
write. On startup, `dump.rdb` is loaded first, then the (now-empty-or-partial)
WAL is replayed on top of it.

## Testing

```bash
cargo test --workspace       # unit + integration tests, incl. a 20k-op
                              # randomized B-Tree fuzz test checked against
                              # std::collections::BTreeMap
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p storage        # criterion benchmarks for SET/GET
```

## Dashboard development

```bash
cd dashboard
npm install
npm run dev      # Vite dev server with proxying to a running kvdb-server on :8080
```
