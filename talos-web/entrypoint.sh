#!/bin/sh
set -e

# Generate SSL certificates if they don't exist
if [ ! -f "/app/certs/key.pem" ] || [ ! -f "/app/certs/cert.pem" ]; then
    echo "🔒 Generating self-signed SSL certificates..."
    openssl req -x509 -newkey rsa:4096 -nodes -keyout /app/certs/key.pem -out /app/certs/cert.pem -days 365 -subj "/CN=localhost"
    echo "✅ Certificates generated."
fi

# Start the application
echo "🚀 Starting TALOS Web..."
exec /app/talos-web