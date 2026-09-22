#!/bin/sh
set -e

# Start the application (image USER is talos; host bind mounts must be uid 1000 —
# see dev/scripts/dev-up.sh which chowns bunker-gnupg-users / bunker-wrapped).
mkdir -p /home/talos/.gnupg-users /home/talos/.talos-wrapped 2>/dev/null || true
echo "🛡️ Starting TALOS Bunker..."
exec talos-bunker
