FROM --platform=$BUILDPLATFORM node:22-alpine AS adminui
WORKDIR /src/adminui

COPY adminui/package.json adminui/package-lock.json ./
RUN npm ci

COPY adminui/ ./
RUN npm run build

FROM golang:1.24-alpine AS builder
WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .
COPY --from=adminui /src/adminui/dist ./adminui/dist

ARG TARGETOS
ARG TARGETARCH
ARG VERSION=dev
RUN CGO_ENABLED=0 GOOS=${TARGETOS:-linux} GOARCH=${TARGETARCH:-amd64} \
    go build -trimpath \
    -ldflags="-s -w -X github.com/taymour/elysiandb/internal/globals.VERSION=${VERSION#v}" \
    -o /out/elysiandb .

FROM alpine:3.20

RUN addgroup -S app && adduser -S app -G app \
 && mkdir -p /data /app \
 && chown -R app:app /data /app

WORKDIR /app

COPY --from=builder /out/elysiandb /usr/local/bin/elysiandb

COPY docker/elysian.yaml /app/elysian.yaml

USER app
EXPOSE 8089

HEALTHCHECK --interval=10s --timeout=2s --retries=3 CMD wget -qO- http://127.0.0.1:8089/health || exit 1

ENTRYPOINT ["/usr/local/bin/elysiandb"]
