FROM --platform=$BUILDPLATFORM node:22-alpine AS adminui
WORKDIR /src/adminui

COPY adminui/package.json adminui/package-lock.json ./
RUN npm ci

COPY adminui/ ./
RUN npm run build

FROM --platform=$BUILDPLATFORM rust:1.91.1-slim-bookworm AS builder
WORKDIR /src

ARG TARGETARCH
ARG CARGO_ZIGBUILD_VERSION=0.24.1
COPY rust-toolchain.toml ./
RUN case "${TARGETARCH:-}" in \
      amd64) arch=x86_64 ;; \
      arm64) arch=aarch64 ;; \
      "") arch="$(uname -m)" ;; \
      *) echo "unsupported architecture ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
 && echo "${arch}-unknown-linux-musl" > /target \
 && rustup toolchain install \
 && rustup target add "$(cat /target)" \
 && if [ "${arch}" = "$(uname -m)" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends musl-tools \
      && rm -rf /var/lib/apt/lists/* \
      && echo build > /cargo-command; \
    else \
      case "$(uname -m)" in \
        x86_64) zig_sha256=24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c ;; \
        aarch64) zig_sha256=f7a654acc967864f7a050ddacfaa778c7504a0eca8d2b678839c21eea47c992b ;; \
        *) echo "unsupported build architecture $(uname -m)" >&2; exit 1 ;; \
      esac \
      && apt-get update \
      && apt-get install -y --no-install-recommends curl xz-utils \
      && rm -rf /var/lib/apt/lists/* \
      && curl -fsSL -o /tmp/zig.tar.xz "https://ziglang.org/download/0.14.1/zig-$(uname -m)-linux-0.14.1.tar.xz" \
      && echo "${zig_sha256}  /tmp/zig.tar.xz" | sha256sum -c - \
      && mkdir -p /opt/zig \
      && tar -xJf /tmp/zig.tar.xz -C /opt/zig --strip-components=1 \
      && rm /tmp/zig.tar.xz \
      && ln -s /opt/zig/zig /usr/local/bin/zig \
      && cargo install --locked cargo-zigbuild --version "${CARGO_ZIGBUILD_VERSION}" \
      && echo zigbuild > /cargo-command; \
    fi

COPY . .
COPY --from=adminui /src/adminui/dist ./adminui/dist

ARG VERSION=dev
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    if [ "$(cat /cargo-command)" = build ]; then export TARGET_CC=musl-gcc; fi \
 && ELYSIANDB_VERSION="${VERSION}" cargo "$(cat /cargo-command)" --release --locked --bin elysiandb --target "$(cat /target)" \
 && mkdir -p /out \
 && cp "target/$(cat /target)/release/elysiandb" /out/elysiandb

FROM alpine:3.20

RUN addgroup -S app && adduser -S app -G app \
 && mkdir -p /data /app \
 && chown -R app:app /data /app

WORKDIR /app

COPY --from=builder /out/elysiandb /usr/local/bin/elysiandb

COPY docker/elysian.yaml /app/elysian.yaml

COPY THIRD_PARTY_NOTICES /usr/share/licenses/elysiandb/THIRD_PARTY_NOTICES

USER app
EXPOSE 8089

HEALTHCHECK --interval=10s --timeout=2s --retries=3 CMD wget -qO- http://127.0.0.1:8089/health || exit 1

ENTRYPOINT ["/usr/local/bin/elysiandb"]
