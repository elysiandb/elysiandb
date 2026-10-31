tcp_benchmark:
	./elysian_bench -addr 127.0.0.1:8088   -vus 500 -duration 20s -keys 20000 -payload 16 -pair
http_benchmark:
	BASE_URL=http://localhost:8089 KEYS=5000 VUS=200 DURATION=30s k6 run elysian_k6.js
api_benchmark:
	BASE_URL=http://localhost:8089 KEYS=5000 VUS=200 DURATION=30s k6 run elysian_api_k6.js

COVERPKG := $(shell go list ./... | tr '\n' ',' | sed 's/,$$//')

VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null | sed 's/^v//')
LDFLAGS := -X github.com/taymour/elysiandb/internal/globals.VERSION=$(VERSION)

.PHONY: test test-e2e-http test-unit test-cover build

test:
	@pkgs=$$(go list -f '{{if or (len .TestGoFiles) (len .XTestGoFiles)}}{{.ImportPath}}{{end}}' ./... | grep -v '^$$'); \
	if [ -z "$$pkgs" ]; then echo "no test packages"; exit 0; fi; \
	go test $$pkgs -v -race -count=1

test-e2e-http:
	go test ./test/e2e/http -v -race -count=1

test-unit:
	@pkgs=$$(go list ./... | grep -v '^github.com/[^/]*/[^/]*/test/'); \
	if [ -z "$$pkgs" ]; then echo "no unit test packages"; exit 0; fi; \
	go test $$pkgs -v -race -count=1

test-cover:
	@pkgs=$$(go list -f '{{if or (len .TestGoFiles) (len .XTestGoFiles)}}{{.ImportPath}}{{end}}' ./... | grep -v '^$$'); \
	if [ -z "$$pkgs" ]; then echo "no test packages"; exit 0; fi; \
	go test $$pkgs -race -coverprofile=coverage.out -coverpkg=$(COVERPKG) -count=1 && \
	go tool cover -func=coverage.out

build:
	go build -ldflags "$(LDFLAGS)" -o elysiandb .

install:
	rm -f /usr/local/bin/elysiandb
	go build -ldflags "$(LDFLAGS)" -o /usr/local/bin/elysiandb .

server:
	go run -ldflags "$(LDFLAGS)" . server

run: build-admin server

build-admin:
	cd adminui && npm install && npm run build

watch-admin:
	cd adminui && npm run build:watch
