#!/bin/sh
set -e

# Setup SSH from Docker secret
if [ -f "/run/secrets/id_rsa_talos" ]; then
    echo "🔑 Setting up SSH key from Docker secret..."
    mkdir -p /home/talos/.ssh
    cp /run/secrets/id_rsa_talos /home/talos/.ssh/id_rsa
    chmod 600 /home/talos/.ssh/id_rsa
    chown talos:talos /home/talos/.ssh/id_rsa
fi

# Start the application
echo "🌉 Starting TALOS Storage..."
exec ./talos-storage
