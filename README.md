# ElysianDB — The Database That Generates Your API

[![Docker Pulls](https://img.shields.io/docker/pulls/taymour/elysiandb.svg)](https://hub.docker.com/r/taymour/elysiandb)
[![Tests](https://img.shields.io/github/actions/workflow/status/taymour/elysiandb/ci.yaml?branch=main\&label=tests)](https://github.com/taymour/elysiandb/actions/workflows/ci.yaml)
[![Coverage](https://codecov.io/gh/elysiandb/elysiandb/branch/main/graph/badge.svg)](https://codecov.io/gh/taymour/elysiandb)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![JS Client](https://img.shields.io/badge/JS%20Client-available-brightgreen)](https://github.com/elysiandb/client-js)


Its built-in database, automatic REST API generation, and high-performance sharded engine make it one of the fastest ways to build and query data without configuration or tooling.

An Admin Web Interface is currently under development and will soon be available.

---

## Highlights

* Instant REST API — CRUD, pagination, filtering, sorting, includes (/api/entity) with zero configuration
* Fast In-Memory KV Engine — sharded store, optional TTL, atomic counters
* Auto-Indexing — lazy index creation on first sort request
* Schema-less JSON — dynamic structures; IDs generated automatically
* Automatic Schema Inference — schemas inferred from observed documents
* Schema API — GET /api/entity/schema for live schema inspection
* Manual Schema Override — PUT /api/entity/schema to define strict schemas
* Strict Schema Validation — reject writes that do not match the manual schema
* Nested Entity Creation — auto-create sub-entities via @entity fields
* Persistence — periodic flush plus crash recovery log
* Protocols — HTTP REST, TCP (Redis-style text protocol)
* High performance — minimal allocations and cache-friendly design
* Transactions - basic transactions and atomic operations
* Built-in Authentication — optional HTTP Basic authentication with salted bcrypt hashing or Token based authentication
* Admin UI — in progress

---

## Quick Example

```js
await fetch("http://localhost:8089/api/articles", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ title: "Hello", tags: ["go", "kv"], published: true })
});

const res = await fetch("http://localhost:8089/api/articles?limit=20&offset=0&sort[title]=asc");
const articles = await res.json();
```

---

## Performance Benchmarks (MacBook Pro M4)

Mixed CRUD, filtering, sorting, nested create, includes. Not microbenchmarks.

### Summary

| Scenario   | Load                | p95 Latency | RPS      | Errors |
| ---------- | ------------------- | ----------- | -------- | ------ |
| Dev Local  | 3 VUs / 100 keys    | 62 µs       | ~52.7k/s | 0%     |
| Small App  | 10 VUs / 500 keys   | 184 µs      | ~81.3k/s | 0%     |
| Light Prod | 25 VUs / 1000 keys  | 412 µs      | ~95.5k/s | 0%     |
| Heavy Load | 200 VUs / 5000 keys | 12.6 ms     | ~60.6k/s | 0%     |

---

## Official Clients

### JavaScript / TypeScript
Repository: https://github.com/elysiandb/elysiandbjs-client  
Install: `npm install @elysiandbjs/client`

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
    arenaChunkSize: 1048576
server:
  http: { enabled: true, host: 0.0.0.0, port: 8089 }
  tcp:  { enabled: true, host: 0.0.0.0, port: 8088 }
log:
  flushIntervalSeconds: 5
stats:
  enabled: false
security:
  authentication:
    enabled: false
    mode: token
    token: "your_secure_token"
api:
  index:
    workers: 4
  schema:
    enabled: true
    strict: false
  cache:
    enabled: true
    cleanupIntervalSeconds: 10
```

## Build and Run

```bash
go build && ./elysiandb server
# or
go run elysiandb.go server
```

---

ElysianDB is built to be simple, fast, and practical — a real backend you can ship in minutes.
