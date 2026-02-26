FROM rust:1.83-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release

FROM alpine:latest
RUN apk add --no-cache gnupg bash
WORKDIR /root
COPY --from=builder /app/target/release/talos-bunker /usr/local/bin/talos-bunker
COPY reveal-backup.sh /usr/local/bin/reveal-backup
RUN chmod +x /usr/local/bin/reveal-backup
EXPOSE 5000
CMD ["talos-bunker"]