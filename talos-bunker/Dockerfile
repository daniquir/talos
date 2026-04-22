FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app

# 1. Create an empty project to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
COPY Cargo.toml ./
RUN cargo build --release

# 2. Copy the actual code and force recompilation of only the binary
COPY src ./src
RUN touch src/main.rs
RUN cargo build --release

FROM alpine:3.19
RUN apk add --no-cache gnupg bash wget su-exec
WORKDIR /home/talos
COPY --from=builder /app/target/release/talos-bunker /usr/local/bin/talos-bunker
COPY reveal-backup.sh /usr/local/bin/reveal-backup
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/reveal-backup && \
    chmod +x /usr/local/bin/entrypoint.sh && \
    addgroup -g 1000 talos && \
    adduser -D -u 1000 -G talos talos && \
    chown -R talos:talos /home/talos

USER talos
EXPOSE 5000

# Fixed internal configuration
ENV PORT=5000

CMD ["/usr/local/bin/entrypoint.sh"]