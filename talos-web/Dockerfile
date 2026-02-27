# STAGE 1: Build
FROM rust:alpine AS builder

# Install minimal dependencies to link the binary
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

# STAGE 2: Runtime (Lightweight Image)
FROM alpine:latest

# Install necessary libraries for reqwest to work (SSL)
RUN apk add --no-cache ca-certificates libgcc

WORKDIR /app

# Copy binary from build stage
COPY --from=builder /app/target/release/talos-web /app/talos-web

# Copiamos los estáticos directamente desde el contexto (más rápido)
COPY static ./static

# Ensure binary has execution permissions
RUN chmod +x /app/talos-web

# Expose port for documentation (real mapping is done in docker-compose)
EXPOSE 3000

# Fixed internal configuration
ENV PORT=3000
ENV STORAGE_URL=http://talos-storage:4000

# Run binary pointing to port address
CMD ["./talos-web"]