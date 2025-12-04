FROM golang:1.24-alpine AS builder
WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .

ARG TARGETOS
ARG TARGETARCH
RUN CGO_ENABLED=0 GOOS=${TARGETOS:-linux} GOARCH=${TARGETARCH:-amd64} \
    go build -trimpath -ldflags="-s -w" -o /out/elysiandb .

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
