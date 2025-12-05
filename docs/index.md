# ElysianDB Documentation

ElysianDB is a **lightweight key–value datastore** written in Go. It provides a **zero‑configuration REST API**, a **simple KV HTTP API**, and a **Redis‑style TCP protocol**.

This document explains how to use ElysianDB, its features, configuration, and runtime behavior.

---

## Overview

ElysianDB lets you store and query data instantly — with or without defining schemas or models.

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
  schema:
    enabled: true
    strict: true
  index:
    workers: 4
  cache:
    enabled: true
    cleanupIntervalSeconds: 10
```

### Configuration Fields

| Key                                  | Description                                                                           |
| ------------------------------------ | ------------------------------------------------------------------------------------- |
| **store.folder**                     | Path where data is persisted to disk                                                  |
| **store.shards**                     | Number of memory shards (must be a power of two)                                      |
| **store.flushIntervalSeconds**       | Interval for automatic persistence to disk                                            |
| **store.crashRecovery.enabled**      | Enables crash recovery logs for durability                                            |
| **store.crashRecovery.maxLogMB**     | Maximum size of recovery logs before flush                                            |
| **server.http**                      | Enables and configures the HTTP REST/KV interface                                     |
| **server.tcp**                       | Enables and configures the TCP text protocol interface                                |
| **log.flushIntervalSeconds**         | Interval for flushing in-memory logs                                                  |
| **stats.enabled**                    | Enables runtime metrics and `/stats` endpoint                                         |
| **api.index.workers**                | Number of workers that rebuild dirty indexes                                          |
| **api.cache.enabled**                | Enables REST API caching for repeated queries                                         |
| **api.schema.enabled**               | Enables automatic schema inference and validation                                     |
| **api.schema.strict**                | If true and schema is manual, new fields are rejected and deep validation is enforced |
| **api.cache.cleanupIntervalSeconds** | Interval for cache expiration cleanup                                                 |
| **security.authentication.enabled**  | Enables authentication layer for all endpoints                                        |
| **security.authentication.mode**     | Authentication mode (currently supports `basic`)                                      |

---

## REST API

ElysianDB automatically exposes REST endpoints per entity. Entities are inferred from the URL.
(All HTTP requests return a `X-Elysian-Version`)
For some requests, there is a `X-Elysian-Cache` header with values : `HIT` or `MISS`

### CRUD Operations

| Method   | Endpoint                      | Description                                                 |
| -------- | ----------------------------- | ----------------------------------------------------------- |
| `POST`   | `/api/<entity>`               | Create one or multiple JSON documents (auto‑ID if missing)  |
| `GET`    | `/api/<entity>`               | List all documents, supports pagination, sorting, filtering |
| `GET`    | `/api/<entity>/schema`        | Schema for entity                                           |
| `GET`    | `/api/<entity>/<id>`          | Retrieve document by ID                                     |
| `PUT`    | `/api/<entity>/<id>`          | Update a single document by ID                              |
| `PUT`    | `/api/<entity>`               | Update multiple documents (batch update)                    |
| `DELETE` | `/api/<entity>/<id>`          | Delete document by ID                                       |
| `DELETE` | `/api/<entity>`               | Delete all documents for an entity                          |
| `GET`    | `/api/export`                 | Dumps all entities as a JSON object                         |
| `POST`   | `/api/import`                 | Imports all objects from a JSON dump                        |
| `POST`   | `/api/<entity>/migrate`       | Run a **migration** across all documents for an entity      |
| `GET`    | `/api/<entity>/count`         | Counts all documents for an entity                          |
| `GET`    | `/api/<entity>/<id>/exists`   | Verifiy if an entity exists                                 |

---

# Schema API

ElysianDB provides two complementary schema systems: **automatic inference** and **manual strict schemas**, both now supporting **required fields**.

---

## 1. Automatic Schema Inference

When `api.schema.enabled: true` **and no manual schema exists**, ElysianDB automatically infers the schema from incoming documents.

### How it works

* The **first inserted document defines the initial schema**.
* Each field's type is extracted (`string`, `number`, `boolean`, `object`, `array`).
* Nested objects and arrays are recursively analyzed.
* Inferred fields automatically include:

  * `required = false` when `api.schema.strict = false`
  * `required = true` when `api.schema.strict = true`

### Behavior on subsequent writes

#### If `api.schema.strict = false` (default):

* Type mismatches are rejected.
* **New fields are allowed** and auto-extend the schema (with `required=false`).

#### If `api.schema.strict = true`:

* **New fields are rejected**.
* Only fields already defined in the schema may appear.
* Missing fields marked `required=true` produce validation errors.

### Example

**Inserted document:**

```json
{"title": "Example", "author": {"name": "Alice"}, "tags": ["go"]}
```

**Inferred schema:**

```json
{
  "id": "articles",
  "fields": {
    "title":   {"type": "string", "required": false},
    "author":  {"type": "object", "required": false, "fields": {
      "name": {"type": "string", "required": false}
    }},
    "tags":    {"type": "array", "required": false}
  }
}
```

(If strict mode was enabled globally, all `required` would be `true`.)

---

## 2. Manual Schema (Strict Mode)

You may explicitly define a schema using:

```
PUT /api/<entity>/schema
```

### Behavior when setting a manual schema

* The schema is saved as an entity inside ElysianDB.
* `_manual: true` marks the schema as user-defined.
* **Strict mode automatically applies for the entity**:

  * No new fields allowed.
  * Missing fields marked `required: true` produce validation errors.

Each field may explicitly define:

```json
{"type": "string", "required": true}
```

### Example manual schema

```
PUT /api/articles/schema
```

Payload:

```json
{
  "fields": {
    "title":     {"type": "string", "required": true},
    "published": {"type": "boolean", "required": false}
  }
}
```

After this, writes such as:

```json
{"published": true}
```

will be rejected because `title` is required.

---

## Schema Endpoints

### **GET /api/<entity>/schema**

Returns the current schema (manual or inferred).

Example:

```json
{
  "id": "articles",
  "fields": {
    "title":     {"name": "title", "type": "string",  "required": true},
    "published": {"name": "published", "type": "boolean", "required": false}
  }
}
```

If no schema exists yet:

```json
{"error": "schema not found"}
```

---

### **PUT /api/<entity>/schema**

Defines a new **manual schema**.

* Replaces any existing schema.
* `_manual: true` is automatically set.
* Strict mode applies immediately.
* All `required` flags provided in the payload are preserved.

Example response:

```json
{
  "id": "articles",
  "fields": {
    "title":     {"type": "string",  "required": true},
    "published": {"type": "boolean", "required": false}
  },
  "_manual": true
}
```

---

## Summary

| Feature     | Automatic Schema (strict=false) | Automatic Schema (strict=true) | Manual Schema |
| ----------- | ------------------------------- | ------------------------------ | ------------- |
| Creation    | On first write                  | On first write                 | `PUT /schema` |
| Required    | All fields `required=false`     | All fields `required=true`     | Declarative   |
| Strict mode | No                              | Yes                            | Always on     |
| New fields  | Allowed                         | Rejected                       | Rejected      |
| Type checks | Yes                             | Yes                            | Yes           |
| Best for    | Prototyping                     | Controlled API evolution       | Production    |

---

## Migrations

The **Migration API** allows you to apply **global data updates** declaratively via REST. It is especially useful for updating or cleaning up fields across all records of an entity.

### Endpoint

```
POST /api/<entity>/migrate
```

### Supported Actions

| Action | Description                                       |
| ------ | ------------------------------------------------- |
| `set`  | Update one or more fields (supports nested paths) |

### Example: Simple `set` Migration

```bash
curl -X POST http://localhost:8089/api/users/migrate \
  -H 'Content-Type: application/json' \
  -d '[
    { "set": [{ "active": true, "role": "member" }] }
  ]'
```

This updates all `users` documents, setting `active = true` and `role = member`.

### Example: Nested Field Migration

```bash
curl -X POST http://localhost:8089/api/accounts/migrate \
  -H 'Content-Type: application/json' \
  -d '[
    { "set": [{ "profile.city": "Paris", "profile.language": "fr" }] }
  ]'
```

ElysianDB automatically traverses the JSON hierarchy, creating intermediate maps if needed, and updates nested fields like `profile.city` for every record.

### Expected Response

```json
{
  "message": "Entity 'users' migrated successfully."
}
```

If an entity does not exist or the JSON is malformed, the endpoint responds with a 4xx error and a descriptive message.

### Combined Example

```bash
curl -X POST http://localhost:8089/api/articles/migrate \
  -H 'Content-Type: application/json' \
  -d '[
    { "set": [{ "published": false }] },
    { "set": [{ "tags": ["archived"] }] }
  ]'
```

### Nested Entity Creation (works the same way with update)

ElysianDB supports creating entities that contain **sub-entities** within the same request. When a JSON object includes fields containing other objects with an `@entity` key, those sub-entities are automatically created, linked, and assigned unique IDs if missing.

#### Example

```bash
curl -X POST http://localhost:8089/api/articles \
  -H 'Content-Type: application/json' \
  -d '{
    "title": "Nested Example",
    "authors": [
      {
        "@entity": "author",
        "fullname": "Mister T",
        "status": "writer",
        "job": {
          "@entity": "job",
          "id": "1234567890",
          "designation": "Worker"
        }
      },
      {
        "@entity": "author",
        "fullname": "Alberto",
        "status": "coco",
        "job": {
          "@entity": "job",
          "id": "1234567890"
        }
      }
    ]
  }'
```

This will create:

* an `article` linked to two `author` entities
* the `author` entities themselves, each linked to the same `job`
* the `job` entity if it does not already exist

ElysianDB automatically assigns IDs when missing and replaces nested objects with lightweight references of the form:

```json
{
  "@entity": "author",
  "id": "<uuid>"
}
```

This mechanism also works recursively and supports **arrays of sub-entities**, allowing a document to include multiple nested or shared linked entities.

### Query Parameters

* `countOnly` — If true, returns only the count
* `limit` — Max number of items to return
* `offset` — Number of items to skip
* `search` - Full text search
* `sort[field]=asc|desc` — Sort results (builds index automatically) and works with nested fields or entities
* `filter[field][op]=value` — Filter results by field
* `fields=title,slug` — Return only selected fields
* `includes=author,author.category` — Includes sub-entities

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

### Includes and Nested Filters

ElysianDB supports **relationship expansion** via the `includes` query parameter. It allows loading linked entities directly within query results.

#### Example


```bash
curl "http://localhost:8089/api/articles/123?includes=author,author.job"
```

Or

```bash
curl "http://localhost:8089/api/articles?includes=author,author.job"
```

This expands nested linked entities recursively:

```json
[
  {
    "id": "a1",
    "title": "Example",
    "author": {
      "id": "u1",
      "fullname": "Alice",
      "job": {
        "id": "j1",
        "designation": "Writer"
      }
    }
  }
]
```

#### Filtering on Included Entities

Filters can reference sub-entity fields when `includes` are provided.

```bash
curl "http://localhost:8089/api/articles?includes=author&filter[author.fullname][eq]=Alice"
```

Nested filters are supported to any depth:

```bash
curl "http://localhost:8089/api/posts?includes=author,author.job&filter[author.job.designation][eq]=Writer"
```

Using `includes=all` expands all linked entities recursively.

---

## Authentication

ElysianDB supports optional authentication to protect all REST and KV endpoints. Two modes are available: `basic` and `token`.

### Configuration

Enable authentication in your `elysian.yaml`:

```yaml
authentication:
  enabled: true
  mode: basic   # basic or token
```

If `enabled` is false, all endpoints are publicly accessible.

When using `token` mode, define a token:

```yaml
authentication:
  enabled: true
  mode: token
  token: my-secret-token
```

---

## User Management (Basic Auth)

When running in `basic` mode, users are stored in the `users.json` file inside the configured `store.folder` directory.
Passwords are hashed with bcrypt after combining them with a server-generated secret stored in `users.key`.

Both files are automatically created when needed.

---

## Client Usage

### Basic Authentication

All requests must include an `Authorization` header:

```
Authorization: Basic base64(username:password)
```

Example for `admin:secret`:

```
Authorization: Basic YWRtaW46c2VjcmV0
```

### Token Authentication

When using token mode:

```
Authorization: Bearer your-token
```

Example:

```
Authorization: Bearer my-secret-token
```

### curl Examples

Basic auth:

```bash
curl -u admin:secret http://localhost:8089/api
```

token auth:

```bash
curl -H "Authorization: Bearer my-secret-token" http://localhost:8089/api
```

---

## Error Handling

If authentication fails, the server returns:

```
401 Unauthorized
```

Requests without authentication headers are rejected when authentication is enabled.

---

## Security Notes

* Passwords are never stored in plaintext
* Hashing is done using bcrypt
* A per-instance secret key is used as part of the hashing process
* Deleting `users.key` invalidates all stored password hashes
* Deleting `users.json` removes all users
* In `token` mode, anyone with the token can fully access the API

---

## Transactions

ElysianDB provides a lightweight transaction system allowing clients to queue multiple write, update, and delete operations into an isolated context before applying them atomically.

### Transaction Lifecycle

A transaction follows this sequence:

1. Begin a new transaction.
2. Add operations to the transaction.
3. Commit the transaction to apply all operations.
4. Optionally roll back to discard all pending operations.

### Endpoints

#### Begin a Transaction

```
POST /api/tx/begin
```

Returns a transaction identifier used for subsequent operations.

#### Add Operations

Write:

```
POST /api/tx/{txId}/entity/{entity}
```

Body contains the JSON object to create.

Update:

```
PUT /api/tx/{txId}/entity/{entity}/{id}
```

Body contains the fields to update.

Delete:

```
DELETE /api/tx/{txId}/entity/{entity}/{id}
```

Each operation is stored in memory and not applied until commit.

#### Commit

```
POST /api/tx/{txId}/commit
```

Executes all queued operations in order. If any write validation fails or an update targets a non-existing document, the commit aborts and returns an error.

#### Rollback

```
POST /api/tx/{txId}/rollback
```

Discards all pending operations.

### Isolation and Behavior

Transactions are isolated from the live datastore until committed. Operations accumulated inside a transaction do not affect the current state and cannot be observed by other requests.

A commit applies operations sequentially:

* Write operations call the standard entity creation logic, including schema validation and sub-entity processing.
* Update operations apply partial updates to the existing document.
* Delete operations remove the targeted entity.

If any operation fails, the transaction is aborted, and no changes are applied.

### Error Handling

* Unknown transaction identifiers return an error for commit or when retrieving the transaction.
* Write validation errors cause commit to fail.
* Updates on missing entities cause commit to fail.
* Rollback on unknown transactions succeeds without effect.

### Use Cases

* Batch creation of multiple entities.
* Complex multi-step updates requiring atomicity.
* Staging modifications before applying them.

This system provides atomic grouped modifications while keeping the overall design simple and lightweight.

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

| Metric                  | Description                     |
| ----------------------- | ------------------------------- |
| `keys_count`            | Number of active keys           |
| `expiration_keys_count` | Keys with TTL                   |
| `uptime_seconds`        | Time since start                |
| `total_requests`        | Total requests handled          |
| `hits` / `misses`       | Successful vs failed lookups    |
| `entities_count`        | Counts all entites in JsonStore |

Example output:

```json
{
  "keys_count": "1203",
  "expiration_keys_count": "87",
  "uptime_seconds": "3605",
  "total_requests": "184467",
  "hits": "160002",
  "misses": "24465",
  "entities_count": "11062"
}
```

---

## Development

```bash
go build && ./elysiandb server
# or
make server
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

## Command-Line Interface (CLI)

ElysianDB provides a simple command-line interface with three available commands.

### 1. `server` (default)

Starts the ElysianDB server using the configuration file.

```bash
elysiandb server
```

Running the binary without arguments is equivalent:

```bash
elysiandb
```

---

### 2. `create-user` (basic authentication only)

Creates a new user when the authentication mode is set to `basic`.

```bash
elysiandb create-user
```

The command interactively prompts for:

* username
* password

The user is stored in:

```
<store.folder>/users.json
```

---

### 3. `delete-user` (basic authentication only)

Deletes an existing user.

```bash
elysiandb delete-user
```

The command prompts for the username to remove. If the user does not exist, an error is returned.

---

## Requirements

Both `create-user` and `delete-user` require the following configuration:

```yaml
security:
  authentication:
    enabled: true
    mode: basic
```

These commands are not available when using the `token` authentication mode.

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
