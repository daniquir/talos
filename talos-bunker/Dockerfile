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

FROM alpine:latest
RUN apk add --no-cache gnupg bash
WORKDIR /root
COPY --from=builder /app/target/release/talos-bunker /usr/local/bin/talos-bunker
COPY reveal-backup.sh /usr/local/bin/reveal-backup
RUN chmod +x /usr/local/bin/reveal-backup
EXPOSE 5000

# Fixed internal configuration
ENV PORT=5000

CMD ["talos-bunker"]