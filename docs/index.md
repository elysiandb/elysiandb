# ElysianDB Documentation

ElysianDB turns raw JSON into a complete backend instantly.
With a built-in datastore, auto-generated REST API, and optional KV/TCP interfaces, it gives you a database and a backend in a single, ultra-fast Go binary.

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
| **server.tcp**                       | Enables and configures the TCP text protocol interface                                |
| **log.flushIntervalSeconds**         | Interval for flushing in-memory logs                                                  |
| **stats.enabled**                    | Enables runtime metrics and `/stats` endpoint                                         |
| **api.index.workers**                | Number of workers that rebuild dirty indexes                                          |
| **api.cache.enabled**                | Enables REST API caching for repeated queries                                         |
| **api.schema.enabled**               | Enables automatic schema inference and validation                                     |
| **api.schema.strict**                | If true and schema is manual, new fields are rejected and deep validation is enforced |
| **api.cache.cleanupIntervalSeconds** | Interval for cache expiration cleanup                                                 |
| **security.authentication.enabled**  | Enables authentication layer for all endpoints                                        |
| **security.authentication.mode**     | Authentication mode (currently supports `basic`, `token` `user`)                      |
| **adminui.enabled**                  | Enables the admin web interface                                                       |

---

## REST API

ElysianDB automatically exposes REST endpoints per entity. Entities are inferred from the URL.
(All HTTP requests return a `X-Elysian-Version`)
For some requests, there is a `X-Elysian-Cache` header with values : `HIT` or `MISS`

### CRUD Operations

| Method   | Endpoint                                  | Description                                                 |
| -------- | ----------------------------------------- | ----------------------------------------------------------- |
| `POST`   | `/api/<entity>/create`                    | Create a new type of entity with schema                     |
| `POST`   | `/api/<entity>`                           | Create one or multiple JSON documents (auto‑ID if missing)  |
| `GET`    | `/api/<entity>`                           | List all documents, supports pagination, sorting, filtering |
| `GET`    | `/api/<entity>/schema`                    | Schema for entity                                           |
| `PUT`    | `/api/<entity>/schema`                    | Update schema for entity                                    |
| `GET`    | `/api/<entity>/<id>`                      | Retrieve document by ID                                     |
| `PUT`    | `/api/<entity>/<id>`                      | Update a single document by ID                              |
| `PUT`    | `/api/<entity>`                           | Update multiple documents (batch update)                    |
| `DELETE` | `/api/<entity>/<id>`                      | Delete document by ID                                       |
| `DELETE` | `/api/<entity>`                           | Delete all documents for an entity                          |
| `GET`    | `/api/export`                             | Dumps all entities as a JSON object                         |
| `POST`   | `/api/import`                             | Imports all objects from a JSON dump                        |
| `POST`   | `/api/<entity>/migrate`                   | Run a **migration** across all documents for an entity      |
| `GET`    | `/api/<entity>/count`                     | Counts all documents for an entity                          |
| `GET`    | `/api/<entity>/<id>/exists`               | Verifiy if an entity exists                                 |
| `GET`    | `/api/entity/types`                       | List of all entity types                                    |
| `POST`   | `/api/<entity>/schema`                    | Create a new entity type                                    |
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

ElysianDB supports optional authentication to protect all REST and KV endpoints. Three modes are available: `basic`, `user` and `token`.
When you boot Elysiandb, a default user (username: "admin", password: "admin") is created if it does not exist.

### Configuration

Enable authentication in your `elysian.yaml`:

```yaml
authentication:
  enabled: true
  mode: basic   # basic, token or user
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

Requests without authentication headers are rejected when authentication is enabled.

---

## Security Notes

* Passwords are never stored in plaintext
* Hashing is done using bcrypt
* A per-instance secret key is used as part of the hashing process
* Deleting `users.key` invalidates all stored password hashes
* In `token` mode, anyone with the token can fully access the API

---

## User Management API

The User Management API allows you to manage application users when authentication is enabled in `user` mode. These endpoints are primarily intended for the Admin UI but can also be consumed programmatically.

All endpoints are protected and require a valid **user session**. Most operations require **admin privileges**.

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

* Admin can access any user
* A user can access any user **except themselves**

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

* Admin can change any role

**Request Body**

```json
{
  "role": "user" // or admin
}
```

**Response** `200 OK`

**Errors**

* `403 Forbidden` if access is not allowed
* `400 Bad Request` if the body is invalid
* `404 Not Found` if the user does not exist

---

### Delete User

```
DELETE /api/security/user/{user_name}
```

Deletes a user.

**Authorization**

* Admin can delete any user
* A user can delete another user, but not themselves

The default `admin` user cannot be deleted.

**Response** `200 OK`

**Errors**

* `403 Forbidden` if access is not allowed

---

## Security Rules Summary

| Action                | Admin | Regular User      |
| --------------------- | ----- | ----------------- |
| List users            | Yes   | No                |
| View another user     | Yes   | Yes               |
| View self             | Yes   | No                |
| Create user           | Yes   | No                |
| Change own password   | No    | No                |
| Change other password | Yes   | Yes               |
| Delete user           | Yes   | Yes (except self) |

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

Ownership is determined using the internal field:

```
_core_username
```

---

### Evaluation Rules

* **Create**: allowed if `create` is granted
* **Read**:

  * allowed if `read`
  * otherwise allowed only if `owning_read` and `_core_username == current user`
* **Update**:

  * allowed if `update`
  * otherwise allowed only if `owning_update` and ownership matches
* **Delete**:

  * allowed if `delete`
  * otherwise allowed only if `owning_delete` and ownership matches

For list queries, results are automatically filtered to only include readable documents.

---

### Default Permissions

* **Admin**: all permissions granted
* **Regular user**: only owning permissions granted

ACLs are generated automatically for all users and entities and can be reset to their default state at any time.

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

```yaml\adminui:
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

When listing entities (`GET /api/<entity>`):

* Initial filtering is applied using stored fields only
* `pre_read` hooks are executed for each item
* Filters are re-applied, including those targeting virtual fields
* `post_read` hooks are executed for each remaining item

---

### Caching Interaction

When hooks are enabled for an entity:

* REST API caching is automatically bypassed for that entity
* Responses are always computed dynamically

This guarantees hook correctness at the cost of caching for affected entities only.

---

### API Endpoints (Admin Only)

All hook management endpoints require **admin privileges**.

| Method | Endpoint             | Description              |
| ------ | -------------------- | ------------------------ |
| GET    | `/api/hook/{entity}` | List hooks for an entity |
| GET    | `/api/hook/id/{id}`  | Retrieve a hook by ID    |
| POST   | `/api/hook/{entity}` | Create a new hook        |
| PUT    | `/api/hook/id/{id}`  | Update an existing hook  |
| DELETE | `/api/hook/id/{id}`  | Delete a hook            |

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

The command prompts for the username to remove. If the user does not exist, an error is returned.

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
