# Multi-Stage Production Dockerfile for Rust Antigravity Gateway
# Ultra-compact, static native container (~15 MB, <12 MB RAM)

# Stage 1: Build the native binary
FROM rust:1.80-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app

# Copy rust crate files
COPY rust-gateway/Cargo.toml rust-gateway/Cargo.lock ./
COPY rust-gateway/src ./src
COPY public ../public

# Build optimized static release binary
RUN cargo build --release

# Stage 2: Minimal runtime image
FROM alpine:3.20

RUN apk add --no-cache ca-certificates

WORKDIR /app

# Copy the compiled static binary from builder
COPY --from=builder /app/target/release/rust-gateway /usr/local/bin/rust-gateway

# Set execution defaults
ENV PORT=10000
ENV HOST=0.0.0.0

EXPOSE 10000

ENTRYPOINT ["rust-gateway"]
