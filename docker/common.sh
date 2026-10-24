# Funzioni comuni agli script docker/*.sh (da includere con `source`).
#
# Prestazioni e I/O:
# - nessun bind mount del repo: i file arrivano al container come flusso tar su
#   stdin (letto dall'host direttamente dal suo disco) e rsync li allinea in un
#   volume per profilo. Non serve condividere il disco in Docker Desktop, non si
#   passa dal filesystem condiviso host/VM e le date dei file restano quelle
#   originali, quindi Cargo e Flutter ricompilano solo cio' che e' cambiato;
# - target/, registry e git di Cargo e pub cache stanno in volumi nominati sul
#   disco ext4 della VM di Docker;
# - /tmp e' un tmpfs in RAM.

CLANTO_DOCKER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$CLANTO_DOCKER_DIR/.." && pwd)"
case "$(uname -s)" in MINGW*|MSYS*) export MSYS_NO_PATHCONV=1 ;; esac

require_docker() {
  if ! docker info >/dev/null 2>&1; then
    echo "ERRORE: il daemon Docker non risponde. Avvia Docker Desktop e riprova." >&2
    exit 2
  fi
}

# Il contesto di build e' solo docker/ e passa dal client: niente condivisione file.
build_image() { # dockerfile tag
  local try
  for try in 1 2; do
    tar -C "$REPO_DIR/docker" -cf - . \
      | docker build -q -f "$1" -t "$2" - >/dev/null && return 0
    echo "    build di $2 fallita (tentativo $try), riprovo" >&2
  done
  return 1
}

# Flusso tar del repo, senza .git, artefatti di build e file generati.
repo_tar() {
  tar -C "$REPO_DIR" -cf - \
    --exclude=./.git --exclude=./target \
    --exclude=./flutter/build --exclude=./flutter/.dart_tool \
    .
}

# Allinea il volume di lavoro del profilo al repo, senza eseguire altro.
#   sync_volume <profilo>
sync_volume() {
  docker volume create "clantodesk-work-$1" >/dev/null
  repo_tar | docker run --rm -i -v "clantodesk-work-$1":/work clantodesk-check \
    bash -c 'mkdir -p /tmp/src && tar -xf - -C /tmp/src && bash /tmp/src/docker/sync-src.sh /tmp/src'
}

# Esegue un comando nel container con il repo allineato in /work.
#   dev_run <profilo> <immagine> [opzioni docker extra...] -- <comando bash>
dev_run() {
  local profile="$1" image="$2"; shift 2
  local extra=()
  while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do extra+=("$1"); shift; done
  shift
  for v in clantodesk-cargo-registry clantodesk-cargo-git clantodesk-pub-cache \
           "clantodesk-work-$profile" "clantodesk-target-$profile"; do
    docker volume create "$v" >/dev/null
  done
  repo_tar | docker run --rm -i \
    -v clantodesk-cargo-registry:/root/.cargo/registry \
    -v clantodesk-cargo-git:/root/.cargo/git \
    -v clantodesk-pub-cache:/root/.pub-cache \
    -v "clantodesk-work-$profile":/work \
    -v "clantodesk-target-$profile":/build/target \
    --tmpfs /tmp:exec,size=8g \
    -e CARGO_TARGET_DIR=/build/target \
    -e CARGO_NET_GIT_FETCH_WITH_CLI=false \
    -e CARGO_NET_RETRY=5 \
    -e CARGO_TERM_COLOR=never \
    -w /work \
    "${extra[@]}" \
    "$image" bash -c "mkdir -p /tmp/src && tar -xf - -C /tmp/src && bash /tmp/src/docker/sync-src.sh /tmp/src && cd /work && $* </dev/null"
}
