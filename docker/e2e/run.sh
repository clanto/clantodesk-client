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
build_image e2e/Dockerfile.node clantodesk-e2e-node
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
    clantodesk-e2e-node sleep infinity >/dev/null
}

echo "==> host (root) e controller (utenti tech1, tech2)"
run_node e2e-host
run_node e2e-ctrl
docker exec e2e-ctrl bash -c 'useradd -m tech1 && useradd -m tech2'

SETUP="$(docker exec e2e-host $BIN --e2e host-setup "$PASSWORD" "$TOTP_HEX")"
echo "    $SETUP"
HOST_ID="$(sed -n 's/.*HOST_ID=\([^ ]*\).*/\1/p' <<<"$SETUP")"
grep -q 'TFA=true' <<<"$SETUP" || { echo "2FA non configurato sull'host"; exit 1; }
# Display virtuale: le sessioni di controllo remoto enumerano i monitor.
docker exec -d e2e-host bash -c "Xvfb :0 -screen 0 1280x800x24 >/tmp/xvfb.log 2>&1 & sleep 1; CLANTO_E2E_CAMERA_SINK=null DISPLAY=:0 $BIN --server >/tmp/server.log 2>&1"

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
echo "    host: 2FA presente ma illeggibile dal servizio"
docker exec e2e-host $BIN --e2e host-2fa-foreign | sed 's/^/    /'
expect_prefix "13b 2FA illeggibile: accesso rifiutato" "error:The remote PC has two-factor authentication enabled but cannot" tech1 "$HOST_ID" "$PASSWORD" "$TOTP_HEX"
echo "    host: 2FA attivato dalla UI, cifrato dal servizio"
docker exec e2e-host $BIN --e2e ui-2fa "$TOTP_HEX" | sed 's/^/    /'
expect "13c 2FA salvato dal servizio: chiede il codice" 2fa-required tech1 "$HOST_ID" "$PASSWORD" -
expect "13d 2FA salvato dal servizio: codice corretto" ok tech1 "$HOST_ID" "$PASSWORD" "$TOTP_HEX"
hopt 2fa ""

echo "==> forwarding (sessione di controllo remoto)"
ECHO_PY='import socket,sys,threading
s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
s.bind(("127.0.0.1",int(sys.argv[1])));s.listen()
def h(c):
    while True:
        d=c.recv(65536)
        if not d: break
        c.sendall(d)
    c.close()
while True:
    c,_=s.accept();threading.Thread(target=h,args=(c,),daemon=True).start()'
CLIENT_PY='import socket,sys
d=b""
try:
    s=socket.create_connection(("127.0.0.1",int(sys.argv[1])),timeout=10);s.sendall(b"rev-e2e")
    while len(d)<7:
        x=s.recv(64)
        if not x: break
        d+=x
except Exception: pass
print("ECHO="+d.decode(errors="replace"))'
docker exec -d e2e-host python3 -c "$ECHO_PY" 7001
docker exec -d e2e-host python3 -c "$ECHO_PY" 7002
docker exec -d e2e-ctrl python3 -c "$ECHO_PY" 7003
sleep 1
fwd() { ctrl "$1" forward "$HOST_ID" "$PASSWORD" "$2" 2>/dev/null | sed -n 's/^RESULT=//p'; }
expect_fwd() { # descrizione atteso utente modo
  local got; got="$(fwd "$3" "$4")"
  if [ "$got" = "$2" ]; then PASS=$((PASS+1)); echo "  OK    $1 -> $got"
  else FAIL=$((FAIL+1)); echo "  FAIL  $1 -> atteso $2, ottenuto '$got'"; fi
}
# Forward verso il controller: harness in background, client dalla parte dell'host.
expect_reverse() { # descrizione atteso(ECHO=...) utente
  docker exec e2e-ctrl rm -f /tmp/rev.out
  docker exec -u "$3" -d e2e-ctrl bash -c "$BIN --e2e forward $HOST_ID $PASSWORD reverse:127.0.0.1:7003:20 > /tmp/rev.out 2>&1"
  local port="" out=""
  for _ in $(seq 60); do
    out="$(docker exec e2e-ctrl cat /tmp/rev.out 2>/dev/null || true)"
    port="$(sed -n 's/^LISTEN=//p' <<<"$out")"
    { [ -n "$port" ] || grep -q '^RESULT=' <<<"$out"; } && break; sleep 0.5
  done
  local got
  if [ -n "$port" ]; then got="$(docker exec e2e-host python3 -c "$CLIENT_PY" "$port")"
  else got="$(sed -n 's/^RESULT=//p' <<<"$out")"; fi
  if [ "$got" = "$2" ]; then PASS=$((PASS+1)); echo "  OK    $1 -> $got"
  else FAIL=$((FAIL+1)); echo "  FAIL  $1 -> atteso $2, ottenuto '$got'"; fi
  docker exec e2e-ctrl pkill -f 'e2e forward' >/dev/null 2>&1 || true
}
expect_fwd "14 forwarding spento sull'host" "error:Forwarding is disabled on this device" tech1 "to-host:127.0.0.1:7001"
hopt clanto-forward-enabled Y
hopt clanto-forward-custom-host Y
hopt clanto-forward-custom-host-targets "127.0.0.1:7001"
expect_fwd "15 destinazione ammessa sull'host" ok tech1 "to-host:127.0.0.1:7001"
expect_fwd "16 destinazione non in elenco" "error:Forwarding target not allowed" tech1 "to-host:127.0.0.1:7002"
expect_fwd "16b alias localhost della destinazione ammessa" ok tech1 "to-host:localhost:7001"
# Servizio dell'elenco Clanto spuntato sull'host, senza destinazioni personalizzate.
hopt clanto-forward-custom-host N
hopt clanto-forward-services-host "echo=localhost:7001"
expect_fwd "16d servizio Clanto spuntato sull'host" ok tech1 "to-host:127.0.0.1:7001"
hopt clanto-forward-services-host ""
expect_fwd "16e servizio Clanto non spuntato" "error:Forwarding target not allowed" tech1 "to-host:127.0.0.1:7001"
hopt clanto-forward-custom-host Y
# Profilo per ID «view» (senza Tunnel): niente forward, anche se la policy lo ammette.
hopt id-whitelist "$ID1"
hopt clanto-id-permissions "{\"$ID1\":{\"profile\":\"view\"}}"
expect_fwd "16c profilo Sola visualizzazione: forward rifiutato" "error:Forwarding is not allowed for this connection" tech1 "to-host:127.0.0.1:7001"
hopt clanto-id-permissions ""
hopt id-whitelist ""
expect_reverse "17 inverso non consentito dall'host" "error:listen Forwarding towards the controller is not allowed on this device" tech1
hopt clanto-forward-reverse Y
expect_reverse "18 inverso: il controller non espone nulla" "ECHO=" tech1
docker exec -u tech1 e2e-ctrl $BIN --e2e local-option clanto-forward-enabled Y >/dev/null
docker exec -u tech1 e2e-ctrl $BIN --e2e local-option clanto-forward-custom-controller Y >/dev/null
docker exec -u tech1 e2e-ctrl $BIN --e2e local-option clanto-forward-custom-controller-targets "127.0.0.1:7003" >/dev/null
expect_reverse "19 inverso: servizio del controller esposto sull'host" "ECHO=rev-e2e" tech1

echo "==> webcam del controller (pattern di prova, VP8)"
cam() { ctrl "$1" camera "$HOST_ID" "$PASSWORD" "$2" 2>/dev/null | sed -n 's/^RESULT=//p'; }
expect_cam() { # descrizione atteso(prefisso) utente frame
  local got; got="$(cam "$3" "$4")"
  if [ "${got#"$2"}" != "$got" ]; then PASS=$((PASS+1)); echo "  OK    $1 -> $got"
  else FAIL=$((FAIL+1)); echo "  FAIL  $1 -> atteso '$2...', ottenuto '$got'"; fi
}
expect_cam "20 forwarding webcam spento sull'host" "error:Webcam forwarding is disabled on this device" tech1 10
hopt clanto-forward-webcam Y
hopt clanto-forward-camera-device /dev/null
expect_cam "21 dispositivo che non è v4l2loopback" "error:/dev/null is not a v4l2loopback device" tech1 10
hopt clanto-forward-camera-device ""
expect_cam "22 webcam decodificata dall'host" "ok frames=" tech1 45
expect_ask() { # descrizione atteso modo
  local got; got="$(ctrl tech1 camera-ask "$HOST_ID" "$PASSWORD" "$3" 2>/dev/null | sed -n 's/^RESULT=//p')"
  if [ "$got" = "$2" ]; then PASS=$((PASS+1)); echo "  OK    $1 -> $got"
  else FAIL=$((FAIL+1)); echo "  FAIL  $1 -> atteso $2, ottenuto '$got'"; fi
}
hopt clanto-forward-webcam N
expect_ask "23 webcam dell'host chiesta con forwarding webcam spento" "denied:Webcam forwarding is disabled on this device" request
hopt clanto-forward-webcam Y
expect_ask "24 webcam dell'host scelta senza consenso" "closed:The remote user did not allow the webcam" pull

echo
echo "E2E 2FA, forwarding e webcam: $PASS superati, $FAIL falliti"
if [ "$FAIL" != 0 ]; then
  echo "--- log host (ultime righe) ---"; docker exec e2e-host tail -40 /tmp/server.log || true
  exit 1
fi
