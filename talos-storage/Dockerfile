FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app

# 1. Cacheo de dependencias
RUN mkdir src && echo "fn main() {}" > src/main.rs
COPY Cargo.toml ./
RUN cargo build --release

# 2. Compilación del código fuente real
COPY src ./src
RUN touch src/main.rs
RUN cargo build --release

FROM alpine:3.19
# We need git if we want Storage to sync automatically
RUN apk add --no-cache git openssh-client wget su-exec
WORKDIR /app
COPY --from=builder /app/target/release/talos-storage /app/talos-storage
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /app/talos-storage && \
    chmod +x /usr/local/bin/entrypoint.sh && \
    addgroup -g 1000 talos && \
    adduser -D -u 1000 -G talos talos && \
    mkdir -p /home/talosuser/.password-store /home/talos/.ssh && \
    chown -R talos:talos /app /home/talos /home/talosuser

USER talos
EXPOSE 4000

# Fixed internal configuration
ENV PORT=4000
ENV BUNKER_URL=http://talos-bunker:5000

CMD ["/usr/local/bin/entrypoint.sh"]
