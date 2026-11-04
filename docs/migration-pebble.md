# Migrating to the Pebble storage

ElysianDB now stores its data with [Pebble](https://github.com/cockroachdb/pebble), the embedded key-value store of CockroachDB. This page explains what changes, what does not, and the one case that needs an action: the removal of the MongoDB engine.

## In short

* **Users of the `internal` engine (the default) have nothing to do.** Keep the same configuration file, the same data folder and the same clients. At the first start, ElysianDB migrates the data folder by itself, checks every document, and keeps the previous files aside.
* **The REST API, the Query API, the TCP protocol, KV keys and TTLs, transactions, hooks, schemas, ACLs, accounts, the cache, `/stats`, the monitoring, the Admin UI, the commands and the backup format do not change.** Requests and answers stay byte for byte the same.
* **The `mongodb` engine was removed.** If you used it, move your data with a backup archive before upgrading (see below). It is the only change that needs an action.

Take a backup with `elysiandb backup` (or `GET /api/backup`) before upgrading, as for every upgrade.

## What changes under the hood

ElysianDB keeps its own engine, designed together with its API: entities in insertion order, an id directory, indexes for sorts, filters, references and search built when first needed, exact counts, snapshot reads and conflict checks on writes. What changes is the layer that writes to disk and manages memory:

| Before | Now |
| ------ | --- |
| 0.2.4: JSON files written in full at each flush, with recovery logs | Pebble: write-ahead log, memtables, sorted tables, background compaction, block cache |
| Earlier builds of this version: log segments (`log/`), trees of pages (`pages/`), checkpoints (`MANIFEST`, `kv-*.ckpt`) and a value cache of their own | The same Pebble store, in the `pebble/` sub-folder of `store.folder` |
| Indexes kept in memory or in pages, rebuilt after some restarts | Indexes stored in Pebble, kept across restarts, built in batches of bounded memory |
| A memory budget that bounded the cached documents and pages | A memory budget that bounds the whole process |

Pebble is a Go library: ElysianDB stays a single static binary (`CGO_ENABLED=0`), with no service to run next to it. The Docker image, the `/data` volume and `elysiandb.lock` stay the same, and `users.key` and `sessions.json` keep their place and their format.

The engine stores its model in families of Pebble keys: one record per entity (exact count, references, index states), the documents of each entity in insertion order, the id directory, the entries of the sort, filter and reference indexes, the lists of the search index, the KV keys and their expiration dates, and the list of entity types. A request, a transaction or a batch of documents is one atomic Pebble batch. See Storage engine internals in the [documentation](index.md).

## Upgrading

### From 0.2.4

At the first start on a 0.2.4 data folder, ElysianDB:

1. checks that the disk has at least twice the size of the 0.2.4 files free;
2. reads them as 0.2.4 does, recovery logs included when `store.crashRecovery.enabled` is true;
3. writes everything into a new store in the `.migration` sub-folder, in the order of the 0.2.4 lists;
4. reads the new store back and compares every document, KV key and expiration date: any difference stops the start and leaves the folder untouched;
5. switches to `pebble/`, moves the 0.2.4 files to `legacy-0.2.4/` (never deleted) and logs `storage migrated`.

### From an earlier build of this version

Folders written by earlier builds of this version (`MANIFEST` of format 1 or 2, `log/`, `pages/`, `kv-*.ckpt`, `checkpoint-*.ckpt`, `legacy-checkpoint/`) are migrated the same way. ElysianDB reads the last checkpoint and replays the log written after it, exactly as these builds did after a crash, so a folder left by a `kill -9` loses nothing. It then writes the new store, compares every document, KV key and expiration date, switches to `pebble/` and moves the previous files to `legacy-log-pages/`. A `legacy-0.2.4/` folder left by an earlier migration stays where it is.

Indexes are not carried over: each one is built again, in the background, the first time a request needs it.

### After the migration

```
/data
├── elysiandb.lock
├── users.key
├── sessions.json
├── pebble/              the store
├── legacy-0.2.4/        files of 0.2.4, if the folder came from 0.2.4
└── legacy-log-pages/    files of an earlier build, if the folder came from one
```

The migration needs twice the size of the previous files on disk, and runs once. Its duration depends on the size of the data: every document is written and then read back. Once the new version runs and you no longer need to go back, you can delete `legacy-0.2.4/` and `legacy-log-pages/` yourself; ElysianDB never deletes them.

## Configuration

No key has to change. The keys of the previous storage are still read, and keep their meaning when Pebble has an equivalent:

| Key | Now |
| --- | --- |
| `store.folder` | Unchanged. The store lives in its `pebble/` sub-folder |
| `store.shards` | Number of locks of the KV keys |
| `store.flushIntervalSeconds` | Period at which the write-ahead log is synced to disk. `0` syncs it at `/save`, `SAVE`, shutdown and memtable flushes only |
| `store.crashRecovery.enabled` | `true`: a write is answered once it is in the write-ahead log, so it survives a `kill -9`. `false`: a write is answered as soon as it is queued for the log |
| `store.crashRecovery.maxLogMB` | Largest memtable, between 4 and 64 MB: it bounds the log replayed after a crash |
| `store.durability` | `fsync`: the write-ahead log is synced before each answer, per group of writes |
| `store.memory.limitMB` | Memory budget, now of the whole process (see below) |
| `api.index.workers` | Index builds that run at the same time |
| `engine.name` | `internal`, the default. `mongodb` stops the start (see below) |
| `engine.uri` | Ignored, with an `info` line at startup |

No new key is needed. At startup, the `storage settings` line shows how these keys were applied (durability, sync interval, memtable and block cache sizes, KV locks, index workers).

### Memory budget

`store.memory.limitMB`, or 75 % of the container memory limit when it is not set, now bounds the whole process, whatever the number of documents: 30 % of it goes to the block cache of Pebble, up to 1/16 to its memtables, index builds use batches of bounded size and run one entity at a time per index worker, and the rest is left to requests, hooks, the API cache and the garbage collector. When the process gets close to the budget, heavy requests wait for memory and index builds pause; when it reaches it, heavy requests are answered `503` `{"error":"server busy"}` with `Retry-After: 1`, as when the heavy lane is full, instead of letting the process grow until the system stops it. The new series `memory_budget_bytes`, `memory_live_bytes`, `memory_pressure` and `memory_refused_rps` show it.

## Going back

Backup archives keep format v1, readable in both directions.

* **With the writes made since the upgrade:** run `elysiandb export-legacy --output <empty folder>` while ElysianDB is stopped. It writes a 0.2.4 data folder, with `users.key` and `sessions.json`, that 0.2.4 and the earlier builds of this version both open (the earlier builds migrate it at their first start). Or take a backup archive and restore it with the previous version.
* **Without them:** stop ElysianDB, remove `pebble/`, move the files of `legacy-0.2.4/` (from 0.2.4) or of `legacy-log-pages/` (from an earlier build) back to the data folder, and start the previous version.

## The MongoDB engine was removed

The `mongodb` engine was in beta: it was not interchangeable with the `internal` engine, some features were missing or behaved differently, and part of its data still lived in the internal storage. With Pebble, the `internal` engine covers the needs it was meant for (large volumes, a bounded memory, durable writes) without a second database to run, so the MongoDB engine, its code, its tests and its dependencies were removed.

With `engine.name: mongodb`, ElysianDB now stops at startup with:

```text
level=ERROR msg="the mongodb engine was removed" component=boot engine=mongodb hint="set engine.name to internal and restore a backup taken with the previous version, see docs/migration-pebble.md"
```

To move your data, which is the only configuration change this upgrade requires:

1. **Before upgrading**, with the version you run today on MongoDB, take a backup archive: `GET /api/backup`, or `elysiandb backup --output /backups` while ElysianDB is stopped. The archive does not depend on the engine: it holds the documents, the accounts, the ACLs, the hooks, the schemas, the KV keys and `users.key`.
2. Upgrade ElysianDB, and in `elysian.yaml` set `engine.name: internal` (or remove the `engine` section) and remove `engine.uri`.
3. Point `store.folder` to an empty folder, and restore the archive: `elysiandb restore /backups/elysiandb-backup-<date>.tar.gz` while ElysianDB is stopped, or `POST /api/restore` once it runs.
4. Log in again with the accounts of the backup: sessions are not part of an archive.

The MongoDB database is not touched: delete it yourself once the new version runs. The monitoring keeps the `mongo_ping_ms`, `mongo_errors_rps` and `mongo_pool_in_use` series, always empty or `0`, so that existing dashboards keep working; the `mongodb_unreachable` problem can no longer occur.
