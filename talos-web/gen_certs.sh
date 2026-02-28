#!/bin/sh

CERT_DIR="/data/certs"
mkdir -p "$CERT_DIR"

if [ -f "$CERT_DIR/server.key" ]; then
    echo "🔒 [CERTS] Certificates already exist. Skipping generation."
    exit 0
fi

echo "🛡️ [CERTS] Generating TALOS Certificate Authority..."

# 1. CA (Certificate Authority)
openssl req -new -x509 -days 3650 -keyout "$CERT_DIR/ca.key" -out "$CERT_DIR/ca.crt" \
    -subj "/C=US/ST=Secure/L=Bunker/O=Talos Systems/CN=Talos Root CA" -nodes

# 2. Server Certificate
echo "🛡️ [CERTS] Generating Server Certificate..."
openssl req -newkey rsa:2048 -nodes -keyout "$CERT_DIR/server.key" -out "$CERT_DIR/server.csr" \
    -subj "/C=US/ST=Secure/L=Bunker/O=Talos Systems/CN=talos.local"

# Sign Server Cert with CA
openssl x509 -req -days 3650 -in "$CERT_DIR/server.csr" -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" -CAcreateserial -out "$CERT_DIR/server.crt"

# 3. Client Certificate (The Diplomatic Pass)
echo "🛡️ [CERTS] Generating Client Certificate (The Diplomatic Pass)..."
openssl req -newkey rsa:2048 -nodes -keyout "$CERT_DIR/client.key" -out "$CERT_DIR/client.csr" \
    -subj "/C=US/ST=Secure/L=Bunker/O=Talos Systems/CN=Talos Admin User"

# Sign Client Cert with CA
openssl x509 -req -days 3650 -in "$CERT_DIR/client.csr" -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" -CAcreateserial -out "$CERT_DIR/client.crt"

# Export Client Cert to PKCS#12 (Browser Format)
# Password is empty for "Plug & Play" experience, or you can set one.
openssl pkcs12 -export -out "$CERT_DIR/talos_client_pass.p12" \
    -inkey "$CERT_DIR/client.key" -in "$CERT_DIR/client.crt" \
    -certfile "$CERT_DIR/ca.crt" -passout pass:

chmod 644 "$CERT_DIR"/*

echo "###################################################################"
echo "# [ATTENTION] DIPLOMATIC PASS GENERATED                           #"
echo "#                                                                 #"
echo "# Download this file and install it in your browser/OS:           #"
echo "# ./data/web/certs/talos_client_pass.p12                          #"
echo "#                                                                 #"
echo "###################################################################"