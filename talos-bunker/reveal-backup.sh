#!/bin/sh
BACKUP_FILE="/root/keys/EMERGENCY_BACKUP.txt"

if [ -f "$BACKUP_FILE" ]; then
    echo "--- INICIO DE LLAVE PRIVADA DE EMERGENCIA (COPIAR Y GUARDAR) ---"
    cat "$BACKUP_FILE"
    echo "--- FIN DE LLAVE ---"
    
    # Autodestrucción segura
    rm -f "$BACKUP_FILE"
    echo "\n[!] ARCHIVO DE BACKUP ELIMINADO PARA SIEMPRE."
else
    echo "[X] Error: El backup ya fue reclamado o no existe."
    exit 1
fi