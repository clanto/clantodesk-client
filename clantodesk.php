<?php
/**
 * ClantoDesk Update & Download Hub
 * Gestisce sia le richieste API dei client che la pagina di download per i browser.
 */

// --- CONFIGURAZIONE ---
$LATEST_TAG = '1.4.9';
$UPDATE_ENABLED = true;

// Store Links
$PLAY_STORE_URL = "https://play.google.com/store/apps/details?id=it.clanto.clantodesk";
$APP_STORE_URL  = "https://apps.apple.com/app/clantodesk/id6740578631";

// GitHub Base URL
$REPO_URL = "https://github.com/clanto/clantodesk-client";
$RELEASE_URL = "{$REPO_URL}/releases/tag/{$LATEST_TAG}";
$DOWNLOAD_BASE = "{$REPO_URL}/releases/download/{$LATEST_TAG}";

// Mappa File (basata sui nomi generati dalle GitHub Actions)
$FILES = [
    // Windows
    'win_x64_exe'    => "clantodesk-{$LATEST_TAG}-x86_64.exe",
    'win_x64_msi'    => "clantodesk-{$LATEST_TAG}-x86_64.msi",
    'win_x86_sciter' => "clantodesk-{$LATEST_TAG}-x86-sciter.exe",
    'win_arm_exe'    => "clantodesk-{$LATEST_TAG}-aarch64.exe",
    'win_arm_msi'    => "clantodesk-{$LATEST_TAG}-aarch64.msi",
    
    // macOS
    'mac_x64'        => "clantodesk-{$LATEST_TAG}-x86_64.dmg",
    'mac_arm'        => "clantodesk-{$LATEST_TAG}-aarch64.dmg",
    
    // Linux Debian/Ubuntu
    'linux_deb_x64'  => "clantodesk-{$LATEST_TAG}-x86_64.deb",
    'linux_deb_arm'  => "clantodesk-{$LATEST_TAG}-aarch64.deb",
    'linux_deb_sciter_arm7' => "clantodesk-{$LATEST_TAG}-armv7-sciter.deb",
    
    // Linux AppImage
    'linux_app_x64'  => "clantodesk-{$LATEST_TAG}-x86_64.AppImage",
    'linux_app_arm'  => "clantodesk-{$LATEST_TAG}-aarch64.AppImage",
    
    // Linux Flatpak
    'linux_flat_x64' => "clantodesk-{$LATEST_TAG}-x86_64.flatpak",
    'linux_flat_arm' => "clantodesk-{$LATEST_TAG}-aarch64.flatpak",
    
    // Linux RPM (Fedora/openSUSE)
    'linux_rpm_x64'  => "clantodesk-{$LATEST_TAG}-0.x86_64.rpm",
    'linux_rpm_arm'  => "clantodesk-{$LATEST_TAG}-0.aarch64.rpm",
    'linux_rpm_suse_x64' => "clantodesk-{$LATEST_TAG}-0.x86_64-suse.rpm",
    'linux_rpm_suse_arm' => "clantodesk-{$LATEST_TAG}-0.aarch64-suse.rpm",
    
    // Android APK - variante client (CONN_TYPE=outgoing), it.clanto.clantodesk
    'android_univ'   => "clantodesk-{$LATEST_TAG}-universal.apk",
    'android_x64'    => "clantodesk-{$LATEST_TAG}-x86_64-signed.apk",
    'android_arm'    => "clantodesk-{$LATEST_TAG}-aarch64-signed.apk",
    'android_armv7'  => "clantodesk-{$LATEST_TAG}-armv7-signed.apk",

    // Android APK - variante host (CONN_TYPE=incoming), it.clanto.clantodesk.host.
    // Nessun universale: il job universal e' fissato al ruolo client.
    'android_host_x64'   => "clantodesk-{$LATEST_TAG}-host-x86_64-signed.apk",
    'android_host_arm'   => "clantodesk-{$LATEST_TAG}-host-aarch64-signed.apk",
    'android_host_armv7' => "clantodesk-{$LATEST_TAG}-host-armv7-signed.apk"
];

// --- PROXY DI DOWNLOAD ---
// I binari escono da questo dominio, non da GitHub: i Secure Web Gateway
// aziendali e scolastici bloccano il download di eseguibili da github.com e da
// objects.githubusercontent.com, dominio molto abusato per distribuire malware.
// Mai un redirect verso GitHub: Safe Browsing ispeziona tutta la catena di
// redirect, quindi un 302 vanificherebbe il proxy.
// Nessun effetto su SmartScreen, che valuta solo certificato e hash del file.

// Cache fuori dalla webroot. Se manca o non e' scrivibile si strema senza cache.
$DL_CACHE_DIR = dirname(__DIR__) . '/clantodesk-dl-cache';
// 'none' strema da PHP. 'xsendfile' richiede mod_xsendfile (Apache).
// 'accel' richiede una internal location nginx mappata su $DL_CACHE_DIR.
$DL_SERVE_MODE = 'none';
$DL_ACCEL_LOCATION = '/clanto-dl-internal';

function dl_fail($code, $msg) {
    if (!headers_sent()) {
        http_response_code($code);
        header('Content-Type: text/plain; charset=utf-8');
    }
    echo $msg . "\n";
}

// Scarica una volta nella cache. Il lock evita che N visitatori simultanei
// scarichino N copie dello stesso file.
function dl_fetch_to_cache($url, $path) {
    $dir = dirname($path);
    if (!is_dir($dir) && !@mkdir($dir, 0755, true)) return false;
    if (!is_writable($dir)) return false;

    $lh = @fopen($path . '.lock', 'c');
    if (!$lh) return false;
    if (!flock($lh, LOCK_EX)) { fclose($lh); return false; }

    clearstatcache(true, $path);
    if (is_file($path) && filesize($path) > 0) {
        flock($lh, LOCK_UN); fclose($lh);
        return true;
    }

    $tmp = $path . '.part';
    $out = @fopen($tmp, 'wb');
    if (!$out) { flock($lh, LOCK_UN); fclose($lh); return false; }

    $ok = false;
    if (function_exists('curl_init')) {
        $ch = curl_init($url);
        curl_setopt_array($ch, [
            CURLOPT_FILE           => $out,
            CURLOPT_FOLLOWLOCATION => true,
            CURLOPT_MAXREDIRS      => 5,
            CURLOPT_CONNECTTIMEOUT => 15,
            CURLOPT_TIMEOUT        => 900,
            CURLOPT_FAILONERROR    => true,
            CURLOPT_USERAGENT      => 'ClantoDesk-DownloadProxy',
        ]);
        $ok = (curl_exec($ch) !== false)
              && (int)curl_getinfo($ch, CURLINFO_RESPONSE_CODE) === 200;
        curl_close($ch);
    } else {
        $in = @fopen($url, 'rb');
        if ($in) { $ok = stream_copy_to_stream($in, $out) !== false; fclose($in); }
    }
    fclose($out);

    clearstatcache(true, $tmp);
    if ($ok && is_file($tmp) && filesize($tmp) > 0) {
        $ok = @rename($tmp, $path);
    } else {
        $ok = false;
    }
    if (!$ok) @unlink($tmp);

    flock($lh, LOCK_UN); fclose($lh);
    return $ok;
}

function dl_serve_local($path, $name, $tag, $mode, $accel) {
    clearstatcache(true, $path);
    $size = filesize($path);

    header('Content-Type: application/octet-stream');
    header('Content-Disposition: attachment; filename="' . $name . '"');
    header('Accept-Ranges: bytes');
    header('X-Content-Type-Options: nosniff');
    header('Cache-Control: public, max-age=86400');

    // Con xsendfile o accel il webserver gestisce da solo Range, ETag e sendfile.
    if ($mode === 'xsendfile') { header('X-Sendfile: ' . $path); return; }
    if ($mode === 'accel') {
        header('X-Accel-Redirect: ' . rtrim($accel, '/') . '/' . rawurlencode($tag)
               . '/' . rawurlencode($name));
        return;
    }

    $start = 0;
    $end   = $size - 1;
    if (preg_match('/^bytes=(\d*)-(\d*)$/', $_SERVER['HTTP_RANGE'] ?? '', $m)
        && ($m[1] !== '' || $m[2] !== '')) {
        if ($m[1] === '') {
            $start = max(0, $size - (int)$m[2]);
        } else {
            $start = (int)$m[1];
            if ($m[2] !== '') $end = min((int)$m[2], $size - 1);
        }
        if ($start > $end || $start >= $size) {
            http_response_code(416);
            header("Content-Range: bytes */{$size}");
            return;
        }
        http_response_code(206);
        header("Content-Range: bytes {$start}-{$end}/{$size}");
    }

    header('Content-Length: ' . ($end - $start + 1));
    if (($_SERVER['REQUEST_METHOD'] ?? 'GET') === 'HEAD') return;

    $fp = fopen($path, 'rb');
    if (!$fp) { dl_fail(500, 'Errore di lettura.'); return; }
    fseek($fp, $start);
    $left = $end - $start + 1;
    while ($left > 0 && !feof($fp) && connection_status() === CONNECTION_NORMAL) {
        $chunk = fread($fp, (int)min(262144, $left));
        if ($chunk === false || $chunk === '') break;
        $left -= strlen($chunk);
        echo $chunk;
        flush();
    }
    fclose($fp);
}

// Usata quando la cache non e' disponibile: i byte passano da qui senza toccare
// il disco, GitHub resta invisibile al client.
function dl_stream_passthrough($url, $name) {
    if (!function_exists('curl_init')) {
        $in = @fopen($url, 'rb');
        if (!$in) { dl_fail(502, 'Download non disponibile, riprova piu\' tardi.'); return; }
        header('Content-Type: application/octet-stream');
        header('Content-Disposition: attachment; filename="' . $name . '"');
        header('X-Content-Type-Options: nosniff');
        fpassthru($in);
        fclose($in);
        return;
    }

    $status = 0;
    $sent   = false;
    $ch = curl_init($url);
    curl_setopt_array($ch, [
        CURLOPT_FOLLOWLOCATION => true,
        CURLOPT_MAXREDIRS      => 5,
        CURLOPT_CONNECTTIMEOUT => 15,
        CURLOPT_TIMEOUT        => 900,
        CURLOPT_USERAGENT      => 'ClantoDesk-DownloadProxy',
        CURLOPT_HTTPHEADER     => isset($_SERVER['HTTP_RANGE'])
                                  ? ['Range: ' . $_SERVER['HTTP_RANGE']] : [],
        CURLOPT_HEADERFUNCTION => function ($ch2, $line) use (&$status, $name) {
            if (preg_match('#^HTTP/[\d.]+\s+(\d{3})#', $line, $m)) {
                $status = (int)$m[1];   // l'ultima riga di stato vince: e' quella finale
                return strlen($line);
            }
            if ($status >= 200 && $status < 300
                && preg_match('/^(Content-Length|Content-Range|Accept-Ranges):/i', $line)) {
                if (!headers_sent()) {
                    http_response_code($status);
                    header('Content-Type: application/octet-stream');
                    header('Content-Disposition: attachment; filename="' . $name . '"');
                    header('X-Content-Type-Options: nosniff');
                    header(rtrim($line, "\r\n"));
                }
            }
            return strlen($line);
        },
        CURLOPT_WRITEFUNCTION  => function ($ch2, $data) use (&$status, &$sent) {
            if ($status < 200 || $status >= 300) return strlen($data); // scarta l'errore
            $sent = true;
            echo $data;
            flush();
            return strlen($data);
        },
    ]);
    curl_exec($ch);
    curl_close($ch);

    if (!$sent) dl_fail(502, 'Download non disponibile, riprova piu\' tardi.');
}

function dl_handle($key, $files, $base, $tag, $cacheDir, $mode, $accel) {
    // Whitelist per chiave: nessun nome di file e nessun percorso arriva
    // dall'esterno, altrimenti sarebbe una SSRF verso GitHub piu' un traversal.
    if (!is_string($key) || !array_key_exists($key, $files)) {
        dl_fail(404, 'File non disponibile.');
        return;
    }
    $name = basename($files[$key]);
    $tag  = basename($tag);
    $path = rtrim($cacheDir, '/') . '/' . $tag . '/' . $name;
    $url  = rtrim($base, '/') . '/' . $name;

    clearstatcache(true, $path);
    if ((is_file($path) && filesize($path) > 0) || dl_fetch_to_cache($url, $path)) {
        dl_serve_local($path, $name, $tag, $mode, $accel);
    } else {
        dl_stream_passthrough($url, $name);
    }
}

if (isset($_GET['dl'])) {
    @set_time_limit(0);
    @ini_set('zlib.output_compression', '0');
    while (ob_get_level() > 0) { ob_end_clean(); }
    dl_handle($_GET['dl'], $FILES, $DOWNLOAD_BASE, $LATEST_TAG,
              $DL_CACHE_DIR, $DL_SERVE_MODE, $DL_ACCEL_LOCATION);
    exit;
}

// --- LOGICA DI INSTRADAMENTO ---

// Verifica se è una chiamata API (JSON)
$is_api = (strpos($_SERVER['HTTP_ACCEPT'] ?? '', 'application/json') !== false) || 
          (strpos($_SERVER['CONTENT_TYPE'] ?? '', 'application/json') !== false) ||
          ($_SERVER['REQUEST_METHOD'] === 'POST');

if ($is_api) {
    header('Content-Type: application/json');
    if (!$UPDATE_ENABLED) {
        echo json_encode(['url' => "{$REPO_URL}/releases/tag/0.0.0"]);
        exit;
    }
    
    // Per il client, restituiamo l'URL della release (RustDesk logic)
    echo json_encode([
        'url' => $RELEASE_URL,
        'version' => $LATEST_TAG
    ]);
    exit;
}

// Funzione per recuperare la data della release da GitHub
function get_release_date($repo_url, $tag) {
    $api_url = str_replace("github.com", "api.github.com/repos", $repo_url) . "/releases/tags/{$tag}";
    $options = ['http' => ['method' => 'GET', 'header' => ['User-Agent: PHP']]];
    $ctx = stream_context_create($options);
    $response = @file_get_contents($api_url, false, $ctx);
    if ($response) {
        $data = json_decode($response, true);
        if (isset($data['published_at'])) {
            return date("d/m/Y", strtotime($data['published_at']));
        }
    }
    return date("d/m/Y"); // Fallback
}

$release_date = get_release_date($REPO_URL, $LATEST_TAG);

// --- PAGINA HTML PER BROWSER ---
?>
<!DOCTYPE html>
<html lang="it">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Scarica ClantoDesk v<?php echo $LATEST_TAG; ?></title>
    <link rel="icon" type="image/svg+xml" href="clantodesk.svg">
    <script src="https://cdn.tailwindcss.com"></script>
    <style>
        .loader { border-top-color: #3498db; animation: spinner 1.5s linear infinite; }
        @keyframes spinner { 0% { transform: rotate(0deg); } 100% { transform: rotate(360deg); } }
    </style>
</head>
<body class="bg-gray-900 text-white font-sans antialiased">

    <div class="max-w-4xl mx-auto px-4 py-12">
        <!-- Header: Logo e Titolo Affiancati -->
        <div class="flex flex-col items-center justify-center mb-12">
            <div class="flex items-center space-x-6 mb-6">
                <img src="clantodesk.svg" alt="ClantoDesk Logo" class="w-24 h-24 drop-shadow-2xl">
                <div class="text-left">
                    <h1 class="text-5xl font-extrabold text-white">
                        ClantoDesk
                    </h1>
                    <p class="text-xl text-gray-400">Il tuo desktop, ovunque tu sia.</p>
                </div>
            </div>
            <!-- Chip Versione -->
            <div class="inline-flex items-center px-4 py-1.5 rounded-full bg-gray-800 border border-gray-700 text-sm font-medium text-blue-400">
                <span class="flex h-2 w-2 rounded-full bg-blue-500 mr-2 animate-pulse"></span>
                Ultima versione: <?php echo $LATEST_TAG; ?> del <?php echo $release_date; ?>
            </div>
        </div>

        <!-- Analisi in corso -->
        <div id="analysing" class="bg-gray-800 rounded-3xl p-8 text-center shadow-2xl mb-8 border border-gray-700">
            <div class="loader ease-linear rounded-full border-4 border-t-4 border-gray-600 h-12 w-12 mb-4 mx-auto"></div>
            <h2 class="text-2xl font-semibold mb-2">Analisi del dispositivo in corso...</h2>
            <p id="status-text" class="text-gray-400 font-medium">Identificazione sistema operativo e architettura</p>
        </div>

        <!-- Download Principale (Dinamico) -->
        <div id="main-download" class="hidden space-y-6 mb-12">
            <div class="bg-blue-600 rounded-3xl p-8 text-center shadow-xl transform transition hover:scale-[1.02] border border-blue-400/30">
                <h3 class="text-3xl font-bold mb-4" id="detected-os">Versione Rilevata</h3>
                <div class="flex flex-wrap justify-center gap-4">
                    <a id="download-btn" href="#" class="inline-block bg-white text-blue-600 px-10 py-4 rounded-full font-bold text-xl hover:bg-gray-100 transition shadow-lg active:scale-95">
                        Scarica Ora
                    </a>
                    <a id="download-btn-alt" href="#" class="hidden items-center bg-blue-500 text-white px-10 py-4 rounded-full font-bold text-xl hover:bg-blue-400 transition shadow-lg active:scale-95">
                        Alternativa
                    </a>
                </div>
                <p id="download-note" class="hidden text-sm text-blue-100 mt-4"></p>
            </div>
        </div>

        <!-- Altre Versioni -->
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6 mb-16 text-sm">
            <!-- Windows -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700">
                <h4 class="text-lg font-bold mb-4 text-blue-400 flex items-center border-b border-gray-700 pb-2">
                    <svg class="w-5 h-5 mr-2" fill="currentColor" viewBox="0 0 20 20"><path d="M10 2l-7 3v10l7 3 7-3V5l-7-3z"/></svg>
                    Windows
                </h4>
                <div class="space-y-4">
                    <div class="group">
                        <p class="text-xs text-gray-500 mb-1 group-hover:text-blue-400 transition">Versione Desktop (x64)</p>
                        <div class="flex gap-2">
                            <a href="?dl=win_x64_exe" class="flex-1 text-center py-2 bg-gray-700 hover:bg-blue-600 rounded-lg transition font-semibold">.exe</a>
                            <a href="?dl=win_x64_msi" class="flex-1 text-center py-2 bg-gray-700 hover:bg-blue-600 rounded-lg transition font-semibold">.msi</a>
                        </div>
                    </div>
                    <div class="group border-t border-gray-700 pt-3">
                        <p class="text-xs text-gray-500 mb-1 group-hover:text-blue-400 transition">ARM64 (Snapdragon / Surface)</p>
                        <div class="flex gap-2">
                            <a href="?dl=win_arm_exe" class="flex-1 text-center py-2 bg-gray-700 hover:bg-blue-600 rounded-lg transition font-semibold">.exe</a>
                            <a href="?dl=win_arm_msi" class="flex-1 text-center py-2 bg-gray-700 hover:bg-blue-600 rounded-lg transition font-semibold">.msi</a>
                        </div>
                    </div>
                    <div class="group border-t border-gray-700 pt-3">
                        <p class="text-xs text-gray-500 mb-1 group-hover:text-blue-400 transition">Legacy / Sciter (x86)</p>
                        <a href="?dl=win_x86_sciter" class="block w-full text-center py-2 bg-gray-700 hover:bg-blue-600 rounded-lg transition font-semibold">windows 7</a>
                    </div>
                </div>
            </div>

            <!-- macOS -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700">
                <h4 class="text-lg font-bold mb-4 text-indigo-400 flex items-center border-b border-gray-700 pb-2">
                    <svg class="w-5 h-5 mr-2" fill="currentColor" viewBox="0 0 20 20"><path d="M10 18a8 8 0 100-16 8 8 0 000 16zM7 9a1 1 0 100-2 1 1 0 000 2zm6-1a1 1 0 11-2 0 1 1 0 012 0z"/></svg>
                    macOS
                </h4>
                <div class="space-y-4">
                    <div class="group">
                        <p class="text-xs text-gray-500 mb-1 group-hover:text-indigo-400 transition">Apple Silicon (M1/M2/M3)</p>
                        <a href="?dl=mac_arm" class="block w-full text-center py-2 bg-gray-700 hover:bg-indigo-600 rounded-lg transition font-semibold">ARM .dmg</a>
                    </div>
                    <div class="group border-t border-gray-700 pt-3">
                        <p class="text-xs text-gray-500 mb-1 group-hover:text-indigo-400 transition">Intel</p>
                        <a href="?dl=mac_x64" class="block w-full text-center py-2 bg-gray-700 hover:bg-indigo-600 rounded-lg transition font-semibold">x64 .dmg</a>
                    </div>
                </div>
            </div>

            <!-- Linux Universal -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700">
                <h4 class="text-lg font-bold mb-4 text-green-400 flex items-center border-b border-gray-700 pb-2">
                    <svg class="w-5 h-5 mr-2" fill="currentColor" viewBox="0 0 20 20"><path d="M2 10a8 8 0 018-8v8h8a8 8 0 11-16 0z"/></svg>
                    Universal Linux
                </h4>
                <div class="space-y-3">
                    <div class="group">
                        <p class="text-[10px] text-gray-500 mb-1 uppercase">AppImage</p>
                        <div class="flex gap-2 text-xs">
                            <a href="?dl=linux_app_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition font-semibold">x64</a>
                            <a href="?dl=linux_app_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition font-semibold">ARM64</a>
                        </div>
                    </div>
                    <div class="group border-t border-gray-700 pt-2 text-xs">
                        <p class="text-[10px] text-gray-500 mb-1 uppercase">Flatpak</p>
                        <div class="flex gap-2">
                            <a href="?dl=linux_flat_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition font-semibold">x64</a>
                            <a href="?dl=linux_flat_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition font-semibold">ARM64</a>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Linux Debian/Ubuntu -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700">
                <h4 class="text-lg font-bold mb-4 text-orange-400 flex items-center border-b border-gray-700 pb-2">
                    <svg class="w-5 h-5 mr-2" fill="currentColor" viewBox="0 0 20 20"><path d="M2 10a8 8 0 018-8v8h8a8 8 0 11-16 0z"/></svg>
                    Debian / Ubuntu
                </h4>
                <div class="space-y-3">
                    <div class="flex gap-2 text-[10px]">
                        <a href="?dl=linux_deb_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-orange-600 rounded-lg transition font-semibold">x86_64</a>
                        <a href="?dl=linux_deb_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-orange-600 rounded-lg transition font-semibold">aarch64</a>
                        <a href="?dl=linux_deb_sciter_arm7" class="flex-1 text-center py-2 bg-gray-700 hover:bg-orange-600 rounded-lg transition font-semibold">ARMv7</a>
                    </div>
                </div>
            </div>

            <!-- Linux RedHat/SUSE -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700">
                <h4 class="text-lg font-bold mb-4 text-red-400 flex items-center border-b border-gray-700 pb-2">
                    <svg class="w-5 h-5 mr-2" fill="currentColor" viewBox="0 0 20 20"><path d="M2 10a8 8 0 018-8v8h8a8 8 0 11-16 0z"/></svg>
                    RPM / Fedora
                </h4>
                <div class="space-y-3 text-xs">
                    <div class="group">
                        <p class="text-[10px] text-gray-500 mb-1 uppercase">Standard RPM</p>
                        <div class="flex gap-2">
                            <a href="?dl=linux_rpm_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-red-600 rounded-lg transition font-semibold">x64</a>
                            <a href="?dl=linux_rpm_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-red-600 rounded-lg transition font-semibold">ARM64</a>
                        </div>
                    </div>
                    <div class="group border-t border-gray-700 pt-2">
                        <p class="text-[10px] text-gray-500 mb-1 uppercase">openSUSE</p>
                        <div class="flex gap-2">
                            <a href="?dl=linux_rpm_suse_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-red-600 rounded-lg transition font-semibold">x64</a>
                            <a href="?dl=linux_rpm_suse_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-red-600 rounded-lg transition font-semibold">ARM64</a>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Android APKs -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700">
                <h4 class="text-lg font-bold mb-4 text-green-500 flex items-center border-b border-gray-700 pb-2">
                    <svg class="w-5 h-5 mr-2" fill="currentColor" viewBox="0 0 24 24"><path d="M17.523 15.3414c-.5511 0-.9993-.4486-.9993-.9997s.4482-.9997.9993-.9997c.5511 0 .9993.4486.9993.9997s-.4482.9997-.9993.9997m-11.046 0c-.5511 0-.9993-.4486-.9993-.9997s.4482-.9997.9993-.9997c.5511 0 .9993.4486.9993.9997s-.4482.9997-.9993.9997m11.4045-6.02l1.9973-3.4592a.416.416 0 0 0-.1521-.5676.416.416 0 0 0-.5676.1521l-2.0223 3.503c-1.5335-.6949-3.2433-1.0747-5.0388-1.0747s-3.5053.3798-5.0388 1.0747l-2.0223-3.503a.416.416 0 0 0-.5676-.1521.416.416 0 0 0-.1521.5676l1.9973 3.4592C3.1215 10.9204 1 13.5994 1 16.6914h22c0-3.092-2.1215-5.771-5.1185-7.37"></path></svg>
                    Android APK
                </h4>
                <div class="space-y-3">
                    <div class="group">
                        <p class="text-[10px] text-gray-500 mb-1 uppercase">ClantoDesk &mdash; solo controllo remoto</p>
                        <a href="?dl=android_univ" class="block w-full text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-xs font-semibold">Universal Installer</a>
                        <div class="flex flex-wrap gap-2 mt-2">
                            <a href="?dl=android_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-[10px] font-semibold">ARM64</a>
                            <a href="?dl=android_armv7" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-[10px] font-semibold">ARMv7</a>
                            <a href="?dl=android_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-[10px] font-semibold">x64</a>
                        </div>
                    </div>
                    <div class="group border-t border-gray-700 pt-2">
                        <p class="text-[10px] text-gray-500 mb-1 uppercase">ClantoDesk Host &mdash; dispositivo controllato</p>
                        <div class="flex flex-wrap gap-2">
                            <a href="?dl=android_host_arm" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-[10px] font-semibold">ARM64</a>
                            <a href="?dl=android_host_armv7" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-[10px] font-semibold">ARMv7</a>
                            <a href="?dl=android_host_x64" class="flex-1 text-center py-2 bg-gray-700 hover:bg-green-600 rounded-lg transition text-[10px] font-semibold">x64</a>
                        </div>
                        <p class="text-[10px] text-gray-600 mt-2 normal-case">Da installare sui monitor interattivi: non e' sul Play Store.</p>
                    </div>
                </div>
            </div>

            <!-- Mobile Stores -->
            <div class="bg-gray-800 p-6 rounded-2xl border border-gray-700 md:col-span-2 lg:col-span-3 text-center">
                <h4 class="text-lg font-bold mb-6 text-yellow-500">Scarica per il tuo cellulare</h4>
                <div class="flex flex-wrap justify-center gap-6">
                    <a href="<?php echo $PLAY_STORE_URL; ?>" target="_blank" class="transform transition hover:scale-105">
                        <img src="https://upload.wikimedia.org/wikipedia/commons/7/78/Google_Play_Store_badge_EN.svg" alt="Google Play" class="h-14">
                    </a>
                    <a href="<?php echo $APP_STORE_URL; ?>" target="_blank" class="transform transition hover:scale-105">
                        <img src="https://upload.wikimedia.org/wikipedia/commons/3/3c/Download_on_the_App_Store_Badge.svg" alt="App Store" class="h-14">
                    </a>
                </div>
            </div>
        </div>

        <!-- Info Release -->
        <div class="text-center space-y-4">
            <p class="text-gray-500 text-sm">
                Integrità dei file: Gli hash SHA sono disponibili nei dettagli tecnici della release ufficiale su GitHub.
            </p>
            <a href="<?php echo $RELEASE_URL; ?>" target="_blank" class="inline-block border border-gray-600 text-gray-300 px-6 py-2 rounded-full hover:bg-gray-700 transition">
                Dettagli Release su GitHub
            </a>
        </div>
    </div>

    <script>
        document.addEventListener('DOMContentLoaded', function() {
            const analysingBox = document.getElementById('analysing');
            const statusText = document.getElementById('status-text');
            const mainDownload = document.getElementById('main-download');
            const detectedOSText = document.getElementById('detected-os');
            const btn = document.getElementById('download-btn');
            const btnAlt = document.getElementById('download-btn-alt');
            const note = document.getElementById('download-note');

            // Chiavi del proxy, non nomi di file: i byte non escono da GitHub.
            const BASE = "?dl=";
            const F = {
                win_x64:   "win_x64_exe",
                win_arm:   "win_arm_exe",
                mac_arm:   "mac_arm",
                mac_x64:   "mac_x64",
                linux_x64: "linux_deb_x64",
                linux_arm: "linux_deb_arm"
            };
            const PLAY = "<?php echo $PLAY_STORE_URL; ?>";
            const APPSTORE = "<?php echo $APP_STORE_URL; ?>";

            // Solo Chromium dichiara l'architettura. Safari e Firefox no.
            async function archFromClientHints() {
                try {
                    if (navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
                        const d = await navigator.userAgentData.getHighEntropyValues(['architecture']);
                        if (d.architecture === 'arm') return 'arm';
                        if (d.architecture === 'x86') return 'x64';
                    }
                } catch (e) {}
                return null;
            }

            // Su macOS lo User-Agent dice sempre "Intel", anche su Apple Silicon.
            // Il renderer WebGL invece distingue: "Apple GPU" vs Intel/AMD/NVIDIA.
            function macArchFromWebGL() {
                try {
                    const gl = document.createElement('canvas').getContext('webgl');
                    if (!gl) return null;
                    const ext = gl.getExtension('WEBGL_debug_renderer_info');
                    const r = String(ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)
                                         : gl.getParameter(gl.RENDERER));
                    if (/intel|amd|radeon|nvidia|geforce/i.test(r)) return 'x64';
                    if (/apple/i.test(r)) return 'arm';
                } catch (e) {}
                return null;
            }

            function show(osLabel, file, autostart) {
                analysingBox.classList.add('hidden');
                mainDownload.classList.remove('hidden');
                detectedOSText.innerText = "Scarica per " + osLabel;
                btn.href = file.startsWith('http') ? file : BASE + file;
                if (autostart) {
                    setTimeout(() => { window.location.href = btn.href; }, 1500);
                }
            }

            // Architettura non determinabile: due pulsanti e nessun download
            // automatico, meglio che servire il pacchetto sbagliato.
            function showChoice(osLabel, labelA, fileA, labelB, fileB, msg) {
                analysingBox.classList.add('hidden');
                mainDownload.classList.remove('hidden');
                detectedOSText.innerText = osLabel;
                btn.href = BASE + fileA;
                btn.innerText = labelA;
                btnAlt.href = BASE + fileB;
                btnAlt.innerText = labelB;
                btnAlt.classList.remove('hidden');
                btnAlt.classList.add('inline-block');
                note.innerText = msg;
                note.classList.remove('hidden');
            }

            (async () => {
                const ua = navigator.userAgent;
                const arch = await archFromClientHints();

                if (ua.includes('Android')) {
                    show("Android", PLAY, false);
                } else if (ua.includes('iPhone') || ua.includes('iPad')) {
                    show("iOS", APPSTORE, false);
                } else if (ua.includes('Win')) {
                    if (arch === 'arm') show("Windows ARM64", F.win_arm, true);
                    else show("Windows (x64)", F.win_x64, true);
                } else if (ua.includes('Mac')) {
                    const macArch = arch || macArchFromWebGL();
                    if (macArch === 'arm') show("macOS (Apple Silicon)", F.mac_arm, true);
                    else if (macArch === 'x64') show("macOS (Intel)", F.mac_x64, true);
                    else showChoice("macOS: scegli la versione",
                        "Apple Silicon", F.mac_arm, "Intel", F.mac_x64,
                        "Non riusciamo a determinare il processore. Se il Mac e' del 2020 o successivo, quasi certamente Apple Silicon (menu Apple > Informazioni su questo Mac).");
                } else if (ua.includes('Linux')) {
                    if (arch === 'arm') show("Linux ARM64 (.deb)", F.linux_arm, true);
                    else if (arch === 'x64') show("Linux x86_64 (.deb)", F.linux_x64, true);
                    else showChoice("Linux: scegli l'architettura",
                        "x86_64 .deb", F.linux_x64, "ARM64 .deb", F.linux_arm,
                        "Verifica con: uname -m. Per altri formati usa l'elenco qui sotto.");
                } else {
                    statusText.innerText = "Non siamo riusciti a identificare il tuo dispositivo. Scegli una versione qui sotto.";
                }
            })();
        });
    </script>
</body>
</html>
