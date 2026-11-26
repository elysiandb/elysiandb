tcp_benchmark:
	cargo run --release --locked --bin elysian_bench -- -addr 127.0.0.1:8088   -vus 500 -duration 20s -keys 20000 -payload 16 -pair
http_benchmark:
	BASE_URL=http://localhost:8089 KEYS=5000 VUS=200 DURATION=30s k6 run elysian_k6.js
api_benchmark:
	BASE_URL=http://localhost:8089 KEYS=5000 VUS=200 DURATION=30s k6 run elysian_api_k6.js

VERSION ?= $(or $(if $(wildcard .git),$(shell git describe --tags --always --dirty 2>/dev/null)),dev)
CARGO_TARGET_DIR ?= target
COVERAGE_SKIPS = $(shell python3 .github/ci/tests.py skip-args)
UNINSTRUMENTED_TESTS = $(shell python3 .github/ci/tests.py filter-args)

.PHONY: test test-e2e-http test-unit test-cover test-cover-group test-uninstrumented build

test:
	cargo test --workspace --locked --release -- --test-threads=1

test-e2e-http:
	cargo test --locked --release --test e2e_http -- --test-threads=1

test-unit:
	cargo test --workspace --locked --release --lib --bins

test-cover:
	@python3 .github/ci/tests.py skip-args > /dev/null
	cargo llvm-cov --workspace --locked --release --no-fail-fast --lcov --output-path lcov.info -- --test-threads=1 $(COVERAGE_SKIPS) && cargo llvm-cov report --summary-only

test-cover-group:
	@python3 .github/ci/tests.py targets "$(GROUP)" > /dev/null
	cargo llvm-cov --workspace --locked --release --no-fail-fast --lcov --output-path lcov.info $(shell python3 .github/ci/tests.py targets "$(GROUP)") -- --test-threads=1 $(COVERAGE_SKIPS) && cargo llvm-cov report --summary-only

test-uninstrumented:
	@python3 .github/ci/tests.py filter-args > /dev/null
	cargo test --workspace --locked --release --lib --bins --tests --no-fail-fast -- --test-threads=1 $(UNINSTRUMENTED_TESTS)

build:
	ELYSIANDB_VERSION="$(VERSION)" cargo build --release --locked --bin elysiandb && cp "$(CARGO_TARGET_DIR)/release/elysiandb" ./elysiandb

install:
	rm -f /usr/local/bin/elysiandb
	ELYSIANDB_VERSION="$(VERSION)" cargo build --release --locked --bin elysiandb && cp "$(CARGO_TARGET_DIR)/release/elysiandb" /usr/local/bin/elysiandb

server:
	ELYSIANDB_VERSION="$(VERSION)" cargo run --release --locked --bin elysiandb -- server

run: build-admin server

build-admin:
	cd adminui && npm install && npm run build

watch-admin:
	cd adminui && npm run build:watch
