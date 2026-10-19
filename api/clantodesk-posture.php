<?php
// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0
declare(strict_types=1);

/*
 * ClantoDesk: verifica della postura dei dispositivi (PHP 8.1+, nessuna dipendenza).
 *
 * RICHIESTA: POST application/json (max 16 KB)
 *   {"posture":{"platform":..,"os_build":..,"av_enabled":"true",..},"mac":"<base64>",
 *    "challenge":"..","host_id":"..","peer_platform":"..","peer_version":".."}
 *   I valori della postura sono sempre stringhe; max 64 chiavi.
 * RISPOSTA: sempre HTTP 200, {"ok":bool,"reasons":[string],
 *   "failed":[{"id":string,"message":string,"bypassable":bool}]}; reasons = message di failed.
 *   Controlli di base (id "core:*", mai bypassable): core:request, core:config, core:signature,
 *   core:coherence, core:revoked, core:policy:<id> (regola malformata), core:unavailable.
 *   Se un controllo di base fallisce si risponde subito, senza valutare le regole.
 * GET ?rules[&host_id=..]: {"rules":[{"id","message","bypassable","platforms"}]}, solo regole
 *   enabled; con host_id solo quelle che si applicano all'host. Altri GET: core:request.
 *
 * FIRMA (identica al client Rust):
 *   chiave  = HMAC-SHA256("clanto-posture-key\n" . peer_version, CLANTO_POSTURE_MASTER)
 *   msg     = "clanto-posture-v1\n" host_id "\n" challenge "\n" peer_version "\n"
 *             + per ogni chiave della postura (ordine bytewise crescente): chiave "=" valore "\n"
 *   mac     = HMAC-SHA256(msg, chiave), binario, trasmesso in base64.
 *
 * POLICY (JSON, CLANTO_POSTURE_POLICY o posture-policy.json accanto a questo file):
 *   {"version":1,"revoked_versions":[..],"groups":{"nome":["host_id",..]},
 *    "eol":{"<platform>":[{"field","regex" (senza delimitatori),"edition_regex" (opz.),
 *              "date":"YYYY-MM-DD"}]},
 *    "rules":[{"id","enabled","platforms":[..],"hosts":[..],"groups":[..],"field","op","value",
 *              "message","allow_missing","unknown_is","bypassable"}]}
 *   Operatori su singolo valore: eq, ne, in, not_in (value array), regex (PCRE completo),
 *     version_gte, version_lt, num_gte, num_lte, present / not_unknown (presente e != "unknown"),
 *     date_within_days (campo "YYYY-MM-DD"; value = intero > 0; passa se la data non è più vecchia
 *     di value giorni rispetto a oggi UTC; data non parsabile: fallisce; value non valido:
 *     core:policy:<id>).
 *   Operatori su liste (elementi separati da "|", value = regex): any_match, none_match, all_match.
 *     Lista vuota o "unknown": none_match passa, gli altri falliscono.
 *   Operatori senza "field":
 *     schedule: value {"tz":"Europe/Rome","windows":[{"days":[1..7],"from":"HH:MM","to":"HH:MM"}]},
 *       giorni ISO (1 = lunedì); passa se l'ora corrente nel fuso cade in una finestra [from,to).
 *     not_eol: cerca nella tabella eol della piattaforma la prima voce (ordine del JSON: le
 *       specifiche prima delle generiche) la cui regex corrisponde al campo della voce; se la voce
 *       ha "edition_regex" deve corrispondere anche a posture.os_edition (assente o "unknown":
 *       la voce non corrisponde). Fallisce se la data è strettamente passata (UTC); nessuna voce:
 *       passa. Regex malformata: core:policy:<id della regola>.
 *   Campo assente: regola fallita, salvo "allow_missing": true.
 *   Campo "unknown": esito "unknown_is" ("pass" | "fail", default fail); non vale per present /
 *     not_unknown, né per gli operatori su liste se unknown_is è omesso.
 *   Ambito: "hosts" e/o "groups" presenti = la regola vale solo per quegli host o per i membri
 *     dei gruppi (unione). "bypassable" default true.
 *   Regola malformata (operatore ignoto, parametri non validi): core:policy:<id>.
 *   Si restituiscono tutte le regole fallite.
 */

const MAX_BODY = 16384;
const MAX_POSTURE_KEYS = 64;
const MAX_KEY_LEN = 64;
const MAX_VALUE_LEN = 256;
const MAX_ID_LEN = 128;
const MAX_MAC_LEN = 128;

function failure(string $id, string $message, bool $bypassable = false): array
{
    return ['id' => $id, 'message' => $message, 'bypassable' => $bypassable];
}

function send_json(array $payload): never
{
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
    echo json_encode($payload, JSON_UNESCAPED_UNICODE);
    exit;
}

// $failed: elenco di failure(); ok = nessun fallimento.
function respond(array $failed): never
{
    $failed = array_values($failed);
    send_json([
        'ok' => $failed === [],
        'reasons' => array_column($failed, 'message'),
        'failed' => $failed,
    ]);
}

function respond_core(string $id, string $message): never
{
    respond([failure($id, $message)]);
}

// Stringa breve e stampabile, sicura per il log.
function safe_log(string $s): string
{
    return substr(preg_replace('/[^\x20-\x7E]/', '?', $s) ?? '', 0, 64);
}

function str_field(array $a, string $k, int $max, bool $allowEmpty = false): string
{
    if (!isset($a[$k]) || !is_string($a[$k]) || strlen($a[$k]) > $max || (!$allowEmpty && $a[$k] === '')) {
        throw new InvalidArgumentException('campo non valido');
    }
    return $a[$k];
}

function load_policy(): array
{
    $path = getenv('CLANTO_POSTURE_POLICY');
    if (!is_string($path) || $path === '') {
        $path = __DIR__ . '/posture-policy.json';
    }
    $raw = file_get_contents($path);
    if ($raw === false) {
        throw new RuntimeException('policy illeggibile');
    }
    $p = json_decode($raw, true, 16, JSON_THROW_ON_ERROR);
    if (!is_array($p) || !isset($p['rules']) || !is_array($p['rules'])
        || (isset($p['revoked_versions']) && !is_array($p['revoked_versions']))
        || (isset($p['groups']) && !is_array($p['groups']))
        || (isset($p['eol']) && !is_array($p['eol']))) {
        throw new RuntimeException('policy malformata');
    }
    return $p;
}

// preg_match che non lancia: null se pattern non valido o errore PCRE.
function safe_preg(string $pattern, string $subject): ?int
{
    try {
        $r = preg_match($pattern, $subject);
    } catch (ErrorException) {
        return null;
    }
    return $r === false ? null : $r;
}

// Elementi di un campo lista ("a|b|c"); vuoto o "unknown" = lista vuota.
function split_list(string $v): array
{
    if ($v === '' || $v === 'unknown') {
        return [];
    }
    return array_values(array_filter(array_map('trim', explode('|', $v)), static fn ($x) => $x !== ''));
}

// true = regola soddisfatta; null = operatore o parametri non validi.
function eval_op(string $op, string $v, mixed $val): ?bool
{
    $scalar = is_scalar($val);
    switch ($op) {
        case 'eq':
            return $scalar ? $v === (string)$val : null;
        case 'ne':
            return $scalar ? $v !== (string)$val : null;
        case 'in':
            return is_array($val) ? in_array($v, array_map('strval', $val), true) : null;
        case 'not_in':
            return is_array($val) ? !in_array($v, array_map('strval', $val), true) : null;
        case 'regex':
            if (!is_string($val) || safe_preg($val, '') === null) {
                return null;
            }
            return safe_preg($val, $v) === 1;
        case 'any_match':
        case 'none_match':
        case 'all_match':
            if (!is_string($val) || safe_preg($val, '') === null) {
                return null;
            }
            $items = split_list($v);
            $hits = 0;
            foreach ($items as $it) {
                $hits += safe_preg($val, $it) === 1 ? 1 : 0;
            }
            if ($op === 'any_match') {
                return $hits > 0;
            }
            if ($op === 'none_match') {
                return $hits === 0;
            }
            return $items !== [] && $hits === count($items);
        case 'version_gte':
            return $scalar ? version_compare($v, (string)$val, '>=') : null;
        case 'version_lt':
            return $scalar ? version_compare($v, (string)$val, '<') : null;
        case 'num_gte':
        case 'num_lte':
            if (!is_numeric($val)) {
                return null;
            }
            if (!is_numeric($v)) {
                return false;
            }
            return $op === 'num_gte' ? (float)$v >= (float)$val : (float)$v <= (float)$val;
        case 'date_within_days':
            if (!is_int($val) || $val <= 0) {
                return null;
            }
            $utc = new DateTimeZone('UTC');
            $d = DateTimeImmutable::createFromFormat('!Y-m-d', $v, $utc);
            if ($d === false || $d->format('Y-m-d') !== $v) {
                return false;
            }
            return $d >= (new DateTimeImmutable('today', $utc))->modify("-$val days");
        case 'present':
        case 'not_unknown':
            return $v !== '' && $v !== 'unknown';
        default:
            return null;
    }
}

function parse_hhmm(mixed $s): ?int
{
    if (!is_string($s)) {
        return null;
    }
    if ($s === '24:00') {
        return 1440;
    }
    if (preg_match('/^([01]\d|2[0-3]):([0-5]\d)$/', $s, $m) !== 1) {
        return null;
    }
    return (int)$m[1] * 60 + (int)$m[2];
}

// schedule: true se adesso, nel fuso indicato, cade in una finestra; null se value malformato.
function eval_schedule(mixed $val): ?bool
{
    if (!is_array($val) || !is_string($val['tz'] ?? null) || !is_array($val['windows'] ?? null)
        || $val['windows'] === []) {
        return null;
    }
    try {
        $now = new DateTimeImmutable('now', new DateTimeZone($val['tz']));
    } catch (Exception) {
        return null;
    }
    $day = (int)$now->format('N');
    $minutes = (int)$now->format('G') * 60 + (int)$now->format('i');
    $inside = false;
    foreach ($val['windows'] as $w) {
        if (!is_array($w) || !is_array($w['days'] ?? null) || $w['days'] === []) {
            return null;
        }
        $from = parse_hhmm($w['from'] ?? null);
        $to = parse_hhmm($w['to'] ?? null);
        if ($from === null || $to === null || $from >= $to) {
            return null;
        }
        foreach ($w['days'] as $d) {
            if (!is_int($d) || $d < 1 || $d > 7) {
                return null;
            }
        }
        if (in_array($day, $w['days'], true) && $minutes >= $from && $minutes < $to) {
            $inside = true;
        }
    }
    return $inside;
}

// not_eol: false se la prima voce corrispondente ha data strettamente passata; null se tabella malformata.
function eval_not_eol(array $posture, array $eol): ?bool
{
    $entries = $eol[$posture['platform'] ?? ''] ?? [];
    if (!is_array($entries)) {
        return null;
    }
    $utc = new DateTimeZone('UTC');
    $edition = $posture['os_edition'] ?? '';
    $match = null;
    foreach ($entries as $e) {
        if (!is_array($e) || !is_string($e['field'] ?? null) || !is_string($e['regex'] ?? null)
            || !is_string($e['date'] ?? null)
            || (isset($e['edition_regex']) && !is_string($e['edition_regex']))) {
            return null;
        }
        // Le regex delle voci eol sono senza delimitatori.
        $re = '~' . str_replace('~', '\~', $e['regex']) . '~';
        $edRe = isset($e['edition_regex']) ? '~' . str_replace('~', '\~', $e['edition_regex']) . '~' : null;
        if (safe_preg($re, '') === null || ($edRe !== null && safe_preg($edRe, '') === null)) {
            return null;
        }
        $date = DateTimeImmutable::createFromFormat('!Y-m-d', $e['date'], $utc);
        if ($date === false || $date->format('Y-m-d') !== $e['date']) {
            return null;
        }
        if ($match === null && array_key_exists($e['field'], $posture)
            && safe_preg($re, $posture[$e['field']]) === 1
            && ($edRe === null || ($edition !== '' && $edition !== 'unknown' && safe_preg($edRe, $edition) === 1))) {
            $match = $date;
        }
    }
    return $match === null ? true : new DateTimeImmutable('today', $utc) <= $match;
}

// Ambito per host: true = si applica; null = hosts/groups malformati.
function in_host_scope(array $rule, array $groups, ?string $hostId): ?bool
{
    $hosts = $rule['hosts'] ?? null;
    $names = $rule['groups'] ?? null;
    if ($hosts === null && $names === null) {
        return true;
    }
    foreach ([$hosts, $names] as $list) {
        if ($list !== null && (!is_array($list) || array_filter($list, 'is_string') !== $list)) {
            return null;
        }
    }
    if ($hostId === null) {
        return true;
    }
    if (is_array($hosts) && in_array($hostId, $hosts, true)) {
        return true;
    }
    foreach ($names ?? [] as $n) {
        $members = $groups[$n] ?? [];
        if (is_array($members) && in_array($hostId, $members, true)) {
            return true;
        }
    }
    return false;
}

// Valuta le regole: restituisce i fallimenti e accumula in $policyErrors le regole malformate.
function evaluate_rules(array $policy, array $posture, string $hostId, array &$policyErrors): array
{
    $groups = is_array($policy['groups'] ?? null) ? $policy['groups'] : [];
    $eol = is_array($policy['eol'] ?? null) ? $policy['eol'] : [];
    $failed = [];
    foreach ($policy['rules'] as $rule) {
        $id = is_array($rule) && is_string($rule['id'] ?? null) ? substr($rule['id'], 0, MAX_ID_LEN) : '?';
        $malformed = static function () use ($id, &$policyErrors): void {
            $policyErrors[] = failure('core:policy:' . $id, 'policy non valida: ' . $id);
        };
        if (!is_array($rule)) {
            $malformed();
            continue;
        }
        if (($rule['enabled'] ?? true) !== true) {
            continue;
        }
        $platforms = $rule['platforms'] ?? [];
        if (is_array($platforms) && $platforms !== [] && !in_array($posture['platform'], $platforms, true)) {
            continue;
        }
        $scope = in_host_scope($rule, $groups, $hostId);
        $bypassable = $rule['bypassable'] ?? true;
        $unknownIs = $rule['unknown_is'] ?? null;
        $op = $rule['op'] ?? null;
        if ($scope === null || !is_bool($bypassable) || !is_string($op)
            || ($unknownIs !== null && $unknownIs !== 'pass' && $unknownIs !== 'fail')) {
            $malformed();
            continue;
        }
        if (!$scope) {
            continue;
        }

        if ($op === 'schedule') {
            $pass = eval_schedule($rule['value'] ?? null);
        } elseif ($op === 'not_eol') {
            $pass = eval_not_eol($posture, $eol);
        } else {
            $field = $rule['field'] ?? null;
            if (!is_string($field)) {
                $malformed();
                continue;
            }
            if (!array_key_exists($field, $posture)) {
                if (($rule['allow_missing'] ?? false) === true) {
                    continue;
                }
                $pass = false;
            } else {
                $v = $posture[$field];
                $listOp = in_array($op, ['any_match', 'none_match', 'all_match'], true);
                $noUnknown = in_array($op, ['present', 'not_unknown'], true);
                if ($v === 'unknown' && !$noUnknown && ($unknownIs !== null || !$listOp)) {
                    // Parametri validati comunque, poi esito deciso da unknown_is (default fail).
                    $pass = eval_op($op, $v, $rule['value'] ?? null) === null ? null : $unknownIs === 'pass';
                } else {
                    $pass = eval_op($op, $v, $rule['value'] ?? null);
                }
            }
        }
        if ($pass === null) {
            $malformed();
            continue;
        }
        if (!$pass) {
            $msg = $rule['message'] ?? null;
            $failed[] = failure(
                $id,
                is_string($msg) && $msg !== '' ? $msg : 'requisito non soddisfatto: ' . $id,
                $bypassable
            );
        }
    }
    return $failed;
}

// GET ?rules: elenco pubblico delle regole attive, senza valori né soglie.
function list_rules(): never
{
    $hostId = null;
    if (isset($_GET['host_id'])) {
        if (!is_string($_GET['host_id']) || $_GET['host_id'] === '' || strlen($_GET['host_id']) > MAX_ID_LEN) {
            respond_core('core:request', 'richiesta non valida');
        }
        $hostId = $_GET['host_id'];
    }
    try {
        $policy = load_policy();
    } catch (Throwable) {
        respond_core('core:config', 'policy non leggibile');
    }
    $groups = is_array($policy['groups'] ?? null) ? $policy['groups'] : [];
    $out = [];
    foreach ($policy['rules'] as $rule) {
        if (!is_array($rule) || ($rule['enabled'] ?? true) !== true || !is_string($rule['id'] ?? null)) {
            continue;
        }
        if (in_host_scope($rule, $groups, $hostId) === false) {
            continue;
        }
        $id = substr($rule['id'], 0, MAX_ID_LEN);
        $msg = $rule['message'] ?? null;
        $out[] = [
            'id' => $id,
            'message' => is_string($msg) && $msg !== '' ? $msg : 'requisito non soddisfatto: ' . $id,
            'bypassable' => ($rule['bypassable'] ?? true) !== false,
            'platforms' => is_array($rule['platforms'] ?? null) ? array_values($rule['platforms']) : [],
        ];
    }
    send_json(['rules' => $out]);
}

$hostForLog = '-';
try {
    set_error_handler(static function (int $no, string $str, string $file, int $line): bool {
        throw new ErrorException($str, 0, $no, $file, $line);
    });

    $method = $_SERVER['REQUEST_METHOD'] ?? '';
    if ($method === 'GET' && isset($_GET['rules'])) {
        list_rules();
    }
    if ($method !== 'POST') {
        respond_core('core:request', 'richiesta non valida: metodo non consentito');
    }
    $master = getenv('CLANTO_POSTURE_MASTER');
    if (!is_string($master) || $master === '') {
        // ISPConfig: <sito>/private/ sta accanto a <sito>/web/ e nginx non lo serve.
        $file = __DIR__ . '/../../private/clanto-posture.secret';
        $master = is_readable($file) ? trim((string) file_get_contents($file)) : '';
    }
    if ($master === '') {
        error_log('clanto-posture: master non configurato');
        respond_core('core:config', 'verifica non configurata');
    }

    $body = file_get_contents('php://input', false, null, 0, MAX_BODY + 1);
    if ($body === false || $body === '' || strlen($body) > MAX_BODY) {
        respond_core('core:request', 'richiesta non valida');
    }
    try {
        $req = json_decode($body, true, 8, JSON_THROW_ON_ERROR);
        if (!is_array($req) || !isset($req['posture']) || !is_array($req['posture'])
            || count($req['posture']) > MAX_POSTURE_KEYS) {
            throw new InvalidArgumentException('formato');
        }
        $hostId = str_field($req, 'host_id', MAX_ID_LEN);
        $challenge = str_field($req, 'challenge', MAX_VALUE_LEN);
        $peerVersion = str_field($req, 'peer_version', MAX_ID_LEN);
        $peerPlatform = str_field($req, 'peer_platform', MAX_ID_LEN);
        $mac = str_field($req, 'mac', MAX_MAC_LEN);
        $posture = [];
        foreach ($req['posture'] as $k => $v) {
            $k = (string)$k;
            if ($k === '' || strlen($k) > MAX_KEY_LEN || !is_string($v) || strlen($v) > MAX_VALUE_LEN) {
                throw new InvalidArgumentException('postura');
            }
            $posture[$k] = $v;
        }
    } catch (JsonException | InvalidArgumentException) {
        respond_core('core:request', 'richiesta non valida');
    }
    $hostForLog = safe_log($hostId);

    // Firma: stesso formato canonico del client.
    $releaseKey = hash_hmac('sha256', "clanto-posture-key\n" . $peerVersion, $master, true);
    ksort($posture, SORT_STRING);
    $msg = "clanto-posture-v1\n" . $hostId . "\n" . $challenge . "\n" . $peerVersion . "\n";
    foreach ($posture as $k => $v) {
        $msg .= $k . '=' . $v . "\n";
    }
    $expected = hash_hmac('sha256', $msg, $releaseKey, true);
    $given = base64_decode($mac, true);
    if ($given === false || !hash_equals($expected, $given)) {
        error_log("clanto-posture host_id=$hostForLog esito=ko firma");
        respond_core('core:signature', 'client non riconosciuto: usare ClantoDesk ufficiale');
    }

    if (($posture['platform'] ?? null) !== $peerPlatform || ($posture['client_version'] ?? null) !== $peerVersion) {
        error_log("clanto-posture host_id=$hostForLog esito=ko coerenza");
        respond_core('core:coherence', 'dati del dispositivo incoerenti: usare ClantoDesk ufficiale');
    }

    try {
        $policy = load_policy();
    } catch (RuntimeException | JsonException) {
        error_log("clanto-posture host_id=$hostForLog esito=ko policy");
        respond_core('core:config', 'policy non leggibile');
    }
    $revoked = array_map('strval', array_filter($policy['revoked_versions'] ?? [], 'is_scalar'));
    if (in_array($peerVersion, $revoked, true)) {
        error_log("clanto-posture host_id=$hostForLog esito=ko versione_revocata");
        respond_core('core:revoked', 'versione di ClantoDesk non più ammessa, aggiornare');
    }

    $policyErrors = [];
    $failed = evaluate_rules($policy, $posture, $hostId, $policyErrors);
    if ($policyErrors !== []) {
        error_log("clanto-posture host_id=$hostForLog esito=ko policy_regole");
        respond($policyErrors);
    }
    if ($failed === []) {
        error_log("clanto-posture host_id=$hostForLog esito=ok");
        respond([]);
    }
    error_log("clanto-posture host_id=$hostForLog esito=ko regole=" . safe_log(implode(',', array_column($failed, 'id'))));
    respond($failed);
} catch (Throwable $e) {
    error_log("clanto-posture host_id=$hostForLog errore=" . get_class($e));
    respond_core('core:unavailable', 'verifica non disponibile');
}
