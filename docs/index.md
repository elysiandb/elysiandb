# ElysianDB Documentation

ElysianDB is a **lightweight key–value datastore** written in Go. It provides a **zero‑configuration REST API**, a **simple KV HTTP API**, and a **Redis‑style TCP protocol**.

This document explains how to use ElysianDB, its features, configuration, and runtime behavior.

---

## Overview

ElysianDB lets you store and query data instantly — without defining schemas or models.

You can:

* Use it as a **fast key–value store** (HTTP or TCP)
* Use it as an **instant backend** with **auto‑generated REST APIs**
* Combine both modes for hybrid usage

ElysianDB is designed for **developers who want to prototype, benchmark, or build simple services quickly**.

---

## Configuration

The configuration file (`elysian.yaml`) defines server behavior, storage, and logging.

### Example

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
  enabled: true
api:
  index:
    workers: 4
  cache:
    enabled: true
    cleanupIntervalSeconds: 10
```

### Configuration Fields

| Key                                  | Description                                            |
| ------------------------------------ | ------------------------------------------------------ |
| **store.folder**                     | Path where data is persisted to disk                   |
| **store.shards**                     | Number of memory shards (must be a power of two)       |
| **store.flushIntervalSeconds**       | Interval for automatic persistence to disk             |
| **store.crashRecovery.enabled**      | Enables crash recovery logs for durability             |
| **store.crashRecovery.maxLogMB**     | Maximum size of recovery logs before flush             |
| **server.http**                      | Enables and configures the HTTP REST/KV interface      |
| **server.tcp**                       | Enables and configures the TCP text protocol interface |
| **log.flushIntervalSeconds**         | Interval for flushing in‑memory logs                   |
| **stats.enabled**                    | Enables runtime metrics and `/stats` endpoint          |
| **api.index.workers**                | Number of workers that rebuild dirty indexes           |
| **api.cache.enabled**                | Enables REST API caching for repeated queries          |
| **api.cache.cleanupIntervalSeconds** | Interval for cache expiration cleanup                  |

---

## REST API

ElysianDB automatically exposes REST endpoints per entity. Entities are inferred from the URL.

### CRUD Operations

| Method   | Endpoint             | Description                                                 |
| -------- | -------------------- | ----------------------------------------------------------- |
| `POST`   | `/api/<entity>`      | Create one or multiple JSON documents (auto‑ID if missing)  |
| `GET`    | `/api/<entity>`      | List all documents, supports pagination, sorting, filtering |
| `GET`    | `/api/<entity>/<id>` | Retrieve document by ID                                     |
| `PUT`    | `/api/<entity>/<id>` | Update a single document by ID                              |
| `PUT`    | `/api/<entity>`      | Update multiple documents (batch update)                    |
| `DELETE` | `/api/<entity>/<id>` | Delete document by ID                                       |
| `DELETE` | `/api/<entity>`      | Delete all documents for an entity                          |

### Query Parameters

* `limit` — Max number of items to return
* `offset` — Number of items to skip
* `sort[field]=asc|desc` — Sort results (builds index automatically)
* `filter[field][op]=value` — Filter results by field

### Filtering Operators

| Operator       | Meaning                                |
| -------------- | -------------------------------------- |
| `eq`           | Equals                                 |
| `neq`          | Not equals                             |
| `lt` / `lte`   | Less than / less than or equal         |
| `gt` / `gte`   | Greater than / greater than or equal   |
| `contains`     | Array or string contains value         |
| `not_contains` | Array or string does not contain value |
| `all`          | Array includes all listed values       |
| `any`          | Array includes any listed value        |
| `none`         | Array excludes all listed values       |

### Examples

```bash
# Create a single entity
curl -X POST http://localhost:8089/api/users \
  -H 'Content-Type: application/json' \
  -d '{"name": "Alice", "age": 30}'

# Create multiple entities
curl -X POST http://localhost:8089/api/users \
  -H 'Content-Type: application/json' \
  -d '[{"name": "Bob"}, {"name": "Charlie"}]'

# Batch update entities
curl -X PUT http://localhost:8089/api/users \
  -H 'Content-Type: application/json' \
  -d '[{"id": "u1", "age": 35}, {"id": "u2", "name": "Bobby"}]'

# Query with filters and sorting
curl "http://localhost:8089/api/users?limit=10&offset=0&sort[name]=asc&filter[age][gt]=25"
```

---

## KV HTTP API

The KV API is a minimal interface for basic key–value operations.

| Method   | Path                      | Description                         |
| -------- | ------------------------- | ----------------------------------- |
| `PUT`    | `/kv/{key}?ttl=seconds`   | Store a value (optional TTL)        |
| `GET`    | `/kv/{key}`               | Retrieve a value                    |
| `GET`    | `/kv/mget?keys=key1,key2` | Retrieve multiple keys              |
| `DELETE` | `/kv/{key}`               | Delete key(s)                       |
| `POST`   | `/save`                   | Force flush to disk                 |
| `POST`   | `/reset`                  | Reset all keys                      |
| `GET`    | `/stats`                  | Return runtime metrics (if enabled) |

### Example

```bash
# Store a value
curl -X PUT http://localhost:8089/kv/foo -d 'bar'

# Retrieve it
curl http://localhost:8089/kv/foo

# Store with TTL=10 seconds
curl -X PUT "http://localhost:8089/kv/foo?ttl=10" -d 'bar'
```

---

## TCP Protocol (Redis‑style)

A lightweight text protocol for direct, low‑latency access.

**Commands:**

```
PING                → PONG
SET <key> <value>   → OK
SET TTL=10 <key> <value> → OK (with expiration)
GET <key>           → value
MGET key1 key2 ...  → multiple values
DEL <key>           → Deleted 1
RESET               → OK (clears store)
SAVE                → OK (flush to disk)
```

### Example (telnet)

```bash
telnet localhost 8088
SET foo bar
GET foo
PING
```

---

## Indexing

ElysianDB builds indexes **lazily**. The first time you sort by a field, an index is created.

You can also manually rebuild all indexes via the internal API or restart the database. Indexes are stored per field and entity.

---

## Persistence & Crash Recovery

Data is automatically persisted:

1. **Periodically**, based on `store.flushIntervalSeconds`
2. **On shutdown** (SIGTERM / SIGINT)
3. **On demand** via HTTP `/save` or TCP `SAVE`

Crash recovery ensures data durability even if the process crashes mid-write.

---

## Runtime Statistics

When `stats.enabled: true`, the following metrics are available at `/stats`:

| Metric                  | Description                  |
| ----------------------- | ---------------------------- |
| `keys_count`            | Number of active keys        |
| `expiration_keys_count` | Keys with TTL                |
| `uptime_seconds`        | Time since start             |
| `total_requests`        | Total requests handled       |
| `hits` / `misses`       | Successful vs failed lookups |

Example output:

```json
{
  "keys_count": "1203",
  "expiration_keys_count": "87",
  "uptime_seconds": "3605",
  "total_requests": "184467",
  "hits": "160002",
  "misses": "24465"
}
```

---

## Development

```bash
go build && ./elysiandb
# or
make run
```

Run tests:

```bash
make test
```

Run performance benchmarks:

```bash
make tcp_benchmark
make http_benchmark # requires installing k6
make api_benchmark # requires installing k6
```

---

## Docker

```bash
docker run --rm -p 8089:8089 -p 8088:8088 taymour/elysiandb:latest
```

With persistence:

```bash
docker run -d --name elysiandb \
  -p 8089:8089 -p 8088:8088 \
  -v elysian-data:/data \
  taymour/elysiandb:latest
```
