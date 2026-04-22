# STAGE 1: Build
FROM rust:alpine AS builder

# Install minimal dependencies to link the binary
RUN apk add --no-cache musl-dev sqlite-dev openssl-dev pkgconfig

WORKDIR /app

# 1. Cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
COPY Cargo.toml ./
RUN cargo build --release

# 2. Compile the actual source code
COPY src ./src

RUN touch src/main.rs
RUN cargo build --release

# STAGE 2: Runtime (Lightweight Image)
FROM alpine:3.19

# Install necessary libraries for reqwest to work (SSL)
RUN apk add --no-cache ca-certificates libgcc sqlite-libs openssl wget su-exec

WORKDIR /app

# Copy binary from build stage
COPY --from=builder /app/target/release/talos-web /app/talos-web

# Copy static files directly from the context (faster)
COPY static ./static
COPY gen_certs.sh /usr/local/bin/gen_certs.sh
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

RUN chmod +x /usr/local/bin/gen_certs.sh && \
    chmod +x /usr/local/bin/entrypoint.sh && \
    chmod +x /app/talos-web

# Create data and certs directories with proper permissions
RUN mkdir -p /data /app/certs && \
    addgroup -g 1000 talos && \
    adduser -D -u 1000 -G talos talos && \
    chown -R talos:talos /app /data

USER talos

# Expose port for documentation (real mapping is done in docker-compose)
EXPOSE 3000 3443

# Fixed internal configuration
ENV PORT=3000
ENV STORAGE_URL=http://talos-storage:4000

# Run the entrypoint script
CMD ["/usr/local/bin/entrypoint.sh"]