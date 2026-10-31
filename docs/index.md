# ElysianDB Documentation

ElysianDB turns raw JSON into a complete backend instantly.

It combines a pluggable storage engine, an auto-generated REST API, advanced querying, and optional KV/TCP interfaces into a single, high-performance Go binary. Depending on your needs, ElysianDB can run with its built-in internal engine or delegate persistence and scalability to MongoDB, without changing your API or application logic.

This document explains how to use ElysianDB, its features, configuration, storage engines, and runtime behavior.


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
  http:
    enabled: true
    host: 0.0.0.0
    port: 8089
    cors:
      allowedOrigins: ["https://app.example.com"]
  tcp:  { enabled: true,  host: 127.0.0.1, port: 8088 }
log:
  flushIntervalSeconds: 5
stats:
  enabled: true
security:
  authentication:
    enabled: true
    mode: "token" # token|basic|user
    token: "your_token"
api:
  schema:
    enabled: true
    strict: true
  index:
    workers: 4
  cache:
    enabled: true
    cleanupIntervalSeconds: 10
  hooks:
    enabled: true
adminui:
  enabled: true
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
| **server.http.cors.allowedOrigins**  | Origins allowed to call the HTTP API from a browser (see CORS below). Empty by default |
| **server.tcp**                       | Enables and configures the TCP text protocol interface (see TCP Protocol below) |
| **log.flushIntervalSeconds**         | Interval for flushing in-memory logs                                                  |
| **stats.enabled**                    | Enables runtime metrics and `/stats` endpoint                                         |
| **api.index.workers**                | Number of workers that rebuild dirty indexes                                          |
| **api.cache.enabled**                | Enables REST API caching for repeated queries                                         |
| **api.schema.enabled**               | Enables automatic schema inference and validation                                     |
| **api.schema.strict**                | If true and schema is manual, new fields are rejected and deep validation is enforced |
| **api.cache.cleanupIntervalSeconds** | Interval for cache expiration cleanup                                                 |
| **api.hooks.enabled**                | Enables JavaScript hooks (see Hooks below)                                        |
| **security.authentication.enabled**  | Enables authentication layer for all endpoints except `/health`                       |
| **security.authentication.mode**     | Authentication mode (currently supports `basic`, `token` `user`)                      |
| **security.authentication.token**    | Instance token used in `token` mode                                                   |
| **engine.name**                      | Storage engine: `internal` (default) or `mongodb`                                     |
| **engine.uri**                       | Connection URI for the `mongodb` engine                                               |
| **adminui.enabled**                  | Enables the admin web interface                                                       |

Keys that ElysianDB does not know are ignored, and a warning listing them is printed at startup (for example `field authentication not found in type configuration.Config` when `authentication:` is written at the root instead of under `security:`).

---

## REST API

ElysianDB automatically exposes REST endpoints per entity. Entities are inferred from the URL.
(All HTTP requests return a `X-Elysian-Version` header with the version the binary was built with, see Versioning below)
For some requests, there is a `X-Elysian-Cache` header with values : `HIT` or `MISS`

### CRUD Operations

| Method   | Endpoint                                  | Description                                                 |
| -------- | ----------------------------------------- | ----------------------------------------------------------- |
| `POST`   | `/api/<entity>/create`                    | Create a new type of entity with schema                     |
| `POST`   | `/api/<entity>`                           | Create one or multiple JSON documents (auto‑ID if missing)  |
| `POST`   | `/api/query`                              | Post a complex query                                        |
| `GET`    | `/api/<entity>`                           | List all documents, supports pagination, sorting, filtering |
| `GET`    | `/api/<entity>/schema`                    | Schema for entity                                           |
| `PUT`    | `/api/<entity>/schema`                    | Update schema for entity                                    |
| `GET`    | `/api/<entity>/<id>`                      | Retrieve document by ID                                     |
| `PUT`    | `/api/<entity>/<id>`                      | Update a single document by ID                              |
| `PUT`    | `/api/<entity>`                           | Update multiple documents (batch update)                    |
| `DELETE` | `/api/<entity>/<id>`                      | Delete document by ID                                       |
| `DELETE` | `/api/<entity>`                           | Delete all documents for an entity                          |
| `GET`    | `/api/export`                             | Dumps all entities as a JSON object                         |
| `POST`   | `/api/import`                             | Writes the documents of a JSON dump (see Import below)      |
| `POST`   | `/api/<entity>/migrate`                   | Run a **migration** across all documents for an entity      |
| `GET`    | `/api/<entity>/count`                     | Counts all documents for an entity                          |
| `GET`    | `/api/<entity>/<id>/exists`               | Verifiy if an entity exists                                 |
| `GET`    | `/api/entity/types`                       | Schemas of all entity types (see below)                     |
| `GET`    | `/api/entity/types/name`                  | Names of all entity types                                   |
| `GET`    | `/api/security/user`                      | List all of the users                                       |
| `GET`    | `/api/security/user/<user_name>`          | Retrieve a user                                             |
| `POST`   | `/api/security/user`                      | Create a user                                               |
| `PUT`    | `/api/security/user/<user_name>/password` | Change a user's password                                    |
| `PUT`    | `/api/security/user/<user_name>/role`.    | Change a user's role                                        |
| `DELETE` | `/api/security/user/<user_name>`          | Delete a user                                               |
| `POST`   | `/api/security/login`                     | Log in as a user                                            |
| `POST`   | `/api/security/logout`                    | Log out as a user                                           |
| `GET`    | `/api/security/me`                        | Current authenticated user                                  |
| `GET`    | `/api/acl/<user_name>/<entity>`           | Retrieve ACL for username and entity type                   |
| `GET`    | `/api/acl/<user_name>`                    | Retrieve ACL for username and all entity types              |
| `PUT`    | `/api/acl/<user_name>/<entity>`           | Update ACL for username and entity type                     |
| `PUT`    | `/api/acl/<user_name>/<entity>/default`   | restore default ACL for username and entity type            |

### Entity Types

`GET /api/entity/types/name` returns the names of the entity types:

```json
{ "entities": ["article", "book"] }
```

`GET /api/entity/types` returns their schemas. Each entry of `entities` is the schema of one entity type, **encoded as a JSON string** (decode each entry with `JSON.parse`):

```json
{ "entities": ["{\"_manual\":true,\"fields\":{\"title\":{\"name\":\"title\",\"required\":true,\"type\":\"string\"}},\"id\":\"book\"}"] }
```

Core entity types (`_elysiandb_core_*`, which hold users, ACLs, hooks and schemas) are not listed.

### Core Entities

The `_elysiandb_core_*` entity types (users with their password hashes, ACLs, hooks and schemas) are managed by ElysianDB itself. They cannot be read or written through the generic routes (`/api/<entity>...`, `POST /api/query` and the transaction routes), whatever the authentication mode: these requests return `403`. Use the dedicated endpoints instead (User Management, ACL and Hooks APIs).

### Partial Updates

`PUT /api/<entity>/<id>` and `PUT /api/<entity>` merge the body into the stored document: fields absent from the body keep their stored value. To unset a field, send it with `null`; the field is then stored as `null`, which filters treat like a missing field.

### Creating a List of Documents

`POST /api/<entity>` with a JSON array validates every item before writing any of them. If one item is invalid, nothing is written and the response is `400` with one list of errors per item, in the same order as the request (`[]` for valid items):

```json
[[], [{ "Field": "a", "Message": "expected type string but got number" }]]
```

### Import

`POST /api/import` takes the format produced by `GET /api/export` (`{ "<entity>": [ <document>, ... ] }`). For each entity present in the dump:

* the entity indexes are dropped, then every listed document is written as it appears in the dump, with schema validation
* documents of that entity that are not in the dump are no longer listed, but stay readable by id
* the owner field `_elysiandb_core_username` is kept when the dump contains it; a document without it has no owner

Entities absent from the dump are left untouched.

---

## Storage Engine Selection

ElysianDB exposes a **pluggable storage engine abstraction** that allows the core database logic to run independently of the underlying storage implementation.

This makes it possible to switch storage backends **without changing the API, query language, ACL rules, hooks, transactions, or Admin UI**. The same requests, payloads, and behaviors apply regardless of the engine in use.

---

### Engine Configuration

The storage engine is selected in `elysian.yaml`:

```yaml
engine:
  name: internal
```

Some engines may require additional configuration fields.

---

### Available Engines

#### `internal` (default)

The `internal` engine is the built-in storage engine provided by ElysianDB.

It offers:

* In-memory sharded storage
* Periodic disk persistence
* Crash recovery via write-ahead logs
* Lazy indexing
* Deterministic query execution
* Full compatibility with all ElysianDB features (REST API, Query API, ACLs, hooks, transactions, Admin UI)

This engine is **production-ready** and requires no external dependencies. It is well suited for prototyping, embedded deployments, internal tools, and high-performance read-heavy workloads.

---

#### `mongodb`

The `mongodb` engine allows ElysianDB to use **MongoDB as its persistence layer** while preserving the same API surface and developer experience.

It provides:

* External persistence backed by MongoDB
* Improved durability and operational familiarity
* Horizontal scalability through MongoDB tooling
* Native BSON support and efficient document storage
* Full compatibility with ElysianDB features (REST API, Query API, ACLs, hooks, includes, Admin UI)

This engine is ideal when you want:

* An external, battle-tested database
* Easier backups and replication
* Shared storage across multiple instances
* A smoother transition from prototypes to long-running services

##### Configuration Example

```yaml
engine:
  name: mongodb
  uri: mongodb://elysian:elysian@localhost:27017/elysiandb
```

Switching between `internal` and `mongodb` **does not require any API or payload changes**.

---

### Design Goals

The storage engine abstraction follows these principles:

* Clear separation between **API logic** and **storage implementation**
* Identical API behavior across engines
* No feature loss when switching engines
* Deterministic results regardless of backend
* Minimal overhead for the default engine

This design allows ElysianDB to scale from an embedded, single-binary backend to an externally backed system without rewriting application logic.

---

### Notes

* If no engine is specified, `internal` is used by default
* Engine selection is evaluated at startup
* Switching engines may require data migration
* All engines share the same query model, schema rules, ACLs, and hooks


---

## Query API

ElysianDB provides a powerful **Query API** allowing you to express complex filters, logical conditions, sorting, pagination, projections, and counts using a single request. Queries are executed fully in-memory on top of the datastore and support nested fields and arrays.

---

### Endpoint

```http
POST /api/query
```

---

### Request Body

```json
{
  "entity": "article",
  "offset": 0,
  "limit": 10,
  "filters": { ... },
  "sorts": { "field": "asc|desc", "otherField": "asc|desc" },
  "fields": "field1,field2",
  "countOnly": false
}
```

---

### Parameters

| Field       | Type    | Description                             |        |
| ----------- | ------- | --------------------------------------- | ------ |
| `entity`    | string  | Target entity type                      |        |
| `offset`    | number  | Number of items to skip                 |        |
| `limit`     | number  | Maximum number of items returned        |        |
| `filters`   | object  | Logical filter tree                     |        |
| `sorts`     | object  | Sorting rules (`field: asc              | desc`), applied in the order of the keys |
| `fields`    | string  | Comma-separated list of returned fields |        |
| `countOnly` | boolean | Return only `{ "count": n }`            |        |

---

### Filtering Model

Filters are expressed as a **logical tree** composed of `and`, `or`, and **leaf conditions**.

#### Logical Nodes

```json
{
  "and": [ <filter>, <filter> ]
}
```

```json
{
  "or": [ <filter>, <filter> ]
}
```

#### Leaf Conditions

```json
{
  "field.path": { "operator": "value" }
}
```

A value can be a string, a number or a boolean (`{ "year": { "gte": 1980 } }`, `{ "published": { "eq": true } }`). Any other value (`null`, an array, an object) is rejected with `400`.

Nested paths and array traversal are supported automatically.

---

### Supported Operators

The Query API supports the same operators as the REST filters (see Filtering Operators below): `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `contains`, `not_contains`, `all`, `any` and `none`.

| Operator   | Description                                                            |
| ---------- | ---------------------------------------------------------------------- |
| `eq`       | Equals (supports `*` glob on strings, case-insensitive)                |
| `neq`      | Not equals                                                             |
| `lt`       | Less than (numbers, dates, or strings compared lexicographically)      |
| `lte`      | Less than or equal                                                     |
| `gt`       | Greater than                                                           |
| `gte`      | Greater than or equal                                                  |
| `contains` | Array contains the value, or string contains the substring             |

Glob patterns follow strict semantics:

* `*` matches any sequence
* `abc*def` matches prefix and suffix
* `*mid*` matches substring

---

### Nested and Array Filtering

Filters automatically traverse nested objects and arrays.

```json
{
  "categories.title": { "eq": "coco" }
}
```

Matches if **any** array element satisfies the condition.

---

### Complex Example

```json
{
  "entity": "article",
  "offset": 0,
  "limit": 10,
  "filters": {
    "and": [
      { "status": { "eq": "published" } },
      {
        "or": [
          { "title": { "eq": "*Go*" } },
          { "excerpt": { "eq": "*distributed*" } }
        ]
      },
      {
        "or": [
          {
            "and": [
              { "author.name": { "eq": "Taymour*" } },
              { "tags": { "contains": "Go" } }
            ]
          },
          {
            "and": [
              { "categories.title": { "eq": "coco" } },
              { "publishedAt": { "gte": "2025-01-01" } }
            ]
          }
        ]
      }
    ]
  },
  "sorts": { "publishedAt": "desc" }
}
```

---

### Sorting

Sorting is applied **before** offset and limit.

```json
"sorts": {
  "title": "asc",
  "publishedAt": "desc"
}
```

Sort keys are applied in the order they appear in the request: documents are sorted by `title`, and documents with the same `title` are sorted by `publishedAt`. Numbers compare numerically, dates chronologically and strings lexicographically. With several sort keys, documents that do not have a sort field come last, and values of different types are ordered numbers, dates, strings, then booleans.

Indexes are created lazily when a sort field is first used.

---

### Pagination Order

Execution order:

1. Load all entities
2. Apply filters
3. Apply sorting
4. Apply offset
5. Apply limit

---

### Field Projection

```json
"fields": "id,title,slug"
```

Only the listed fields are returned. Nested fields are supported.

---

### Count-Only Queries

```json
"countOnly": true
```

Response:

```json
{ "count": 42 }
```

Sorting and projection are ignored.

---

### Caching Behavior

* Query results are cached when API cache is enabled
* Cache keys are deterministic and include filters, sorting (with the order of the sort keys), pagination, fields, `countOnly`, and user context
* If hooks are enabled for an entity, caching is automatically bypassed
* Response header:

```
X-Elysian-Cache: HIT | MISS
```

---

### Errors

An invalid request returns `400` with a JSON body describing the problem:

```json
{ "error": "invalid value for year.eq: expected a string, a number or a boolean" }
```

---

### Notes

* Queries are fully deterministic
* Filters are strict and type-aware
* Unsupported field types never match, and an operator that does not apply to a field's type never matches
* Designed for correctness and debuggability rather than SQL-style ambiguity

---

### Entity Type Creation

ElysianDB allows you to explicitly **declare an entity type and its schema** before inserting any data.
This uses the same schema format and validation rules described in the **Schema API section** of the documentation.

---

#### Endpoint

```http
POST /api/<entity>/create
```

`<entity>` is the name of the entity you want to define (e.g. `books`, `movies`, `users`).

---

#### Request Body

The body must contain a `fields` object describing the schema of the entity.
Two forms are supported:

**Shorthand form:**

```json
{
  "fields": {
    "title": "string",
    "pages": "number"
  }
}
```

**Full form (same structure as manual schemas):**

```json
{
  "fields": {
    "title": { "type": "string", "required": true },
    "pages": { "type": "number", "required": false }
  }
}
```

ElysianDB converts this payload into a **manual schema** for the entity.

---

#### Behavior

On success:

* The entity type is registered.
* A manual schema is stored for the entity (see Schema API for details).
* Strict schema rules immediately apply for that entity. You can later adjust or replace this schema at any time using `PUT /api/<entity>/schema`.

Example response:

```json
{
  "id": "books",
  "fields": {
    "title": { "type": "string", "required": true },
    "pages": { "type": "number", "required": false }
  },
  "_manual": true
}
```

---

#### Error Handling

`400 Bad Request` is returned when:

* the JSON body is invalid
* the `fields` property is missing or invalid
* the entity type already exists

In all error cases, nothing is created.

---

#### Example Usage

**1) Declare the entity type:**

```bash
curl -X POST http://localhost:8089/api/book/create \
  -H 'Content-Type: application/json' \
  -d '{
    "fields": {
      "title": { "type": "string", "required": true },
      "pages": { "type": "number", "required": false }
    }
  }'
```

**2) Insert a valid book:**

```bash
curl -X POST http://localhost:8089/api/book \
  -H 'Content-Type: application/json' \
  -d '{ "title": "Dune", "pages": 700 }'
```

**3) Invalid insert (missing required field):**

```bash
curl -X POST http://localhost:8089/api/book \
  -H 'Content-Type: application/json' \
  -d '{ "pages": 123 }'
```

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

Each write **extends** the inferred schema; it never replaces it:

* **New fields are allowed** and are added to the schema, including fields inside nested objects and arrays of objects.
* Fields already in the schema keep their type: a value of another type is rejected (`expected type string but got number`).
* `null` is accepted for any field and does not set its type. A field first seen with `null` takes the type of the first non-null value written later.

This applies whatever the value of `api.schema.strict`. Strict mode (rejecting unknown fields and reporting missing `required` fields) only applies to **manual** schemas: with `strict = true`, inferred fields are recorded as `required = true`, but this flag is only enforced once the schema becomes manual, for example after `PUT /api/<entity>/schema`.

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

You may explicitly define a schema when creating the entity type with `POST /api/<entity>/create` (see Entity Type Creation), or replace the schema of an existing entity type using:

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

After this, creating a document such as:

```json
{"published": true}
```

will be rejected because `title` is required.

### Updates with a manual schema

`PUT /api/<entity>/<id>` and `PUT /api/<entity>` are partial updates: the body is merged into the stored document, and the **merged** document is validated.

* A body that only contains some fields is accepted, as long as the merged document has the required fields: `{"published": false}` on an existing article keeps its `title`.
* Every field of the body is validated: unknown fields and wrong types are rejected.
* `null` unsets an optional field (it is stored as `null`). `null` on a required field is rejected with `required field cannot be null`.
* Stored fields that the body does not touch are only checked for presence: a document written before a schema change can still be updated.

`null` is also accepted for optional fields when creating a document.

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

Defines a new **manual schema** for an existing entity type.

* The entity type must exist: create it first with `POST /api/<entity>/create` or by writing a document. Otherwise the response is `404 {"error":"entity not found"}`.
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

| Feature     | Automatic Schema (strict=false) | Automatic Schema (strict=true)          | Manual Schema (strict=true)          |
| ----------- | ------------------------------- | --------------------------------------- | ------------------------------------ |
| Creation    | On first write                  | On first write                          | `POST /create` or `PUT /schema`      |
| Required    | All fields `required=false`     | Recorded as `required=true`, not enforced | Declarative, enforced              |
| Strict mode | No                              | No                                      | Yes                                  |
| New fields  | Allowed, extend the schema      | Allowed, extend the schema              | Rejected                             |
| Type checks | Yes                             | Yes                                     | Yes                                  |
| Best for    | Prototyping                     | Prototyping with type safety            | Production                           |

With `api.schema.strict = false`, a manual schema is not strict: new fields are allowed and added to it, and missing required fields are not reported.

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
* `search` — Case-insensitive search in every string value of the document, nested values included (see Search below)
* `sort[field]=asc|desc` — Sort results (builds index automatically) and works with nested fields or entities
* `filter[field][op]=value` — Filter results by field
* `fields=title,slug` — Return only selected fields
* `includes=author,author.category` — Includes sub-entities

Filters, `search` and pagination combine: the documents that match every filter **and** the search are selected first, then `offset` and `limit` are applied.

### Search

* `search=zebra` matches documents where any string value contains `zebra`, whatever the case (`Zebra crossing` matches).
* If the value contains `*`, it is a glob matched against whole string values: `search=Zeb*` matches values that start with `Zeb`, `search=*ing` values that end with `ing`.

### Filtering Operators

Filters follow the same rules as the Query API. Nested paths traverse objects and arrays, including included sub-entities: `filter[categories.title][eq]=coco` matches if **any** element satisfies the condition. An entity that does not have the filtered field, or has it set to `null`, does not match, whatever the operator.

| Operator       | Strings                                  | Numbers / dates      | Arrays                                      |
| -------------- | ---------------------------------------- | -------------------- | ------------------------------------------- |
| `eq`           | Equals, `*` glob, case-insensitive       | Equals               | Same items as the comma-separated list      |
| `neq`          | Not equals, `*` glob, case-insensitive   | Not equals           | —                                           |
| `lt` / `lte`   | Lexicographic comparison                 | Less than (or equal) | —                                           |
| `gt` / `gte`   | Lexicographic comparison                 | Greater than (or equal) | —                                        |
| `contains`     | Contains the substring, case-insensitive | —                    | Has an item equal to the value              |
| `not_contains` | Does not contain the substring           | —                    | Has no item equal to the value              |
| `all`          | —                                        | —                    | Has every value of the comma-separated list |
| `any`          | —                                        | —                    | Has at least one value of the list          |
| `none`         | —                                        | —                    | Has none of the values of the list          |

* Dates (`2024-05-01` or RFC 3339) compare chronologically when both sides are dates; otherwise the values compare as strings. `contains` also works on dates, as strings.
* `contains` and `not_contains` look for the whole value: `filter[tags][contains]=Smith, John` matches the item `Smith, John`. Only `eq`, `all`, `any` and `none` split their value on commas.
* Booleans support `eq` and `neq`.
* An operator marked `—` never matches for that type.

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

Filters can reference sub-entity fields whether `includes` are provided or not.

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

ElysianDB supports optional authentication to protect all REST, KV and TCP endpoints. Three modes are available: `basic`, `user` and `token`.
When you boot Elysiandb, a default user (username: "admin", password: "admin") is created if it does not exist.

### Configuration

Enable authentication in your `elysian.yaml`, under `security`:

```yaml
security:
  authentication:
    enabled: true
    mode: basic   # basic, token or user
```

If `enabled` is false, all endpoints are publicly accessible.

When using `token` mode, define a token:

```yaml
security:
  authentication:
    enabled: true
    mode: token
    token: my-secret-token
```

The `authentication` block must be nested under `security`. Written at the root of the file, it is ignored (authentication stays disabled) and a warning about the unknown key is printed at startup.

`GET /health` never requires credentials, so the Docker `HEALTHCHECK`, orchestrators and load balancers can use it in every mode. It returns `200` with an empty body.

---

## User Management (Basic Auth)

When running in `basic` mode, users are stored in a core entity.
Passwords are hashed with bcrypt after combining them with a server-generated secret stored in `users.key`, created when needed.

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

Requests without authentication headers are rejected when authentication is enabled, except `GET /health`.

---

## Security Notes

* Passwords are never stored in plaintext
* Hashing is done using bcrypt
* A per-instance secret key is used as part of the hashing process
* Deleting `users.key` invalidates all stored password hashes
* In `token` mode, anyone with the token can fully access the API
* The token is compared in constant time
* `GET /config` never returns secrets: the token is replaced with `***` and the password of `engine.uri` with `xxxxx`

### Session cookie

In `user` mode, `POST /api/security/login` sets an `edb_session` cookie (`HttpOnly`, `SameSite=Strict`, valid 24 hours). The cookie is marked `Secure` only when the request reached ElysianDB over HTTPS, or through a proxy that sends `X-Forwarded-Proto: https`. Over plain HTTP, for example on a LAN address or a Docker service name, browsers would otherwise drop the cookie.

Sessions are stored in `sessions.json` in the store folder.

### CORS

Browsers only let pages from another origin call the HTTP API when that origin is allowed:

```yaml
server:
  http:
    cors:
      allowedOrigins: ["https://app.example.com", "http://localhost:5173"]
```

For an allowed origin, responses include `Access-Control-Allow-Origin` (the request origin), `Access-Control-Allow-Credentials: true`, and the allowed headers and methods. Other origins get no CORS header, so the browser blocks the response.

The list is empty by default: only same-origin pages, such as the Admin UI served at `/admin`, can call the API from a browser. `"*"` allows every origin, with credentials, which lets any website send requests with the session cookie of a logged-in user: only use it for local development. Clients that are not browsers (servers, scripts, mobile apps) are not affected by CORS.

### Admin-only routes in user mode

In `user` mode, routes that act on the whole database, the configuration or entity schemas require an admin session. Other users get `403`. In `basic` and `token` modes, these routes still accept any valid credentials. The same applies to TCP commands (see TCP Protocol).

| Method                 | Path                                           |
| ---------------------- | ---------------------------------------------- |
| `GET`, `PUT`, `DELETE` | `/kv/{key}`, `/kv/mget`                        |
| `POST`                 | `/save`, `/reset`                              |
| `GET`                  | `/stats`, `/config`, `/api/export`             |
| `POST`                 | `/api/import`, `/api/<entity>/migrate`         |
| `POST`                 | `/api/<entity>/create`                         |
| `PUT`                  | `/api/<entity>/schema`                         |
| `DELETE`               | `/api/<entity>`                                |

Document routes (`/api/<entity>`, `/api/<entity>/<id>`, `/api/query`, transactions) are checked per document with ACLs instead.

---

## User Management API

The User Management API allows you to manage application users when authentication is enabled in `user` mode. These endpoints are primarily intended for the Admin UI but can also be consumed programmatically.

All endpoints except login are protected and require the session of an **admin** user. Login is also limited to admin users: regular users cannot open a session in `user` mode.

---

## Authentication Requirements

These endpoints are available only if:

```yaml
security:
  authentication:
    enabled: true
    mode: "user"
```

Requests must include a valid session cookie obtained via the Admin UI login flow.

---

## Endpoints

### List Users

```
GET /api/security/user
```

Returns the list of all users.

**Authorization**

* Admin only

**Response** `200 OK`

```json
{
  "users": [
    { "username": "admin", "role": "admin" },
    { "username": "john", "role": "user" }
  ]
}
```

**Errors**

* `403 Forbidden` if the current user is not an admin

---

### Get User by Username

```
GET /api/security/user/{user_name}
```

Returns a single user.

**Authorization**

* Admin only

**Response** `200 OK`

```json
{
  "username": "john",
  "role": "user"
}
```

**Errors**

* `403 Forbidden` if access is not allowed
* `404 Not Found` if the user does not exist

---

### Create User

```
POST /api/security/user
```

Creates a new user.

**Authorization**

* Admin only

**Request Body**

```json
{
  "username": "alice",
  "password": "secret",
  "role": "user"
}
```

**Response** `200 OK`

**Errors**

* `403 Forbidden` if the current user is not an admin
* `400 Bad Request` if the body is invalid
* `404 Not Found` if creation fails

---

### Change User Role

```
PUT /api/security/user/{user_name}/role
```

Changes the role of a user.

**Authorization**

* Admin can change any role, including their own

**Request Body**

```json
{
  "role": "user" // or admin
}
```

**Response** `200 OK`

**Errors**

* `403 Forbidden` if access is not allowed
* `400 Bad Request` if the body is invalid or the role is not `admin` or `user`
* `404 Not Found` if the user does not exist
* `409 Conflict` if the change would leave no admin user (`{"error":"at least one admin user must remain"}`)

---

### Delete User

```
DELETE /api/security/user/{user_name}
```

Deletes a user.

**Authorization**

* Admin can delete any user, except the default `admin` user and the last admin user

**Response** `200 OK`

**Errors**

* `403 Forbidden` if access is not allowed, or for the default `admin` user (`{"error":"the default admin user cannot be deleted"}`)
* `409 Conflict` for the last admin user (`{"error":"at least one admin user must remain"}`)

---

## Security Rules Summary

| Action                | Admin                                  | Regular User |
| --------------------- | -------------------------------------- | ------------ |
| List users            | Yes                                    | No           |
| View another user     | Yes                                    | No           |
| View self             | Yes                                    | No           |
| Create user           | Yes                                    | No           |
| Change own password   | Yes                                    | No           |
| Change other password | Yes                                    | No           |
| Change a role         | Yes, if an admin remains               | No           |
| Delete user           | Yes, except `admin` and the last admin | No           |

Accounts, ACLs and hooks have no ACL of their own: every admin can manage all of them.

---

## Notes

* Passwords are never returned by the API
* Passwords are hashed using bcrypt with a server-side secret
* All responses are JSON
* All endpoints return the `X-Elysian-Version` header

---

## Access Control Lists (ACL)

ElysianDB provides a built-in ACL system to control access to entities at the API level.

### Activation

ACLs are enforced only when authentication is enabled and running in `user` mode:

```yaml
security:
  authentication:
    enabled: true
    mode: "user"
```

If authentication is disabled, all operations are allowed.

---

### Core Principle

Permissions are evaluated per **(user, entity)** pair.

Each ACL entry defines which actions a given user can perform on a specific entity.
If no ACL exists for a user/entity pair, access is denied by default.

---

### Permissions

Global permissions:

* `create`
* `read`
* `update`
* `delete`

Owning permissions (apply only to documents owned by the user):

* `owning_read`
* `owning_update`
* `owning_delete`

The ACL API also returns `owning_write`, granted by default. It is kept for compatibility and is not evaluated: creating documents requires `create`.

Ownership is determined using the internal field:

```
_elysiandb_core_username
```

In `user` mode, ElysianDB sets this field to the current user when a document is created (REST or transaction), whatever the payload contains. It is returned with the document in API responses, so clients can show who owns a document, and it is read-only for users that only have owning permissions.

---

### Evaluation Rules

* **Create**: allowed if `create` is granted

  * a document sent with the `id` of an existing document replaces it, so the update rules below also apply
  * the owner is always the current user, whatever the payload contains
  * for an entity type that does not exist yet, the user's role defaults apply: admins can create it, regular users cannot
* **Read**:

  * allowed if `read`
  * otherwise allowed only if `owning_read` and `_elysiandb_core_username == current user`
* **Update**:

  * allowed if `update`
  * otherwise allowed only if `owning_update` and the stored document belongs to the current user; the owner field cannot be changed
* **Delete**:

  * allowed if `delete`
  * otherwise allowed only if `owning_delete` and ownership matches

For list queries, results are automatically filtered to only include readable documents.

---

### Default Permissions

* **Admin**: all permissions granted
* **Regular user**: only owning permissions granted

ACLs are generated automatically for all users and entities (after a REST create, an entity type creation or a transaction commit) and can be reset to their default state at any time.

Core entities (`_elysiandb_core_user`, `_elysiandb_core_acl`, `_elysiandb_core_hook`, `_elysiandb_core_schema`) have no ACL. `PUT /api/acl/<user_name>/<entity>` and `PUT /api/acl/<user_name>/<entity>/default` on them return `400 {"error":"core entities have no ACL: users, ACLs and hooks are managed by admin users only"}`: an admin cannot be restricted on accounts, ACLs or hooks.

---

### Security Model

* Default deny if no ACL is found
* No implicit access
* Ownership checks are enforced at read, update, and delete time
* ACL logic applies uniformly across REST and transactional operations


---

## Admin UI

ElysianDB includes an optional, built‑in **Admin UI** that allows you to visually inspect, browse, and manage your data, entity types, schemas, and configuration.

The Admin UI is served directly by the ElysianDB HTTP server and requires no external frontend stack once built.

By default, there is an admin user created with username = "admin" and password = "admin". Please connect to the AdminUI and change the password.

---

## Enabling the Admin UI

To activate the Admin UI, make sure the following configuration is set in your `elysian.yaml`:

```yaml
adminui:
  enabled: true

security:
  authentication:
    enabled: true
    mode: "user"   # required for Admin UI
```

The Admin UI **requires** authentication to be enabled and must run in `user` mode.

---

## ️ Building the Admin UI

Before starting the server, build the UI assets using:

```bash
make build-admin
```

This compiles the React Admin interface and embeds the generated assets into the Go binary.

Whenever you change the Admin UI frontend code, rebuild it and restart the server.

If the binary was built without the UI assets, `/admin` answers `404` and a warning is printed at startup when `adminui.enabled` is true. The official Docker image always contains the Admin UI (see Docker).

---

## Accessing the Admin UI

Once enabled and built, start ElysianDB normally:

```bash
elysiandb server
```

Then open:

```
http://localhost:8089/admin
```

You will be prompted to log in with your configured credentials. The Admin UI provides:

* A dashboard showing instance information
* Entity type listing and creation
* Schema browsing and editing
* Data browsing with pagination, filters, and search
* Automatic forms for CRUD operations
* Live introspection of your datastore

---

## Languages

The Admin UI is available in **English** and **French**.

On the first visit, it follows the browser language and falls back to English when that language is not supported. The language can be changed at any time from the selector on the login page or in the navigation bar, and the choice is remembered by the browser.

Translations live in `adminui/src/i18n/locales/`, with one JSON file per language. To add a language, add its file there, register it in `adminui/src/i18n/translate.js` and `adminui/src/i18n/languages.js`, then run the Admin UI tests, which check that every language defines exactly the same keys and placeholders as English.

---

## Authentication Requirements

The Admin UI only works when:

* `security.authentication.enabled = true`
* `security.authentication.mode = "user"`

This mode uses a **session-based login** dedicated to the Admin UI.

---

## Development Workflow

For local development:

```bash
# Rebuild the UI
make build-admin

# Run server
elysiandb server
```

If you wish to work on the frontend separately, you can use the Vite dev server, but the Admin UI in production always uses the embedded build.

Run the Admin UI tests with:

```bash
cd adminui && npm test
```

---

## Summary

To enable and test the Admin UI:

1. Set in `elysian.yaml`:

   * `adminui.enabled = true`
   * `security.authentication.enabled = true`
   * `security.authentication.mode = "user"`
2. Run `make build-admin`
3. Start the server
4. Visit `http://localhost:8089/admin`

The Admin UI gives you a full graphical interface to interact with ElysianDB without writing any API calls manually.

---

## Hooks

ElysianDB provides a built-in **hooks system** that allows you to execute custom logic on entities during their lifecycle. Hooks are designed to enrich, transform, or filter data dynamically without modifying stored documents.

Hooks are supported on **read operations** and can run at two different stages: **before filtering (`pre_read`)** and **after loading (`post_read`)**.

---

### Activation

Hooks are disabled by default and must be explicitly enabled in the configuration:

```yaml
api:
  hooks:
    enabled: true
```

When disabled, hooks are completely bypassed and introduce zero overhead.

---

### Core Concepts

* Hooks are stored as a **core entity** (`_elysiandb_core_hook`)
* Hooks are scoped per **entity type**
* Multiple hooks can exist for the same entity and event
* Hooks are executed **in priority order (highest first)**
* Hooks can be individually enabled or disabled
* Hooks may optionally **bypass ACL checks** when querying other entities

Hooks do **not** persist changes back to storage. They only affect query processing or the API response.

---

### Supported Events

| Event       | Description                                                                  |
| ----------- | ---------------------------------------------------------------------------- |
| `pre_read`  | Executed after initial filtering and before final in-memory filtering        |
| `post_read` | Executed after an entity or list item is fully loaded and ready for response |

---

### Hook Execution Model

Hooks are executed using an embedded JavaScript runtime (Goja).
Each hook must define a function matching the event name.

#### `pre_read`

```javascript
function preRead(ctx) {
  return ctx.entity
}
```

`pre_read` hooks are executed **per entity** after the initial query has been resolved. They may add or modify **virtual properties** which can then be used by a second filtering pass.

#### `post_read`

```javascript
function postRead(ctx) {
  return ctx.entity
}
```

`post_read` hooks are executed after all filtering has completed and just before the response is returned.

The return value is ignored in both cases; mutations are applied directly on `ctx.entity`.

---

### Hook Context (`ctx`)

The `ctx` object exposes:

| Property                 | Description                               |
| ------------------------ | ----------------------------------------- |
| `entity`                 | The current entity object being processed |
| `query(entity, filters)` | Query another entity programmatically     |

---

### Example: Virtual Filtering with `pre_read`

```javascript
function preRead(ctx) {
  ctx.entity.isLate = ctx.entity.dueDate < new Date().toISOString()
  return ctx.entity
}
```

```http
GET /api/task?filter[isLate][eq]=true
```

The filter is ignored during the initial query and applied after the `pre_read` hook has materialized the virtual field.

---

### Example: Enrichment with `post_read`

```javascript
function postRead(ctx) {
  const orders = ctx.query("order", {
    userId: { eq: ctx.entity.id }
  })

  ctx.entity.ordersCount = orders.length
  return ctx.entity
}
```

If `bypass_acl` is disabled, results returned by `query` are filtered using ACL rules.

---

### Priority and Ordering

Each hook defines a numeric `priority`.
Hooks with higher priority values are executed first.

This allows predictable composition of multiple hooks on the same entity and event.

---

### Behavior on Lists

When listing entities (`GET /api/<entity>`) or querying them (`POST /api/query`):

* Initial filtering is applied using stored fields only (the Query API loads every document of the entity)
* `pre_read` hooks are executed for each item
* Filters are re-applied, including those targeting virtual fields
* `offset` and `limit` are applied to the filtered result, so a page always contains the matching documents
* `post_read` hooks are executed for each remaining item

---

### Caching Interaction

When hooks are enabled for an entity:

* REST API caching is automatically bypassed for that entity
* Responses are always computed dynamically

This guarantees hook correctness at the cost of caching for affected entities only.

---

### API Endpoints (Admin Only)

All hook management endpoints require **admin privileges**:

* In `user` mode, requests must include the session cookie of an admin user (obtained via `POST /api/security/login`)
* In `token` mode, requests must include the instance token (`Authorization: Bearer <token>`)
* In `basic` mode, requests must include valid Basic credentials
* With authentication disabled, the endpoints are open like every other route

| Method | Endpoint                  | Description              |
| ------ | ------------------------- | ------------------------ |
| GET    | `/api/hook/{entity}`      | List hooks for an entity |
| GET    | `/api/hook/id/{id}`       | Retrieve a hook by ID    |
| POST   | `/api/hook/{entity}`      | Create a new hook        |
| PUT    | `/api/hook/id/{id}`       | Update an existing hook  |
| DELETE | `/api/hook/{entity}/{id}` | Delete a hook            |

`PUT /api/hook/id/{id}` is a partial update: fields absent from the body keep their stored value, so `{"enabled": false}` only disables the hook. An empty `script` is replaced with the default script of the event. The response is the full updated hook.

* `404 {"error":"hook not found"}` if the hook does not exist
* `400` if the update leaves `name`, `entity`, `event`, `language` or `script` empty

---

### Example: Creating a Hook

```bash
curl -X POST http://localhost:8089/api/hook/book \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "filterLateBooks",
    "event": "pre_read",
    "language": "javascript",
    "priority": 10,
    "enabled": true,
    "bypass_acl": true,
    "script": "function preRead(ctx) { ctx.entity.isLate = ctx.entity.year < 2000 }"
  }'
```

---

### Use Cases

Typical use cases for hooks include:

* Computing derived or aggregated fields
* Filtering on virtual or computed properties
* Enriching responses with related data
* Implementing read-time projections
* Applying business rules without mutating stored data

Hooks provide a powerful and controlled extension point while keeping the core datastore immutable and predictable.


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

Checks all queued operations, then executes them in order. If any write validation fails or an update targets a non-existing document, the commit aborts, returns `400` with `{"error":"validation error"}` or `{"error":"update failed"}`, and no operation is applied.

#### Rollback

```
POST /api/tx/{txId}/rollback
```

Discards all pending operations.

### Isolation and Behavior

Transactions are isolated from the live datastore until committed. Operations accumulated inside a transaction do not affect the current state and cannot be observed by other requests.

Before writing anything, a commit checks the whole sequence of operations against the current data:

* each update must target a document that exists at that point of the sequence, taking the earlier writes and deletes of the transaction into account (an update after a delete of the same document fails)
* each write must pass schema validation, taking into account the schema changes made by the earlier operations of the transaction
* sub-entities created by nested writes count as existing documents for the next operations

If any check fails, the transaction is aborted, and no changes are applied. Otherwise the operations are applied sequentially:

* Write operations call the standard entity creation logic, including schema validation and sub-entity processing.
* Update operations apply partial updates to the existing document.
* Delete operations remove the targeted entity.

Commits run one at a time. Requests outside transactions are not blocked during a commit.

### Error Handling

* Unknown transaction identifiers return an error for commit or when retrieving the transaction.
* Write validation errors cause commit to fail, without applying any operation.
* Updates on missing entities cause commit to fail, without applying any operation, including when the entity was deleted by another request after the update was queued.
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
| `GET`    | `/config`                 | Return the configuration, with secrets redacted |
| `GET`    | `/health`                 | Liveness check, no credentials needed |

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
AUTH <credentials>  → OK or ERR invalid credentials
PING                → PONG
SET <key> <value>   → OK
SET TTL=10 <key> <value> → OK (with expiration)
GET <key>           → value
MGET key1 key2 ...  → multiple values
DEL <key>           → Deleted 1
RESET               → OK (clears store)
SAVE                → OK (flush to disk)
EXIT                → Goodbye! (closes the connection)
```

### Authentication

When `security.authentication.enabled` is true, every command except `PING`, `EXIT` and `AUTH` answers `ERR auth required` until the connection authenticates with `AUTH`. Authentication lasts for the connection.

| Mode    | Command                       | Accepted credentials                 |
| ------- | ----------------------------- | ------------------------------------ |
| `token` | `AUTH <token>`                | The instance token                   |
| `basic` | `AUTH <username> <password>`  | Any user                             |
| `user`  | `AUTH <username> <password>`  | Admin users only, like `/kv` over HTTP |

The password is everything after the first space, so it may contain spaces. With authentication disabled, `AUTH` answers `OK` and is not needed.

The KV store also holds the REST API indexes and the list of entity types: a `RESET` or `DEL` on internal keys breaks the REST API. Keep the TCP port private (bind it to `127.0.0.1` or disable `server.tcp`) unless clients need it.

### Example (telnet)

```bash
telnet localhost 8088
AUTH my-secret-token
SET foo bar
GET foo
PING
```

The TCP benchmark accepts the same credentials: `./elysian_bench -addr 127.0.0.1:8088 -auth my-secret-token`.

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
| `version`               | Version of the running binary   |

The counters are JSON **strings** containing integers (`"1203"`, not `1203`): parse them as numbers on the client side.

Example output:

```json
{
  "keys_count": "1203",
  "expiration_keys_count": "87",
  "uptime_seconds": "3605",
  "total_requests": "184467",
  "hits": "160002",
  "misses": "24465",
  "version": "0.1.14",
  "entities_count": "11062"
}
```

---

## Development

```bash
make build-admin   # optional: embeds the Admin UI
make build && ./elysiandb server
# or
make server
```

### Versioning

The version reported by `X-Elysian-Version` and `/stats` is set at build time. `make build`, `make install` and `make server` use `git describe --tags --always --dirty` (for example `0.1.14` on a release tag, `0.1.14-3-g244e812` three commits later). A plain `go build` reports `dev`. To set it yourself:

```bash
go build -ldflags "-X github.com/taymour/elysiandb/internal/globals.VERSION=1.2.3" -o elysiandb .
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

### 2. `create-user` (basic or user authentication only)

Creates a new user when the authentication mode is set to `basic`.

```bash
elysiandb create-user
```

The command interactively prompts for:

* username
* password

The user is stored in a core entity

---

### 3. `delete-user` (basic or user authentication only)

Deletes an existing user.

```bash
elysiandb delete-user
```

The command prompts for the username to remove. The default `admin` user and the last admin user cannot be deleted.

---

### 4. `change-password` (basic or user authentication only)

Changes the password of an existing user.

```bash
elysiandb change-password
```

The command prompts for the username and the new password. If the user does not exist, an error is returned.

---

### 5. `help`

Deletes an existing user.

```bash
elysiandb help
```

The command prompts the list fo available commands.

---

### 6. `reset`

Reset ElysianDB completely including schemas and users.

```bash
elysiandb reset
```

---

## Requirements

Both `create-user` and `delete-user` require authentication in `basic` or `user` mode:

```yaml
security:
  authentication:
    enabled: true
    mode: basic # or user
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

The image uses `docker/elysian.yaml`, with authentication and the Admin UI disabled. Mount your own configuration to change it:

```bash
docker run -d --name elysiandb \
  -p 8089:8089 \
  -v elysian-data:/data \
  -v $(pwd)/elysian.yaml:/app/elysian.yaml:ro \
  taymour/elysiandb:latest
```

* The image contains the Admin UI: set `adminui.enabled: true` and `security.authentication.mode: user` to serve it at `/admin`.
* The `HEALTHCHECK` calls `/health`, which works in every authentication mode.
* Images report the version computed by `git describe` at build time. To build one yourself: `docker build --build-arg VERSION=$(git describe --tags --always) -t elysiandb .`
