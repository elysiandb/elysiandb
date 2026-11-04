# ElysianDB — Turn Raw JSON into a Full Backend Instantly

[![Docker Pulls](https://img.shields.io/docker/pulls/taymour/elysiandb.svg)](https://hub.docker.com/r/taymour/elysiandb)
[![Tests](https://img.shields.io/github/actions/workflow/status/taymour/elysiandb/ci.yaml?branch=main\&label=tests)](https://github.com/taymour/elysiandb/actions/workflows/ci.yaml)
[![Coverage](https://codecov.io/gh/elysiandb/elysiandb/branch/main/graph/badge.svg)](https://codecov.io/gh/taymour/elysiandb)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![JS Client](https://img.shields.io/badge/JS%20Client-available-brightgreen)](https://github.com/elysiandb/client-js)

ElysianDB is a self-contained backend engine that turns raw JSON into a usable, queryable API in seconds.

It combines automatic REST API generation, advanced querying and indexing, authentication, ACLs, hooks, and optional KV and TCP interfaces into a single fast Go binary, with an embedded storage engine built on Pebble.

The goal is simple: remove backend boilerplate so you can focus on shipping features instead of infrastructure.

## Why ElysianDB

Most projects start the same way: define models, write CRUD endpoints, add filters, pagination, authentication, indexing, caching, migrations, and admin tooling.

ElysianDB removes this entire layer.

You send JSON. You get a fully functional backend.

No ORM. No controllers to scaffold. No migrations to write. No schema to maintain unless you explicitly want one.

Here is a related and explicative article https://dev.to/taymour/why-i-built-elysiandb-2f5i

## Storage Engine

ElysianDB stores everything in its `internal` engine, designed together with its API: it knows entities, insertion order, references, schemas, the filter language and hooks. The engine keeps this design and delegates the disk and memory mechanics to [Pebble](https://github.com/cockroachdb/pebble), the embedded key-value store of CockroachDB, written in pure Go: the binary stays a single static file, with no service to run next to it.

It provides:

* Documents in insertion order, an id directory, and indexes for sorts, filters, search and references, built when first needed and kept up to date by every write
* Atomic writes: a request, a transaction or a batch of documents is one Pebble batch, written to the write-ahead log before the answer
* Snapshot reads that never wait for writes, and updates checked for conflicts so that no update is lost
* A memory budget that bounds the whole process: Pebble's block cache and memtables, index builds, request memory and the API cache
* Full compatibility with all ElysianDB features (REST API, Query API, ACLs, hooks, transactions, Admin UI)

Data folders written by ElysianDB 0.2.4 and by earlier builds of the internal engine are migrated at the first start, without any action. The MongoDB engine has been removed. See [Migrating to the Pebble storage](docs/migration-pebble.md).

## What You Get Out of the Box

Instant REST API with CRUD operations, pagination, sorting, filtering, projections, and includes

Advanced query system with logical operators, nested filters, array traversal, and deterministic caching

High-performance datastore with lazy indexing and predictable execution

Optional strict schema validation with manual overrides

Nested entity creation and linking using simple JSON conventions

Built-in authentication with token, basic, or user-based modes

Access control lists enforced at the API level

Configurable JavaScript hooks to enrich or filter data at read time

Optional Admin UI served directly by the binary, with a live dashboard, monitoring charts and a log viewer

Structured logs (text or JSON), audit of admin actions, and an optional Prometheus `/metrics` endpoint

HTTP REST API, Redis-style TCP protocol, and key–value endpoints

All of this ships as a single binary or Docker image.

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

No model definition. No migration. No configuration.

## Querying Without SQL

ElysianDB includes a dedicated Query API that lets you express complex conditions in a single request.

You can combine filters, logical operators, sorting, pagination, projections, and counts without writing SQL or custom endpoints.

Queries are deterministic, type-aware, and executed predictably, making them easy to debug and safe to cache.

## Performance

ElysianDB is designed for low latency and high throughput.

Paged, filtered, sorted, counted and searched requests use indexes and cost about the same on 1,000 or 60,000 documents, and writes are grouped into a few disk writes.

On a MacBook Pro M4, mixed workloads with CRUD, filtering, sorting, nested writes, and includes reach tens of thousands of requests per second with sub-millisecond latencies under moderate load.

These are real API scenarios, not synthetic microbenchmarks.

## When to Use ElysianDB

Rapid prototyping and MVPs

Frontend-first development where the backend should not block progress

Internal tools and admin backends

Mocking APIs and datasets

Simple services that do not justify a full ORM or framework-heavy backend

High-performance read-heavy APIs with predictable access patterns

## When Not to Use It

ElysianDB is not a relational database replacement.

It is not designed for complex cross-entity joins or heavy transactional workloads.

If you need strong relational guarantees, complex SQL queries, or decades of ecosystem tooling, a traditional relational database may be a better fit.

## Official Clients

JavaScript and TypeScript client available at:
[https://github.com/elysiandb/client-js](https://github.com/elysiandb/client-js)

## Run with Docker

```bash
docker run --rm -p 8089:8089 -p 8088:8088 taymour/elysiandb:latest
```

The image ships with the Admin UI and a `HEALTHCHECK` on `/health`, which needs no credentials. The default configuration (`docker/elysian.yaml`) disables authentication and the Admin UI; mount your own file to change it:

```bash
docker run -d -p 8089:8089 -v elysian-data:/data -v $(pwd)/elysian.yaml:/app/elysian.yaml:ro taymour/elysiandb:latest
```

## Build and Run Locally

```bash
make build-admin   # optional: embeds the Admin UI in the binary
make build && ./elysiandb server
```

`make build` stamps the binary with the version from `git describe`, returned in the `X-Elysian-Version` header and by `/stats`. A plain `go build` reports `dev`:

```bash
go build && ./elysiandb server
```

Or:

```bash
go run elysiandb.go server
```

## Security Notes

* With `security.authentication.enabled`, every HTTP route except `/health` and every TCP command except `PING`, `EXIT` and `AUTH` requires credentials. TCP clients authenticate with `AUTH <token>` or `AUTH <username> <password>`.
* Browsers on another origin can only call the API when that origin is listed in `server.http.cors.allowedOrigins` (empty by default).
* `GET /config` redacts the token and the password of `engine.uri`.

See the [documentation](docs/index.md) for the details.

## Philosophy

ElysianDB favors clarity over magic and determinism over flexibility.

Every query produces the same result for the same input.

There are no hidden joins, no implicit behavior, and no background mutations.

What you send is what you get.

## Status

ElysianDB is actively developed and evolving.

Upgrading from an earlier version needs no change: see [Migrating to the Pebble storage](docs/migration-pebble.md).

Feedback, issues, and contributions are welcome.
