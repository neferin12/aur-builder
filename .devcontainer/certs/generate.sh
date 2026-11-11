#!/bin/sh
set -eu
cd /certs
# The CA persists across rebuilds so browsers need to trust it only once.
if [ ! -s ca.crt ] || [ ! -s ca.key ]; then
    openssl req -x509 -newkey rsa:3072 -nodes -keyout ca.key -out ca.crt \
        -days 3650 -subj '/CN=AUR Builder Development CA' \
        -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign'
fi
if [ ! -s server.crt ] || ! openssl x509 -checkend 604800 -noout -in server.crt; then
    openssl req -newkey rsa:2048 -nodes -keyout server.key -out server.csr -subj '/CN=aur.localhost'
    cat > server.ext <<'EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:aur.localhost,DNS:auth.localhost,DNS:mail.localhost,DNS:s3.localhost,DNS:mailpit,DNS:localhost,IP:127.0.0.1
EXT
    openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial \
        -out server.crt -days 365 -extfile server.ext
fi
# Services run with different UIDs. This volume is exclusively for local development.
chmod 644 ca.crt server.crt server.key
chmod 600 ca.key
