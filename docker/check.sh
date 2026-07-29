#!/usr/bin/env bash
# Type-check del crate Rust in container, senza toolchain Rust in locale.
#
#   docker/check.sh                 # feature di default
#   docker/check.sh --features flutter,linux-pkg-config
#
# Il repo viene montato in sola lettura e copiato dentro il container: il tuo
# working tree non viene toccato (gen_version() scrive src/version.rs, e
# scrap/build.rs prova a scrivere i binding generati).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="clantodesk-check"
CARGO_VOLUME="clantodesk-cargo"   # cache dei crate fra le esecuzioni

FEATURES="${*:-linux-pkg-config}"
case "$FEATURES" in
  --features*) CARGO_ARGS="$FEATURES" ;;
  *)           CARGO_ARGS="--features $FEATURES" ;;
esac

# Se il daemon non risponde, dirlo subito e chiaramente: un fallimento qui e'
# facile da confondere con "check passato" leggendo l'exit code sbagliato.
if ! docker info >/dev/null 2>&1; then
  echo "ERRORE: il daemon Docker non risponde. Avvia Docker Desktop e riprova." >&2
  exit 2
fi

echo "==> build immagine (rapido dopo la prima volta)"
docker build -q -f "$REPO_ROOT/docker/Dockerfile.check" -t "$IMAGE" "$REPO_ROOT/docker" >/dev/null

docker volume create "$CARGO_VOLUME" >/dev/null

echo "==> cargo check --locked $CARGO_ARGS"
docker run --rm \
  -v "$REPO_ROOT":/src-ro:ro \
  -v "$CARGO_VOLUME":/root/.cargo/registry \
  -e CARGO_NET_GIT_FETCH_WITH_CLI=true \
  -e CARGO_TARGET_DIR=/build/target \
  "$IMAGE" \
  bash -c '
    set -e
    mkdir -p /work /build/target
    tar -C /src-ro --exclude=./.git --exclude=./target -cf - . | tar -C /work -xf -
    cd /work
    cargo check --locked '"$CARGO_ARGS"' -j "$(nproc)"
  '
