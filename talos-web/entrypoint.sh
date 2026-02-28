#!/bin/sh

# Generate certificates if missing
/usr/local/bin/gen_certs.sh

# Start the application
echo "🚀 Starting TALOS Web..."
exec /app/talos-web