FROM rust:1.83-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release

FROM alpine:latest
# Necesitamos git si queremos que el Storage haga el sync automáticamente
RUN apk add --no-cache git
WORKDIR /root
COPY --from=builder /app/target/release/talos-storage /usr/local/bin/
EXPOSE 4000
CMD ["talos-storage"]
