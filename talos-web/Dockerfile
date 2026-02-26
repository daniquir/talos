# ETAPA 1: Compilación
FROM rust:1.83-alpine AS builder

# Instalamos dependencias mínimas para enlazar el binario
RUN apk add --no-cache musl-dev

WORKDIR /app

# Copiamos primero los archivos de dependencias para aprovechar la cache de Docker
COPY Cargo.toml ./
# Si tienes un Cargo.lock, descomenta la siguiente línea:
# COPY Cargo.lock ./

# Copiamos el código fuente y los archivos estáticos
COPY src ./src
COPY static ./static

# Compilamos el binario
RUN cargo build --release

# ETAPA 2: Ejecución (Imagen Ligera)
FROM alpine:latest

# Instalamos librerías necesarias para que reqwest funcione (SSL)
RUN apk add --no-cache ca-certificates libgcc

WORKDIR /app

# Copiamos el binario desde la etapa de compilación
COPY --from=builder /app/target/release/talos-web /app/talos-web

# Copiamos la carpeta static completa al directorio de trabajo
COPY --from=builder /app/static /app/static

# Aseguramos que el binario tiene permisos de ejecución
RUN chmod +x /app/talos-web

# Exponemos el puerto para documentación (el mapeo real se hace en docker-compose)
EXPOSE 3000

# Ejecutamos el binario apuntando a la dirección del puerto
CMD ["./talos-web"]