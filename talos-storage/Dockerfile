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

FROM alpine:latest
# We need git if we want Storage to sync automatically
RUN apk add --no-cache git openssh-client
WORKDIR /app
COPY --from=builder /app/target/release/talos-storage /app/talos-storage
EXPOSE 4000

# Fixed internal configuration
ENV PORT=4000
ENV BUNKER_URL=http://talos-bunker:5000

CMD ["./talos-storage"]
