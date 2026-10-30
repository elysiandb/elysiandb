# Contributing to ElysianDB

Thanks for your interest in ElysianDB! This project aims to be a **lightweight, fast-first in‑memory key–value store** with **simple persistence** and **two protocols** (HTTP + a tiny TCP text protocol). Contributions are welcome as long as we keep things **simple, predictable, and efficient**.
Any new idea is welcome !
Please open an issue first so we can discuss it together.

## Project Scope & Principles

* **Keep it light:** minimal dependencies, small surface area.
* **Fast by default:** low‑latency hot paths; prefer clarity + perf over feature bloat.
* **KV core:** strict **key → value (\[]byte)** model.
* **Data structures can evolve *only* if they stay simple** (e.g., small value helpers or minimal indexing) and do not complicate hot paths.
* **Protocols are first‑class:** both **HTTP** and **TCP** must remain supported and easy to use.
* **Operational simplicity:** easy config, sane defaults, Docker‑friendly, good docs.

### Non‑Goals (for now)

* Complex multi‑model features (hashes/lists/streams, etc.) unless they can be layered without bloating the core.
* Distributed clustering/replication (single‑node simplicity first).

---

## How You Can Help

* **Docs & Examples:** improve README, configuration docs, quick starts, troubleshooting.
* **Configuration UX:** validate config (e.g., shard count power‑of‑two), clearer errors.
* **Testing:** add e2e tests for HTTP/TCP (GET 404, DELETE, RESET, SAVE/persistence); race tests.
* **Performance & Memory:** safe micro‑optimizations (batching TCP flushes, reduce allocs/GC); keep code readable.
* **Developer Experience:** Makefile tasks, CI polish, linters.
* **SDK:** Provide SDK's for various languages

---

## Dev Setup

1. **Go 1.23+** installed.
2. Clone and install deps:

   ```bash
   git clone https://github.com/taymour/elysiandb
   cd elysiandb
   go mod tidy
   ```
3. Run locally:

   ```bash
   go run elysiandb.go
   ```
4. Run tests (race detector):

   ```bash
   make test
   ```

   Tests that exercise the MongoDB engine need a MongoDB server (4.4 or newer). Without one they are skipped locally, while CI always runs them:

   ```bash
   docker run -d --name elysiandb-test-mongo -p 27017:27017 mongo:7
   ELYSIANDB_TEST_MONGODB_URI=mongodb://localhost:27017 make test
   ```

   Each test uses its own throwaway database and drops it afterwards.
5. Bench (optional): see README (k6 for HTTP, built‑in TCP bench).

---

## Testing Guidelines

* Prefer **e2e tests** for the HTTP API first; TCP tests welcome too.
* Keep tests **deterministic**; reset state via `POST /reset` where needed.
* Behavior visible through the REST API must be identical for both engines: add scenarios to `test/e2e/engines`, which replays each one against `internal` and `mongodb` and compares every response.
* Run with `-race` and ensure no data races are introduced.

---

## Coding Guidelines

* Favor clarity over cleverness in hot paths.
* Keep allocations in check (copy only when necessary; reuse buffers where reasonable).
* Logging should never block the hot path; avoid noisy logs in critical sections.
* For concurrency: prefer simple patterns (sharding + fine‑grained locks). Avoid global state where possible.

---

## Git & PR Workflow

1. Fork → branch from `main`.
2. Keep PRs **small and focused**; open an issue first for larger changes.
3. Make sure CI passes (`make test`).
4. Add/adjust tests when changing behavior.
5. PR title should be clear

**Commit style:** no strict convention required; concise, imperative present is appreciated (e.g., `storage: avoid extra copy in put`).

---

## Documentation Changes

* Keep README concise; move longer guides to `docs/`.
* When adding config options, update README + examples.

---

## Security

If you find a vulnerability, please open a private discussion or contact the maintainer rather than filing a public issue first.

---

## License

By contributing, you agree that your contributions are licensed under the repository’s MIT license.

---

## Quick Checklist (before opening a PR)

* [ ] Code is formatted and idiomatic.
* [ ] Tests pass locally with `-race`.
* [ ] Added/updated tests for behavior changes.
* [ ] Docs/config updated if needed.
* [ ] Changes keep the project **simple, fast, and lightweight**.

