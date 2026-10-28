#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
# SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0
"""Applica il brand di un partner (livello 1) al checkout, prima della build.

Uso: apply-brand.py <cartella del repo partner> <nome del repo partner>

Legge `brand.json` e la cartella `icons/` (CLANTO.md, «Build per i partner»),
copia le icone sopra quelle di ClantoDesk, cambia il nome mostrato da Task
Manager (Windows) e dalla barra dei menu (macOS) e scrive in $GITHUB_ENV i
valori letti dal codice Rust (`src/clanto/brand.rs`) e il prefisso degli asset.
Servizio, cartelle, bundle id, `clantodesk.exe` e firma non cambiano.
"""

import json
import os
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

REPO_RE = re.compile(r"[a-z0-9][a-z0-9-]{0,62}")
PREFIX_RE = re.compile(r"[a-z][a-z0-9-]{1,31}")
# Il nome finisce in Runner.rc, Info.plist e Cargo.toml: solo ASCII senza virgolette.
NAME_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9 .-]{0,38}[A-Za-z0-9.]")
COMPANY_RE = re.compile(r"[^\W_][\w .,&'-]{0,58}[\w.]")
URL_RE = re.compile(r"https://[A-Za-z0-9.-]+(/[A-Za-z0-9._~%/?#=&+-]*)?")

# file in icons/ -> file di ClantoDesk sostituiti
ICONS = {
    "icon.png": ["flutter/assets/icon.png", "res/icon.png"],
    "icon.ico": ["res/icon.ico", "flutter/windows/runner/resources/app_icon.ico"],
    "tray-icon.ico": ["res/tray-icon.ico"],
    "AppIcon.icns": ["flutter/macos/Runner/AppIcon.icns"],
    "icon_tray_macos.png": ["flutter/assets/icon_tray_macos.png", "res/mac-tray-dark-x2.png"],
}
MAGIC = {".png": b"\x89PNG\r\n\x1a\n", ".ico": b"\x00\x00\x01\x00", ".icns": b"icns"}
MAX_ICON_BYTES = 2 * 1024 * 1024


def fail(msg):
    print(f"::error::brand: {msg}")
    sys.exit(1)


def field(brand, key, pattern, required=True):
    value = brand.get(key)
    if value is None and not required:
        return ""
    if not isinstance(value, str) or not pattern.fullmatch(value):
        fail(f"campo '{key}' assente o non valido: {value!r}")
    return value


def replace_once(path, pattern, repl):
    file = ROOT / path
    text = file.read_bytes().decode("utf-8")
    new, count = re.subn(pattern, lambda _m: repl, text, count=1)
    if count != 1:
        fail(f"{path}: punto da modificare non trovato")
    file.write_bytes(new.encode("utf-8"))


def main():
    if len(sys.argv) != 3:
        fail("uso: apply-brand.py <cartella partner> <repo partner>")
    src = Path(sys.argv[1]).resolve()
    repo = sys.argv[2]
    if not REPO_RE.fullmatch(repo) or repo == "clantodesk-client":
        fail(f"nome del repo partner non valido: {repo!r}")

    try:
        brand = json.loads((src / "brand.json").read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        fail(f"brand.json illeggibile: {e}")
    if not isinstance(brand, dict):
        fail("brand.json deve essere un oggetto")
    known = {"name", "company", "file-prefix", "support-url", "website-url"}
    unknown = set(brand) - known
    if unknown:
        fail(f"campi sconosciuti in brand.json: {sorted(unknown)}")

    name = field(brand, "name", NAME_RE)
    company = field(brand, "company", COMPANY_RE)
    prefix = field(brand, "file-prefix", PREFIX_RE)
    support = field(brand, "support-url", URL_RE)
    website = field(brand, "website-url", URL_RE)
    if prefix == "clantodesk":
        fail("file-prefix deve essere quello del partner")

    for icon, targets in ICONS.items():
        file = src / "icons" / icon
        if file.is_symlink() or not file.is_file():
            fail(f"icons/{icon} assente")
        data = file.read_bytes()
        if len(data) > MAX_ICON_BYTES or not data.startswith(MAGIC[file.suffix]):
            fail(f"icons/{icon} non valida (formato {file.suffix}, max {MAX_ICON_BYTES} byte)")
        for target in targets:
            shutil.copyfile(file, ROOT / target)

    replace_once(
        "flutter/windows/runner/Runner.rc",
        r'VALUE "FileDescription", "[^"]*"',
        f'VALUE "FileDescription", "{name}"',
    )
    replace_once(
        "libs/portable/Cargo.toml",
        r'FileDescription = "[^"]*"',
        f'FileDescription = "{name}"',
    )
    replace_once(
        "flutter/macos/Runner/Info.plist",
        r"(<key>CFBundleName</key>\s*<string>)\$\(PRODUCT_NAME\)(</string>)",
        f"<key>CFBundleName</key>\n\t<string>{name}</string>\n"
        f"\t<key>CFBundleDisplayName</key>\n\t<string>{name}</string>",
    )

    values = {
        "CLANTO_BRAND_NAME": name,
        "CLANTO_BRAND_COMPANY": company,
        "CLANTO_BRAND_FILE_PREFIX": prefix,
        "CLANTO_BRAND_REPO": repo,
        "CLANTO_BRAND_SUPPORT_URL": support,
        "CLANTO_BRAND_WEBSITE_URL": website,
        "ASSET_PREFIX": prefix,
    }
    env_file = os.environ.get("GITHUB_ENV")
    if env_file:
        with open(env_file, "a", encoding="utf-8") as f:
            for key, value in values.items():
                f.write(f"{key}={value}\n")
    for key, value in values.items():
        print(f"{key}={value}")


if __name__ == "__main__":
    main()
