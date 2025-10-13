# ElysianDB — Lightweight KV Store with **Zero‑Config Auto‑Generated REST API**

<p align="left">
  <img src="docs/logo.png" alt="ElysianDB Logo" width="200"/>
</p>

[![Docker Pulls](https://img.shields.io/docker/pulls/taymour/elysiandb.svg)](https://hub.docker.com/r/taymour/elysiandb)
[![Tests](https://img.shields.io/github/actions/workflow/status/taymour/elysiandb/ci.yaml?branch=main&label=tests)](https://github.com/taymour/elysiandb/actions/workflows/ci.yaml)
[![Coverage](https://codecov.io/gh/elysiandb/elysiandb/branch/main/graph/badge.svg)](https://codecov.io/gh/taymour/elysiandb)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**ElysianDB** is a lightweight, fast key–value store written in Go. It speaks both **HTTP** and **TCP**:

* a minimal Redis‑style **text protocol** over TCP for max performance,
* a simple **KV HTTP API**, and now
* a **zero‑configuration, auto‑generated REST API** that lets you treat ElysianDB like an **instant backend** for your frontend.

> **One‑liner:** You get an **auto‑generated REST API** (CRUD, pagination, sort) **with no configuration**; **entities are inferred from the URL**, and **indexes** are created automatically.

See [CONTRIBUTING.md](CONTRIBUTING.md) if you’d like to help.
Here is the [documentation](https://github.com/elysiandb/elysiandb/blob/main/docs/index.md)

---

## Highlights

* **Zero‑Config REST API** — auto-generated CRUD endpoints per entity (`/api/<entity>`)
* **Fast KV Engine** — in-memory sharded store with optional TTL and on-disk persistence
* **Multi‑Protocol** — HTTP, TCP (Redis-style text protocol), and Instant REST
* **Automatic Indexing** — lazy-built indexes on first sort request
* **Schema‑less JSON** — store any structure; IDs generated automatically
* **Persistence** — automatic periodic flush and graceful shutdown

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

No setup, no schema — just start and query.

---

## Performance

| Scenario   | Load               | p95 Latency | RPS    | Errors |
| ---------- | ------------------ | ----------- | ------ | ------ |
| Dev Local  | 3 VUs / 100 keys   | **0.23 ms** | ~15k/s | 0%     |
| Small App  | 10 VUs / 500 keys  | **0.37 ms** | ~40k/s | 0%     |
| Light Prod | 25 VUs / 1000 keys | **1.35 ms** | ~41k/s | 0%     |

> Sub‑millisecond latency under realistic workloads — true instant REST APIs.

---

## Run with Docker

```bash
docker run --rm -p 8089:8089 -p 8088:8088 taymour/elysiandb:latest
```

Default config:

```yaml
store:
  folder: /data
  shards: 512
  flushIntervalSeconds: 5
  crashRecovery: { enabled: true, maxLogMB: 100 }
server:
  http: { enabled: true,  host: 0.0.0.0, port: 8089 }
  tcp:  { enabled: true,  host: 0.0.0.0, port: 8088 }
log:
  flushIntervalSeconds: 5
stats:
  enabled: false
api:
  index:
    workers: 4
  cache:
    enabled: true
    cleanupIntervalSeconds: 10

```

---

## Protocols

### HTTP REST

* `POST   /api/<entity>` → Create
* `GET    /api/<entity>` → List (`limit`, `offset`, `sort[field]=asc|desc`, `filter`)
* `GET    /api/<entity>/<id>` → Read
* `PUT    /api/<entity>/<id>` → Update
* `DELETE /api/<entity>/<id>` → Delete

### TCP (Redis‑style)

* `SET <key> <value>`
* `GET <key>` / `MGET key1 key2`
* `DEL <key>` / `RESET` / `SAVE` / `PING`

---

## Stats & Persistence

* Automatic periodic persistence to disk
* Logged writes and crash replay
* Graceful flush on shutdown
* `/stats` endpoint for runtime metrics (if enabled)

---

## Build & Run

```bash
go build && ./elysiandb
# or
go run elysiandb.go
```
