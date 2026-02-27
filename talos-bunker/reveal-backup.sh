#!/bin/sh
BACKUP_FILE="/root/keys/EMERGENCY_BACKUP.txt"

if [ -f "$BACKUP_FILE" ]; then
    echo "--- BEGIN EMERGENCY PRIVATE KEY (COPY AND SAVE) ---"
    cat "$BACKUP_FILE"
    echo "--- END OF KEY ---"
    
    rm -f "$BACKUP_FILE"
    echo "\n[!] BACKUP FILE DELETED FOREVER."
else
    echo "[X] Error: Backup already claimed or does not exist."
    exit 1
fi