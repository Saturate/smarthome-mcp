FROM rust:1-slim AS build
ARG TARGETARCH

RUN apt-get update && apt-get install -y musl-tools cmake gcc-aarch64-linux-gnu && rm -rf /var/lib/apt/lists/*

RUN case "$TARGETARCH" in \
      amd64) rustup target add x86_64-unknown-linux-musl ;; \
      arm64) rustup target add aarch64-unknown-linux-musl ;; \
    esac

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN case "$TARGETARCH" in \
      amd64) \
        cargo build --release --target x86_64-unknown-linux-musl && \
        cp target/x86_64-unknown-linux-musl/release/smarthome-mcp /app/smarthome-mcp ;; \
      arm64) \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc \
        cargo build --release --target aarch64-unknown-linux-musl && \
        cp target/aarch64-unknown-linux-musl/release/smarthome-mcp /app/smarthome-mcp ;; \
    esac

FROM scratch
COPY --from=build /app/smarthome-mcp /smarthome-mcp
EXPOSE 3000
ENTRYPOINT ["/smarthome-mcp"]
