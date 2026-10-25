#!/usr/bin/env python3
"""Firma il catalogo dei servizi di forwarding di ClantoDesk (Ed25519).

    sign-forward-catalog.py --genkey chiave.pem
        crea la chiave privata e stampa la pubblica in base64: va nel secret
        GitHub CLANTO_FORWARD_CATALOG_PK. La chiave privata resta a Clanto e non
        entra mai nel repository.

    sign-forward-catalog.py catalogo.json chiave.pem > clantodesk-forward-catalog.json
        firma i byte esatti di catalogo.json; il risultato si pubblica come
        https://clanto.it/api/clantodesk-forward-catalog.json

Il catalogo deve avere "version": 1 e "expires": "YYYY-MM-DD"; il client lo
rifiuta dopo la scadenza. Richiede il pacchetto `cryptography`.
"""
import base64
import json
import sys

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def genkey(path: str) -> None:
    key = Ed25519PrivateKey.generate()
    pem = key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    with open(path, "xb") as f:
        f.write(pem)
    raw = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    print(base64.b64encode(raw).decode())


def sign(catalog_path: str, key_path: str) -> None:
    with open(catalog_path, "rb") as f:
        payload = f.read()
    catalog = json.loads(payload)
    if catalog.get("version") != 1 or len(catalog.get("expires", "")) != 10:
        sys.exit("catalogo non valido: servono version 1 ed expires YYYY-MM-DD")
    with open(key_path, "rb") as f:
        key = serialization.load_pem_private_key(f.read(), password=None)
    signature = key.sign(payload)
    print(json.dumps({
        "payload": base64.b64encode(payload).decode(),
        "signature": base64.b64encode(signature).decode(),
    }))


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--genkey":
        genkey(sys.argv[2])
    elif len(sys.argv) == 3:
        sign(sys.argv[1], sys.argv[2])
    else:
        sys.exit(__doc__)
