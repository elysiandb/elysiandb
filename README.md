# ElysianDB — Turn Raw JSON into a Full Backend Instantly

[![Docker Pulls](https://img.shields.io/docker/pulls/taymour/elysiandb.svg)](https://hub.docker.com/r/taymour/elysiandb)
[![Tests](https://img.shields.io/github/actions/workflow/status/taymour/elysiandb/ci.yaml?branch=main\&label=tests)](https://github.com/taymour/elysiandb/actions/workflows/ci.yaml)
[![Coverage](https://codecov.io/gh/elysiandb/elysiandb/branch/main/graph/badge.svg)](https://codecov.io/gh/taymour/elysiandb)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![JS Client](https://img.shields.io/badge/JS%20Client-available-brightgreen)](https://github.com/elysiandb/client-js)

ElysianDB is a self-contained backend engine that turns raw JSON into a usable, queryable API in seconds.

It combines automatic REST API generation, advanced querying and indexing, authentication, ACLs, hooks, and optional KV and TCP interfaces into a single fast Go binary, while supporting multiple storage backends through a pluggable engine system.

The goal is simple: remove backend boilerplate so you can focus on shipping features instead of infrastructure.

## Why ElysianDB

Most projects start the same way: define models, write CRUD endpoints, add filters, pagination, authentication, indexing, caching, migrations, and admin tooling.

ElysianDB removes this entire layer.

You send JSON. You get a fully functional backend.

No ORM. No controllers to scaffold. No migrations to write. No schema to maintain unless you explicitly want one.

Here is a related and explicative article https://dev.to/taymour/why-i-built-elysiandb-2f5i

## Storage Engine

ElysianDB uses a **pluggable storage engine abstraction** that cleanly separates API logic from the underlying persistence layer.

The same REST API, Query API, ACLs, hooks, transactions, and Admin UI work identically regardless of the selected engine.

The storage engine is selected at startup via configuration:

```yaml
engine:
  name: internal
```

### Available Engines

**internal (default)**

The `internal` engine is the original storage engine shipped with ElysianDB. It provides:

* Sharded in-memory storage
* Periodic disk persistence
* Crash recovery via write-ahead logs
* Lazy index creation
* Very high throughput and low latency
* Full compatibility with all ElysianDB features

This engine is production-ready and optimized for speed, prototyping, and controlled workloads.

**mongodb**

The `mongodb` engine allows ElysianDB to run on top of MongoDB while keeping the same API surface and behavior.

It provides:

* Persistence backed by MongoDB
* Better durability guarantees for long-lived datasets
* Horizontal scalability through MongoDB deployments
* Identical REST API, Query API, ACLs, hooks, and Admin UI behavior

Example configuration:

```yaml
engine:
  name: mongodb
  uri: mongodb://elysian:elysian@localhost:27017/elysiandb
```

The MongoDB engine is designed for users who want ElysianDB’s API and query model while relying on an external database for storage, replication, or operational constraints.

### Future Engines

The engine abstraction is designed to support additional backends in the future, such as:

* Alternative persistence models
* Embedded databases
* Specialized engines optimized for specific workloads
* Experimental or in-memory–only backends

All engines are selected using the same configuration mechanism, without impacting application code or API usage.

## What You Get Out of the Box

Instant REST API with CRUD operations, pagination, sorting, filtering, projections, and includes

Advanced query system with logical operators, nested filters, array traversal, and deterministic caching

High-performance datastore with lazy indexing and predictable execution

Optional strict schema validation with manual overrides

Nested entity creation and linking using simple JSON conventions

Built-in authentication with token, basic, or user-based modes

Access control lists enforced at the API level

Configurable JavaScript hooks to enrich or filter data at read time

Optional Admin UI served directly by the binary

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

With the internal engine, the system relies on sharded in-memory storage, minimal allocations, and cache-friendly data structures.

On a MacBook Pro M4, mixed workloads with CRUD, filtering, sorting, nested writes, and includes reach tens of thousands of requests per second with sub-millisecond latencies under moderate load.

With the MongoDB engine, performance is naturally bounded by MongoDB and network latency, but remains stable and predictable while enabling durability and scalability.

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

## Build and Run Locally

```bash
go build && ./elysiandb server
```

Or:

```bash
go run elysiandb.go server
```

## Philosophy

ElysianDB favors clarity over magic and determinism over flexibility.

Every query produces the same result for the same input.

There are no hidden joins, no implicit behavior, and no background mutations.

What you send is what you get.

## Status

ElysianDB is actively developed and evolving.

The MongoDB engine expands its applicability to more persistent and scalable workloads, while the internal engine remains ideal for speed and simplicity.

Feedback, issues, and contributions are welcome.
