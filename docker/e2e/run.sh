#!/usr/bin/env bash
# Prova end-to-end del 2FA e dei controller fidati, tutta in container:
# rendezvous (hbbs/hbbr ufficiali), host ClantoDesk reale (--server, root) e un
# controller headless con due utenti (chiavi diverse).
#
#   docker/e2e/run.sh            # build + scenari
#   KEEP=1 docker/e2e/run.sh     # lascia i container accesi per ispezionarli
#
# L'harness (docker/e2e/e2e_harness.rs) entra solo nella copia del repo nel
# volume di lavoro del profilo `e2e`: il working tree e il prodotto non cambiano.
# La chiave di hbbs sta in un volume persistente: entra nel binario al build, e
# cambiarla a ogni giro forzerebbe la ricompilazione del crate.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../common.sh"

NET=clantodesk-e2e
SERVER_IMAGE=rustdesk/rustdesk-server:1.1.14
SERVER_VOLUME=clantodesk-e2e-server
PASSWORD='E2e-Pass-7731'
TOTP_HEX=00112233445566778899aabbccddeeff01234567
BIN=/out/rustdesk

remove_all() {
  docker rm -f e2e-hbbs e2e-hbbr e2e-host e2e-ctrl >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
}
on_exit() {
  if [ "${KEEP:-0}" = 1 ]; then echo "KEEP=1: container lasciati accesi"; else remove_all; fi
}
require_docker
remove_all
trap on_exit EXIT

build_image Dockerfile.check clantodesk-check
for v in clantodesk-e2e-out "$SERVER_VOLUME"; do docker volume create "$v" >/dev/null; done
docker network create "$NET" >/dev/null

echo "==> rendezvous $SERVER_IMAGE"
docker run -d --name e2e-hbbr --network "$NET" -v "$SERVER_VOLUME":/root "$SERVER_IMAGE" hbbr >/dev/null
docker run -d --name e2e-hbbs --network "$NET" -v "$SERVER_VOLUME":/root "$SERVER_IMAGE" \
  hbbs -r e2e-hbbr:21117 >/dev/null
for _ in $(seq 30); do
  # L'immagine di hbbs non ha una shell: si legge il volume da un altro container.
  RS_PUB_KEY="$(docker run --rm -v "$SERVER_VOLUME":/d clantodesk-check cat /d/id_ed25519.pub 2>/dev/null || true)"
  [ -n "$RS_PUB_KEY" ] && break; sleep 1
done
[ -n "$RS_PUB_KEY" ] || { echo "hbbs non ha generato la chiave"; docker logs e2e-hbbs; exit 1; }
echo "    chiave rendezvous: $RS_PUB_KEY"

echo "==> build con harness (RENDEZVOUS_SERVER=e2e-hbbs)"
dev_run e2e clantodesk-check \
  -v clantodesk-e2e-out:/out \
  -e RENDEZVOUS_SERVER=e2e-hbbs \
  -e RS_PUB_KEY="$RS_PUB_KEY" \
  -- bash docker/e2e/build-inner.sh

run_node() { # nome
  docker run -d --name "$1" --network "$NET" -v clantodesk-e2e-out:/out:ro \
    clantodesk-check sleep infinity >/dev/null
}

echo "==> host (root) e controller (utenti tech1, tech2)"
run_node e2e-host
run_node e2e-ctrl
docker exec e2e-ctrl bash -c 'useradd -m tech1 && useradd -m tech2'

SETUP="$(docker exec e2e-host $BIN --e2e host-setup "$PASSWORD" "$TOTP_HEX")"
echo "    $SETUP"
HOST_ID="$(sed -n 's/.*HOST_ID=\([^ ]*\).*/\1/p' <<<"$SETUP")"
grep -q 'TFA=true' <<<"$SETUP" || { echo "2FA non configurato sull'host"; exit 1; }
docker exec -d e2e-host bash -c "$BIN --server >/tmp/server.log 2>&1"

ctrl() { docker exec -u "$1" e2e-ctrl $BIN --e2e "${@:2}"; }
KEY1="$(ctrl tech1 device-key | sed -n 's/^DEVICE_KEY=//p')"
ID1="$(ctrl tech1 my-id | sed -n 's/^MY_ID=//p')"
ID2="$(ctrl tech2 my-id | sed -n 's/^MY_ID=//p')"
echo "    host $HOST_ID | tech1 $ID1 chiave $KEY1 | tech2 $ID2"
[ -n "$KEY1" ] || { echo "tech1 senza chiave del dispositivo"; exit 1; }

echo "==> attesa registrazione dell'host"
for _ in $(seq 40); do
  out="$(ctrl tech1 login "$HOST_ID" "$PASSWORD" - 2>/dev/null | sed -n 's/^RESULT=//p')"
  case "$out" in error:connect*|error:timeout|"") sleep 2 ;; *) break ;; esac
done

PASS=0; FAIL=0
expect() { # descrizione atteso utente args...
  local desc="$1" want="$2" user="$3"; shift 3
  local got
  got="$(ctrl "$user" login "$@" 2>/dev/null | sed -n 's/^RESULT=//p')"
  if [ "$got" = "$want" ]; then PASS=$((PASS+1)); echo "  OK    $desc -> $got"
  else FAIL=$((FAIL+1)); echo "  FAIL  $desc -> atteso $want, ottenuto '$got'"; fi
}

echo "==> scenari"
expect "1 nessuna fiducia: serve il 2FA" 2fa-required tech1 "$HOST_ID" "$PASSWORD" -
expect "2 codice TOTP corretto: accesso" ok tech1 "$HOST_ID" "$PASSWORD" "$TOTP_HEX"
expect "3 Trust this device (hwid) con TOTP: accesso" ok tech1 "$HOST_ID" "$PASSWORD" "$TOTP_HEX" hwid
expect "4 hwid gia' inviato: l'host non l'ha salvato" 2fa-required tech1 "$HOST_ID" "$PASSWORD" - hwid

echo "    host: fiducia a tech1 ($ID1)"
docker exec e2e-host $BIN --e2e host-trust "$ID1=$KEY1" | sed 's/^/    /'
expect "5 chiave registrata e firma valida: niente 2FA" ok tech1 "$HOST_ID" "$PASSWORD" -
# tech1 e tech2 stanno sullo stesso PC: stesso ID, chiavi diverse.
expect "6 stesso ID, chiave di un altro utente" 2fa-required tech2 "$HOST_ID" "$PASSWORD" - "as-id=$ID1"
expect "7 ID non in elenco" 2fa-required tech2 "$HOST_ID" "$PASSWORD" - "as-id=999888777"
expect "7b chiave fidata usata con un altro ID" 2fa-required tech1 "$HOST_ID" "$PASSWORD" - "as-id=999888777"
expect "8 controller fidato senza firma" 2fa-required tech1 "$HOST_ID" "$PASSWORD" - no-key
expect "9 controller fidato, password errata" "error:Wrong Password" tech1 "$HOST_ID" "wrong-$PASSWORD" -

echo "    host: elenco svuotato"
docker exec e2e-host $BIN --e2e host-trust "" | sed 's/^/    /'
expect "10 fiducia revocata: torna il 2FA" 2fa-required tech1 "$HOST_ID" "$PASSWORD" -

echo "    host: clanto-require-2fa=Y (policy)"
hopt() { docker exec e2e-host $BIN --e2e host-option "$@" | sed 's/^/    /'; }
hopt clanto-require-2fa Y
expect "11 policy 2FA, 2FA configurato: chiede il codice" 2fa-required tech1 "$HOST_ID" "$PASSWORD" -
echo "    host: 2FA disattivato con la policy attiva"
hopt 2fa ""
expect_prefix() { # descrizione prefisso utente args...
  local desc="$1" want="$2" user="$3"; shift 3
  local got
  got="$(ctrl "$user" login "$@" 2>/dev/null | sed -n 's/^RESULT=//p')"
  if [ "${got#"$want"}" != "$got" ]; then PASS=$((PASS+1)); echo "  OK    $desc -> $got"
  else FAIL=$((FAIL+1)); echo "  FAIL  $desc -> atteso '$want...', ottenuto '$got'"; fi
}
expect_prefix "12 policy 2FA senza 2FA: accesso rifiutato" "error:The remote PC requires two-factor" tech1 "$HOST_ID" "$PASSWORD" "$TOTP_HEX"
hopt clanto-require-2fa N
expect "13 senza policy e senza 2FA: solo password" ok tech1 "$HOST_ID" "$PASSWORD" -

echo
echo "E2E 2FA: $PASS superati, $FAIL falliti"
if [ "$FAIL" != 0 ]; then
  echo "--- log host (ultime righe) ---"; docker exec e2e-host tail -40 /tmp/server.log || true
  exit 1
fi
