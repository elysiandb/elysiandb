# ElysianDB — Lightweight KV Store with **Instant Zero-Config REST API**

[![Docker Pulls](https://img.shields.io/docker/pulls/taymour/elysiandb.svg)](https://hub.docker.com/r/taymour/elysiandb)
[![Tests](https://img.shields.io/github/actions/workflow/status/taymour/elysiandb/ci.yaml?branch=main\&label=tests)](https://github.com/taymour/elysiandb/actions/workflows/ci.yaml)
[![Coverage](https://codecov.io/gh/elysiandb/elysiandb/branch/main/graph/badge.svg)](https://codecov.io/gh/taymour/elysiandb)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**ElysianDB** is a blazing-fast, in-memory key–value store with a **zero-configuration, auto-generated REST API**. Written in Go and optimized with a **sharded arena allocator**, **zero-copy GET path**, and **cache-friendly JSON storage**, ElysianDB lets you spin up a full backend in seconds.

No schema files. No migrations. No ORMs. **Just start and query.**

---

## Highlights

* **Instant REST API** — CRUD, pagination, sorting, filtering, includes (`/api/<entity>`) *with zero configuration*
* **Fast In-Memory KV Engine** — sharded store, optional TTL, atomic counters
* **Auto-Indexing** — lazy index creation on first sort request
* **Schema-less JSON** — dynamic structures; IDs generated automatically
* **Optional Schema Validation** — infer schema from first POST, enforce afterwards
* **Nested Entity Creation** — auto-create sub-entities via `@entity` fields
* **Persistence** — periodic flush + crash recovery log
* **Protocols** — HTTP REST, TCP (Redis-style text protocol)
* **Built for Speed** — no GC churn thanks to arena allocation

---

## Quick Example

```js
// Create an entity
await fetch("http://localhost:8089/api/articles", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ title: "Hello", tags: ["go", "kv"], published: true }),
});

// Fetch with pagination & sorting
const res = await fetch("http://localhost:8089/api/articles?limit=20&offset=0&sort[title]=asc");
const articles = await res.json();
```

---

## Performance Benchmarks (MacBook Pro M4)

Run with **mixed CRUD, filtering, sorting, nested create, includes**, and heavy JSON workloads — not microbenchmarks.

### Summary

| Scenario       | Load                | p95 Latency | RPS      | Errors |
| -------------- | ------------------- | ----------- | -------- | ------ |
| **Dev Local**  | 3 VUs / 100 keys    | **62 µs**   | ~52.7k/s | 0%     |
| **Small App**  | 10 VUs / 500 keys   | **184 µs**  | ~81.3k/s | 0%     |
| **Light Prod** | 25 VUs / 1000 keys  | **412 µs**  | ~95.5k/s | 0%     |
| **Heavy Load** | 200 VUs / 5000 keys | **12.6 ms** | ~60.6k/s | 0%     |

### Why this matters

* **Sub-millisecond latency** up to 25 VUs
* **~60k requests/second** under heavy mixed load
* **Zero errors** across millions of requests
* **1.7 GB/s response throughput**, on a laptop

> **Bottom line:** ElysianDB outperforms most REST backends on mixed JSON workloads — while staying simpler than MongoDB.

---

## Run with Docker

```bash
docker run --rm -p 8089:8089 -p 8088:8088 taymour/elysiandb:latest
```

Default configuration:

```yaml
store:
  folder: /data
  shards: 512
  flushIntervalSeconds: 5
  crashRecovery: { enabled: true, maxLogMB: 100 }
  json:
    arenaChunkSize: 1048576  # 1 MiB
server:
  http: { enabled: true, host: 0.0.0.0, port: 8089 }
  tcp:  { enabled: true, host: 0.0.0.0, port: 8088 }
log:
  flushIntervalSeconds: 5
stats:
  enabled: false
api:
  index:
    workers: 4
  schema:
    enabled: true
  cache:
    enabled: true
    cleanupIntervalSeconds: 10
```

---

## Protocols

### **HTTP REST**

* `POST   /api/<entity>` → Create
* `GET    /api/<entity>` → List (pagination, filtering, sorting)
* `GET    /api/<entity>/<id>` → Read
* `PUT    /api/<entity>/<id>` → Update
* `DELETE /api/<entity>/<id>` → Delete
* `GET    /api/<entity>/schema` → Schema for entity (if config api.schema is enabled)

### **TCP (Redis-style)**

* `SET <key> <value>`
* `GET <key>` / `MGET key1 key2`
* `DEL <key>` / `RESET` / `SAVE` / `PING`

---

## Persistence & Stats

* Periodic flush to disk
* Crash-safe write-ahead log
* Graceful shutdown flush
* `/stats` endpoint for runtime metrics

---

## Build & Run

```bash
go build && ./elysiandb
# or
go run elysiandb.go
```

---

ElysianDB is built to be **simple**, **fast**, and **practical** — a real backend you can ship in minutes.
