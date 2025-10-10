<p align="left">
  <img src="docs/logo.png" alt="ElysianDB Logo" width="200"/>
</p>

[![Docker Pulls](https://img.shields.io/docker/pulls/taymour/elysiandb.svg)](https://hub.docker.com/r/taymour/elysiandb)
[![Tests](https://img.shields.io/github/actions/workflow/status/taymour/elysiandb/ci.yaml?branch=main&label=tests)](https://github.com/taymour/elysiandb/actions/workflows/ci.yaml)
[![Coverage](https://codecov.io/gh/elysiandb/elysiandb/branch/main/graph/badge.svg)](https://codecov.io/gh/taymour/elysiandb)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

# ElysianDB — Lightweight KV Store with **Zero‑Config Auto‑Generated REST API**

**ElysianDB** is a lightweight, fast key–value store written in Go. It speaks both **HTTP** and **TCP**:

* a minimal Redis‑style **text protocol** over TCP for max performance,
* a simple **KV HTTP API**, and now
* a **zero‑configuration, auto‑generated REST API** that lets you treat ElysianDB like an **instant backend** for your frontend.

> **One‑liner:** You get an **auto‑generated REST API** (CRUD, pagination, sort) **with no configuration**; **entities are inferred from the URL**, and **indexes** are created automatically on first sort or managed manually.

See [CONTRIBUTING.md](CONTRIBUTING.md) if you’d like to help.

---

**Frontend example, just start ElysianDB and you can execute this code out of the box without any configuration or schema upload :**

```
// Create an article
await fetch("http://localhost:8089/api/articles", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ title: "Hello", tags: ["go", "kv"], published: true }),
});


// Fetch articles
const res = await fetch("http://localhost:8089/api/articles?limit=20&offset=0&sort[title]=asc");
const articles = await res.json();
```
> More features to see below. This is a basic version but more features are coming.

---

## ElysianDB Performance Summary

**Instant REST API Benchmarks — Small Web Project Scenarios**

ElysianDB demonstrates super real-time performance under realistic small-project workloads, maintaining sub-millisecond latency even under concurrent access.

---

### Benchmark Scenarios

| Scenario             | Command                                                        | Description                                                  |
| -------------------- | -------------------------------------------------------------- | ------------------------------------------------------------ |
| **Dev Local**        | `BASE_URL=http://localhost:8089 KEYS=100 VUS=3 DURATION=10s`   | Light local usage — simulate developer / staging environment |
| **Small Web App**    | `BASE_URL=http://localhost:8089 KEYS=500 VUS=10 DURATION=20s`  | Typical small production web app or SaaS dashboard           |
| **Light Production** | `BASE_URL=http://localhost:8089 KEYS=1000 VUS=25 DURATION=30s` | Simulates modest real-world traffic with concurrent users    |

---

### Results Overview

| Scenario             | Load               | p95 (Global) | RPS    | HTTP Errors | Notes                          |
| -------------------- | ------------------ | ------------ | ------ | ----------- | ------------------------------ |
| **Dev Local**        | 3 VUs / 100 keys   | **0.23 ms**  | ~15k/s | 0 %         | Practically instantaneous      |
| **Small Web App**    | 10 VUs / 500 keys  | **0.37 ms**  | ~40k/s | 0 %         | Ultra-fluid API performance    |
| **Light Production** | 25 VUs / 1000 keys | **1.35 ms**  | ~41k/s | 0 %         | Stable at moderate concurrency |

---

### Key Takeaways

* **Sub-millisecond latency** for 95% of requests up to 10 concurrent users
* **Consistent <2 ms latency** under 25 concurrent users (hundreds of real users equivalent)
* **Zero errors** across all runs
* **Full coverage**: filtering, sorting, nested operations, and updates all included
* **Lazy indexing** remains transparent and cost-free under real-world usage

---

### Conclusion

> **ElysianDB delivers true instant REST APIs — consistently under 2 ms at web-scale concurrency, with zero configuration and automatic indexing.**

## Highlights

* **Fast in‑memory store** with shard routing (xxhash), optional TTL.
* **Persistence** via on‑disk JSON snapshots (periodic + graceful shutdown + manual save).
* **Three interfaces**: TCP text protocol, KV HTTP endpoints, and **Zero‑Config REST Entities**.
* **Schema‑less JSON**: store any JSON per entity; IDs generated automatically.
* **Indexing**:
  * **automatic, on‑demand** (first sort builds the index),
  * or **manual** add/remove per field.
* **Listing features**: `limit`, `offset`, and `sort[field]=asc|desc`.
* **CI + tests** (unit + e2e) and coverage reports.
* **Docker‑first**—one small image, sane defaults.

---

## Requirements

* [Go](https://go.dev/) 1.23+
* Optional benchmarking:

  * **TCP**: built‑in `elysian_bench` load generator
  * **HTTP**: [k6](https://k6.io/)

---

## Configuration

Server settings are defined in `elysian.yaml`. You can enable **HTTP**, **TCP**, or **both**, and control the number of in‑memory **shards**.

```yaml
store:
  folder: /data
  shards: 512                  # power of two recommended
  flushIntervalSeconds: 5      # periodic on-disk flush interval (seconds)
server:
  http: { enabled: true, host: 0.0.0.0, port: 8089 }
  tcp:  { enabled: true, host: 0.0.0.0, port: 8088 }
log:
  flushIntervalSeconds: 5      # periodic log flush interval (seconds)
stats:
  enabled: true # enable runtime counters & /stats endpoint data
```

**Keys**

* `store.folder` – Path where data files are stored (must be writable).
* `store.shards` – Number of shards for the in‑memory store. **Must be ≥1** and ideally a **power of two** (e.g. 128/256/512).
* `store.flushIntervalSeconds` – Interval, in seconds, between periodic persistence to disk.
* `server.http.*` – HTTP listener configuration (`enabled`, `host`, `port`).
* `server.tcp.*` – TCP listener configuration (`enabled`, `host`, `port`).
* `log.flushIntervalSeconds` – Interval, in seconds, between periodic log writes/flushes.
* `stats.enabled` – When true, all request/hit/miss/key counters are updated at runtime and exposed at /stats (HTTP). Needs to have server.http.enabled = true.

> To run a single protocol, set the other listener to `enabled: false`.

---

## Building and Running

```bash
# build executable
go build

# run directly with your local elysian.yaml
./elysiandb
# or
go run elysiandb.go
```

### Health

* HTTP: `GET /health` → `200 OK`
* TCP: send `PING` → `PONG` (when TCP is enabled)

---

## Docker

Prebuilt images are available on Docker Hub:

```bash
# pull the latest
docker pull taymour/elysiandb:latest
```

## Persistence & Shutdown Behavior

ElysianDB persists in‑memory data to disk in the following cases:

1. **Periodic flush** — Controlled by `store.flushIntervalSeconds` (see **Configuration**). Data is snapshotted to disk at the configured interval.
2. **Manual save** —

   * **HTTP**: `POST /save`
   * **TCP**: `SAVE`
     Forces an immediate snapshot to disk.
3. **Graceful shutdown** — On **SIGTERM** or **SIGINT** (e.g., `docker stop`, Ctrl+C), ElysianDB flushes current data to disk before exiting and logs a shutdown message.

> **Note:** **SIGKILL (9)** cannot be intercepted on Unix-like systems; if the process is killed with SIGKILL, no shutdown hook runs and a final flush cannot be guaranteed.

### Quick verification

```bash
# Run locally and write some data, then trigger a graceful stop:
kill -TERM <elysiandb_pid>
# or Ctrl+C in the foreground process
# After restart, verify your data is present.
```

### Quick start (ephemeral)

```bash
docker run --rm \
  -p 8089:8089 \  # HTTP
  -p 8088:8088 \  # TCP
  taymour/elysiandb:latest
```

### With persistence

```bash
docker run -d --name elysiandb \
  -p 8089:8089 \
  -p 8088:8088 \
  -v elysian-data:/data \
  taymour/elysiandb:latest
```

The image includes a default config at `/app/elysian.yaml`:

```yaml
store:
  folder: /data
  shards: 512
  flushIntervalSeconds: 5
server:
  http: { enabled: true,  host: 0.0.0.0, port: 8089 }
  tcp:  { enabled: true,  host: 0.0.0.0, port: 8088 }
log:
  flushIntervalSeconds: 5
stats:
  enabled: true
```

To override the config, mount your own file:

```bash
docker run -d --name elysiandb \
  -p 8089:8089 -p 8088:8088 \
  -v elysian-data:/data \
  -v $(pwd)/elysian.yaml:/app/elysian.yaml:ro \
  taymour/elysiandb:latest
```

---

## Protocols

### Instant REST API (zero config)

**Create** an article, **list** with pagination, **fetch** by id, **update**, **delete**:

```bash
# Create (entity is inferred from /api/<entity>)
curl -s -X POST http://localhost:8089/api/articles \
  -H 'Content-Type: application/json' \
  -d '{"title":"Hello","tags":["go","kv"],"published":true}'

# List with offset/limit and sort by title ascending
curl -s "http://localhost:8089/api/articles?limit=20&offset=0&sort[title]=asc"

# Read one (replace <id> with the value returned by POST)
curl -s http://localhost:8089/api/articles/<id>

# Update
curl -s -X PUT http://localhost:8089/api/articles/<id> \
  -H 'Content-Type: application/json' \
  -d '{"title":"Hello v2","published":true}'

# Delete one
curl -s -X DELETE http://localhost:8089/api/articles/<id> -i

# Destroy all records for an entity (dangerous)
curl -s -X DELETE http://localhost:8089/api/articles -i
```

**Index management (optional):**

```bash
# Manually create an index for field "created_at"
curl -X POST http://localhost:8089/api/articles/created_at/index -i

# Remove that index
curl -X DELETE http://localhost:8089/api/articles/created_at/index -i
```

> You usually **don’t need** to create indexes yourself: the first time you request `sort[field]`, ElysianDB will **build the index automatically** and reuse it next time.

---

## Zero‑Config REST Entities — How It Works

* **Entity inference** from the URL: `/api/<entity>` → entity name is `<entity>` (e.g., `articles`, `users`, `orders`, `weird-entity_name42`).
* **Schema‑less JSON** per entity. ElysianDB stores documents “as is”.
* **IDs** are generated as UUIDs on `POST /api/<entity>` and returned in the response.
* **CRUD endpoints**:

  * `POST   /api/<entity>` → create JSON document (auto‑ID)
  * `GET    /api/<entity>` → list with `limit`, `offset`, `sort[field]=asc|desc`
  * `GET    /api/<entity>/<id>` → fetch by id
  * `PUT    /api/<entity>/<id>` → merge/update fields
  * `DELETE /api/<entity>/<id>` → delete by id
  * `DELETE /api/<entity>` → delete **all** documents of that entity
* **Sorting & Indexing**:

  * Query like `?sort[score]=desc` builds **(or refreshes)** the index for `score` if missing.
  * Indexes can also be **managed explicitly** per field (see endpoints above).
* **Pagination**: `limit` and `offset` are integers; defaults are `0` (no limit) and `0`.

This makes ElysianDB a drop‑in **instant backend API** for POCs, internal tools, and simple frontends.

### TCP text protocol ("Redis style")

A very small, line‑based text protocol. Each command is a line terminated by `\n`. Whitespace separates tokens.

**Supported commands (core):**
* `GET <key>` → returns raw value bytes; if missing, returns an empty payload or a not‑found marker
* `MGET <key1> <key2> ...` → fetches values for multiple keys in a single request
* `SET <key> <value>` → stores value; optional `TTL=<seconds>` support via `SET TTL=10 <key> <value>`
* `DEL <key>` → deletes key
* `SAVE` → persist db to disk
* `RESET` → resets all db keys
* `PING` → health command, returns `PONG`

**Examples (telnet):**

```bash
# connect
telnet 127.0.0.1 8088

# set a value
SET foo bar

# get it
GET foo

# get multiple values
MGET foo bar baz

# set with ttl=10s
SET TTL=10 session:123 abc

# delete
DEL foo
```

> The protocol is intentionally simple for benchmarking and learning purposes.

### HTTP API

| Method | Path                           | Description                                                                                         |
| ------ | ------------------------------ | --------------------------------------------------------------------------------------------------- |
| GET    | `/health`                      | Liveness probe                                                                                      |
| MGET   | `/kv/mget?keys=key1,key2,key3` | Retrieve values for multiple keys in a single request; returns a JSON object mapping keys to values |
| PUT    | `/kv/{key}?ttl=100`            | Store value bytes for `key` with optional ttl in seconds, returns `204`                             |
| GET    | `/kv/{key}`                    | Retrieve value bytes for `key`                                                                      |
| DELETE | `/kv/{key}`                    | Remove value for `key`, returns `204`                                                               |
| POST   | `/save`                        | Force persist current store to disk (already done automatically)                                    |
| POST   | `/reset`                       | Clear all data from the store                                                                       |
| GET    | `/stats`                       | Runtime statistics (see below)                                                                      |

**Examples:**

```bash
# store a value
curl -X PUT http://localhost:8089/kv/foo -d 'bar'

# store a value for 10 seconds
curl -X PUT "http://localhost:8089/kv/foo?ttl=10" -d 'bar'

# fetch it
curl -X GET http://localhost:8089/kv/foo

# fetch multiple
curl -X GET http://localhost:8089/kv/mget?keys=key1,key2,key3

# delete it
curl -X DELETE http://localhost:8089/kv/foo

# persist to disk
curl -X POST http://localhost:8089/save

# reset the store
curl -X POST http://localhost:8089/reset
```

---

## Runtime Statistics (optional)

ElysianDB can expose lightweight, high‑cardinality‑safe counters to help you observe activity.

### Enabling

Set stats.enabled: true in elysian.yaml. When enabled:

The server exposes GET /stats (HTTP) with JSON metrics.

Counters are updated in the HTTP/TCP handlers with atomic 64‑bit operations (no locks).

Uptime is incremented once per second.

The /stats endpoint is served by the HTTP server. If HTTP is disabled, you won't be able to fetch stats even if stats.enabled: true.

### Metrics

All counters are uint64 and encoded as JSON strings. Example payload:

```
{
  "keys_count": "1203",
  "expiration_keys_count": "87",
  "uptime_seconds": "3605",
  "total_requests": "184467",
  "hits": "160002",
  "misses": "24465"
}
```

### Field semantics

keys_count — number of live keys currently in the store (post‑TTL purge). Updated on create/delete.

expiration_keys_count — number of keys currently tracked with TTL.

uptime_seconds — seconds since process start.

total_requests — total HTTP+TCP requests handled by ElysianDB (sum of all operations).

hits / misses — successful vs. not‑found lookups.


## Benchmarks (local, indicative)

Two separate load generators are provided:

* **TCP**: native benchmark tool (`make tcp_benchmark`) (fully made by AI)
* **HTTP**: k6 script (`make http_benchmark`)

### TCP (paired SET→GET, payload 16B)

```
VUs: 500, duration: 20s, keys: 20,000, payload: 16B, paired mode (SET→GET 1:1)
Throughput: ~361,966 req/s  (≈180,983 pairs/s)
Latency:    p50 0.84 ms, p95 3.80 ms, p99 6.11 ms, max 28.53 ms
Errors:     0, Misses: 0
```

### HTTP (PUT/GET/DEL mix)

```
VUs: 200, duration: 30s
Requests:   ~72,814 req/s
Latency:    p50 ≈ 2.1 ms, p95 6.54 ms, max 45.83 ms
Errors:     0.00%
```

> The TCP benchmark runs a minimal text protocol with paired SET→GET and tiny payloads; the HTTP test mixes PUT/GET/DEL and pays the HTTP parsing overhead. As a result, **TCP typically delivers \~5× higher RPS** in this setup.

**Shortcuts:**

```bash
make tcp_benchmark   # runs the TCP benchmark tool with sensible defaults (fully made by AI)
make http_benchmark  # runs the k6 script (requires k6)
```

You can tweak VUs/duration/keys in scripts or via env vars as documented in the benchmark files.
