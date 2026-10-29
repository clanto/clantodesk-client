// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Postura del dispositivo Windows con API native, al posto dello script
//! PowerShell: stesse chiavi (src/clanto/posture.rs), una frazione del tempo.
//! Registro per quasi tutto; API di Windows Security Center per antivirus e
//! firewall; proprietà della shell per BitLocker (senza privilegi); WMI solo
//! per VBS; TBS per il TPM; IP Helper e WLAN per la rete.
//! Una voce che non si riesce a leggere manca (resta `unknown`).

use std::ffi::c_void;
use windows::{
    core::{BSTR, HSTRING, PCWSTR},
    Win32::{
        Foundation::{ERROR_SUCCESS, HANDLE},
        NetworkManagement::{
            IpHelper::{GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, IP_ADAPTER_ADDRESSES_LH},
            WiFi::{
                wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
                WlanOpenHandle, WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
                DOT11_AUTH_ALGORITHM,
            },
        },
        Networking::WinSock::AF_UNSPEC,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
                EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
            },
            Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ},
            SecurityCenter::{
                IWSCProductList, WSCProductList, WSC_SECURITY_PRODUCT_STATE_ON, WSC_SECURITY_PRODUCT_UP_TO_DATE,
                WSC_SECURITY_PROVIDER, WSC_SECURITY_PROVIDER_ANTIVIRUS, WSC_SECURITY_PROVIDER_FIREWALL,
            },
            TpmBaseServices::{Tbsi_GetDeviceInfo, TPM_DEVICE_INFO},
            Variant::VARIANT,
            Wmi::{
                IEnumWbemClassObject, IWbemClassObject, IWbemLocator, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
                WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
            },
        },
        UI::Shell::IsUserAnAdmin,
    },
};

const RPC_C_AUTHN_WINNT: u32 = 10;
const RPC_C_AUTHZ_NONE: u32 = 0;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn reg_dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
    let (key, name) = (wide(key), wide(name));
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

fn reg_string(root: HKEY, key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(key), wide(name));
    let mut buffer = vec![0u16; 512];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]).trim().to_owned())
}

fn reg_binary(root: HKEY, key: &str, name: &str) -> Option<Vec<u8>> {
    let (key, name) = (wide(key), wide(name));
    let mut buffer = vec![0u8; 64];
    let mut size = buffer.len() as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(buffer.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then(|| buffer[..size as usize].to_vec())
}

fn reg_key_exists(root: HKEY, key: &str) -> bool {
    use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, KEY_READ};
    let key = wide(key);
    let mut handle = HKEY::default();
    let ok = unsafe { RegOpenKeyExW(root, PCWSTR(key.as_ptr()), None, KEY_READ, &mut handle) } == ERROR_SUCCESS;
    if ok {
        unsafe {
            let _ = RegCloseKey(handle);
        }
    }
    ok
}

fn flag(value: bool) -> String {
    if value { "true" } else { "false" }.to_owned()
}

/// Giorni trascorsi da un FILETIME (100 ns dal 1601).
fn days_since_filetime(filetime: u64) -> Option<u64> {
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    let unix_secs = filetime.checked_sub(EPOCH_DIFF_100NS)? / 10_000_000;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(now.saturating_sub(unix_secs) / 86_400)
}

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

fn os(out: &mut Vec<(String, String)>) {
    let build = reg_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "CurrentBuildNumber");
    let major = reg_dword(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "CurrentMajorVersionNumber");
    let minor = reg_dword(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "CurrentMinorVersionNumber");
    if let (Some(build), Some(major), Some(minor)) = (&build, major, minor) {
        out.push(("os_version".into(), format!("{major}.{minor}.{build}")));
    }
    if let Some(build) = &build {
        out.push(("os_build".into(), build.clone()));
    }
    if let Some(mut name) = reg_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "ProductName") {
        // ProductName resta «Windows 10» anche su Windows 11.
        if build.as_deref().and_then(|b| b.parse::<u32>().ok()).map_or(false, |b| b >= 22000) {
            name = name.replace("Windows 10", "Windows 11");
        }
        out.push(("os_edition".into(), format!("Microsoft {name}")));
    }
    if let Some(ubr) = reg_dword(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "UBR") {
        out.push(("os_ubr".into(), ubr.to_string()));
    }
}

fn registry_flags(out: &mut Vec<(String, String)>) {
    if let Some(v) = reg_dword(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\SecureBoot\State", "UEFISecureBootEnabled") {
        out.push(("secure_boot".into(), flag(v == 1)));
    }
    match reg_dword(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control", "PEFirmwareType") {
        Some(2) => out.push(("firmware_type".into(), "uefi".into())),
        Some(1) => out.push(("firmware_type".into(), "bios".into())),
        _ => {}
    }
    if let Some(v) = reg_dword(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update",
        "AUOptions",
    ) {
        out.push(("auto_update_enabled".into(), flag(v >= 3)));
    }
    let reboot = reg_key_exists(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
    ) || reg_key_exists(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired",
    );
    out.push(("pending_reboot".into(), flag(reboot)));
    if let Some(v) = reg_dword(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System", "EnableLUA") {
        out.push(("uac_enabled".into(), flag(v == 1)));
    }
    out.push(("user_is_admin".into(), flag(unsafe { IsUserAnAdmin() }.as_bool())));
    if let Some(v) = reg_dword(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        "ProxyEnable",
    ) {
        out.push(("proxy_set".into(), flag(v == 1)));
    }
    if let (Some(manufacturer), Some(product)) = (
        reg_string(HKEY_LOCAL_MACHINE, r"HARDWARE\DESCRIPTION\System\BIOS", "SystemManufacturer"),
        reg_string(HKEY_LOCAL_MACHINE, r"HARDWARE\DESCRIPTION\System\BIOS", "SystemProductName"),
    ) {
        let text = format!("{product} {manufacturer}");
        let vm = ["Virtual", "VMware", "VirtualBox", "KVM", "QEMU", "Xen"].iter().any(|m| text.contains(m));
        out.push(("is_vm".into(), flag(vm)));
    }
    if let Some(bytes) = reg_binary(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows Defender\Signature Updates",
        "SignaturesLastUpdated",
    ) {
        if bytes.len() >= 8 {
            let filetime = u64::from_le_bytes(bytes[..8].try_into().unwrap_or_default());
            if let Some(days) = days_since_filetime(filetime) {
                out.push(("av_signature_days".into(), days.to_string()));
            }
        }
    }
}

/// Firewall del profilo pubblico: la policy di dominio prevale sulla
/// configurazione locale.
fn public_firewall() -> Option<bool> {
    reg_dword(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Policies\Microsoft\WindowsFirewall\PublicProfile",
        "EnableFirewall",
    )
    .or_else(|| {
        reg_dword(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\PublicProfile",
            "EnableFirewall",
        )
    })
    .map(|v| v == 1)
}

/// Aggiornamenti dai pacchetti CBS installati (stato 112): l'ultimo cumulativo
/// (`RollupFix`) e l'ultimo aggiornamento di qualsiasi tipo.
fn updates(out: &mut Vec<(String, String)>) {
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, KEY_READ};
    const PACKAGES: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\Packages";
    let key = wide(PACKAGES);
    let mut handle = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr()), None, KEY_READ, &mut handle) } != ERROR_SUCCESS {
        return;
    }
    let (mut latest_rollup, mut latest_any) = (0u64, 0u64);
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(handle, index, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None)
        };
        if status != ERROR_SUCCESS {
            break;
        }
        index += 1;
        let name = String::from_utf16_lossy(&name[..len as usize]);
        let rollup = name.starts_with("Package_for_RollupFix");
        if !rollup && !name.starts_with("Package_for_KB") && !name.starts_with("Package_for_ServicingStack") {
            continue;
        }
        let sub = format!(r"{PACKAGES}\{name}");
        if reg_dword(HKEY_LOCAL_MACHINE, &sub, "CurrentState") != Some(112) {
            continue;
        }
        let (Some(high), Some(low)) = (
            reg_dword(HKEY_LOCAL_MACHINE, &sub, "InstallTimeHigh"),
            reg_dword(HKEY_LOCAL_MACHINE, &sub, "InstallTimeLow"),
        ) else {
            continue;
        };
        let time = ((high as u64) << 32) | low as u64;
        latest_any = latest_any.max(time);
        if rollup {
            latest_rollup = latest_rollup.max(time);
        }
    }
    unsafe {
        let _ = RegCloseKey(handle);
    }
    if let Some(days) = (latest_any > 0).then(|| days_since_filetime(latest_any)).flatten() {
        out.push(("last_update_days".into(), days.to_string()));
    }
    if let Some(days) = (latest_rollup > 0).then(|| days_since_filetime(latest_rollup)).flatten() {
        out.push(("os_patch_days".into(), days.to_string()));
    }
}

fn tpm(out: &mut Vec<(String, String)>) {
    let mut info = TPM_DEVICE_INFO::default();
    let status = unsafe {
        Tbsi_GetDeviceInfo(std::mem::size_of::<TPM_DEVICE_INFO>() as u32, &mut info as *mut _ as *mut c_void)
    };
    const TBS_SUCCESS: u32 = 0;
    const TBS_E_TPM_NOT_FOUND: u32 = 0x8028_400F;
    match status {
        TBS_SUCCESS => out.push((
            "tpm_version".into(),
            match info.tpmVersion {
                2 => "2.0".into(),
                1 => "1.2".into(),
                v => v.to_string(),
            },
        )),
        TBS_E_TPM_NOT_FOUND => out.push(("tpm_version".into(), "none".into())),
        _ => {}
    }
}

/// Valore WMI come testo o numero.
fn variant_i64(value: &VARIANT) -> Option<i64> {
    i32::try_from(value).ok().map(i64::from).or_else(|| u32::try_from(value).ok().map(i64::from))
}

/// Righe di una query WMI: per ogni oggetto, le proprietà richieste.
fn wmi_query(namespace: &str, query: &str, props: &[&str]) -> Option<Vec<Vec<VARIANT>>> {
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        let services = locator
            .ConnectServer(&BSTR::from(namespace), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
            .ok()?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
        .ok()?;
        let rows: IEnumWbemClassObject = services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .ok()?;
        let mut result = Vec::new();
        loop {
            let mut objects = [None::<IWbemClassObject>; 1];
            let mut returned = 0u32;
            if rows.Next(WBEM_INFINITE, &mut objects, &mut returned).is_err() || returned == 0 {
                break;
            }
            let Some(object) = objects[0].take() else {
                break;
            };
            let mut row = Vec::new();
            for prop in props {
                let mut value = VARIANT::default();
                let name = HSTRING::from(*prop);
                let _ = object.Get(&name, 0, &mut value, None, None);
                row.push(value);
            }
            result.push(row);
        }
        Some(result)
    }
}

struct WscProduct {
    name: String,
    on: bool,
    up_to_date: bool,
}

/// Prodotti registrati in Windows Security Center (`IWSCProductList`, Windows
/// 8+, senza privilegi). `None` senza Security Center (Windows Server).
fn wsc_products(provider: WSC_SECURITY_PROVIDER) -> Option<Vec<WscProduct>> {
    unsafe {
        let list: IWSCProductList = CoCreateInstance(&WSCProductList, None, CLSCTX_INPROC_SERVER).ok()?;
        list.Initialize(provider).ok()?;
        let count = list.Count().ok()?.max(0) as u32;
        let mut products = Vec::new();
        for index in 0..count {
            let Ok(product) = list.get_Item(index) else {
                continue;
            };
            products.push(WscProduct {
                name: product.ProductName().map(|n| n.to_string()).unwrap_or_default(),
                on: product.ProductState().ok() == Some(WSC_SECURITY_PRODUCT_STATE_ON),
                up_to_date: product.SignatureStatus().ok() == Some(WSC_SECURITY_PRODUCT_UP_TO_DATE),
            });
        }
        Some(products)
    }
}

fn security_center(out: &mut Vec<(String, String)>) {
    if let Some(products) = wsc_products(WSC_SECURITY_PROVIDER_ANTIVIRUS).filter(|p| !p.is_empty()) {
        out.push(("av_enabled".into(), flag(products.iter().any(|p| p.on))));
        out.push(("av_uptodate".into(), flag(products.iter().any(|p| p.on && p.up_to_date))));
        let names: Vec<&str> = products.iter().map(|p| p.name.as_str()).filter(|n| !n.is_empty()).collect();
        out.push(("av_products".into(), names.join("|")));
    }
    let firewall_products = wsc_products(WSC_SECURITY_PROVIDER_FIREWALL)
        .filter(|products| !products.is_empty())
        .map(|products| products.iter().any(|p| p.on));
    let public = public_firewall();
    if let Some(public) = public {
        out.push(("firewall_public".into(), flag(public)));
    }
    if firewall_products.is_some() || public.is_some() {
        out.push((
            "firewall_enabled".into(),
            flag(firewall_products.unwrap_or(false) || public.unwrap_or(false)),
        ));
    }
}

/// Protezione BitLocker del disco di sistema dalla proprietà della shell
/// `System.Volume.BitLockerProtection`: leggibile senza privilegi, mentre
/// `Win32_EncryptableVolume` (WMI) li richiede e senza impiega secondi a
/// rifiutare. 1 = attiva, 2 = disattivata; gli altri stati restano `unknown`.
fn bitlocker(out: &mut Vec<(String, String)>) {
    use windows::Win32::{
        Foundation::PROPERTYKEY,
        UI::Shell::PropertiesSystem::{IPropertyStore, PSGetPropertyKeyFromName, SHGetPropertyStoreFromParsingName, GPS_DEFAULT},
    };
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let status = unsafe {
        let mut key = PROPERTYKEY::default();
        if PSGetPropertyKeyFromName(&HSTRING::from("System.Volume.BitLockerProtection"), &mut key).is_err() {
            return;
        }
        let Ok(store) =
            SHGetPropertyStoreFromParsingName::<_, _, IPropertyStore>(&HSTRING::from(format!(r"{drive}\")), None, GPS_DEFAULT)
        else {
            return;
        };
        let Ok(value) = store.GetValue(&key) else {
            return;
        };
        i32::try_from(&value).ok()
    };
    match status {
        Some(1) => out.push(("disk_encrypted".into(), flag(true))),
        Some(2) => out.push(("disk_encrypted".into(), flag(false))),
        _ => {}
    }
}

fn vbs(out: &mut Vec<(String, String)>) {
    if let Some(rows) = wmi_query(
        r"ROOT\Microsoft\Windows\DeviceGuard",
        "SELECT VirtualizationBasedSecurityStatus FROM Win32_DeviceGuard",
        &["VirtualizationBasedSecurityStatus"],
    ) {
        if let Some(status) = rows.first().and_then(|row| variant_i64(&row[0])) {
            out.push(("vbs_enabled".into(), flag(status == 2)));
        }
    }
}

fn network(out: &mut Vec<(String, String)>) {
    const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
    const IF_TYPE_PPP: u32 = 23;
    const IF_TYPE_IEEE80211: u32 = 71;
    const IF_OPER_STATUS_UP: i32 = 1;
    let mut size = 16 * 1024u32;
    let mut buffer = vec![0u8; size as usize];
    let mut status = 0u32;
    for _ in 0..3 {
        status = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_GATEWAYS,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        if status == 111 {
            // ERROR_BUFFER_OVERFLOW: `size` è la dimensione necessaria.
            buffer = vec![0u8; size as usize];
            continue;
        }
        break;
    }
    if status != ERROR_SUCCESS.0 {
        return;
    }
    let (mut net_type, mut vpn) = (None, false);
    let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        let a = unsafe { &*adapter };
        adapter = a.Next;
        if a.OperStatus.0 != IF_OPER_STATUS_UP {
            continue;
        }
        let description = unsafe { a.Description.to_string() }.unwrap_or_default();
        if a.IfType == IF_TYPE_PPP
            || ["VPN", "TAP", "TUN", "WireGuard"].iter().any(|m| description.contains(m))
        {
            vpn = true;
            continue;
        }
        // Rete attiva: l'adattatore con un gateway predefinito.
        if net_type.is_none() && !a.FirstGatewayAddress.is_null() {
            net_type = Some(match a.IfType {
                IF_TYPE_IEEE80211 => "wifi",
                IF_TYPE_ETHERNET_CSMACD => "ethernet",
                _ => "other",
            });
        }
    }
    if let Some(net_type) = net_type {
        out.push(("net_type".into(), net_type.into()));
    }
    out.push(("vpn_active".into(), flag(vpn)));
}

fn wifi(out: &mut Vec<(String, String)>) {
    unsafe {
        let mut version = 0u32;
        let mut client = HANDLE::default();
        if WlanOpenHandle(2, None, &mut version, &mut client) != ERROR_SUCCESS.0 {
            return;
        }
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(client, None, &mut list) == ERROR_SUCCESS.0 && !list.is_null() {
            let count = (*list).dwNumberOfItems as usize;
            let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), count);
            for item in items {
                let mut size = 0u32;
                let mut data: *mut c_void = std::ptr::null_mut();
                if WlanQueryInterface(
                    client,
                    &item.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                ) != ERROR_SUCCESS.0
                    || data.is_null()
                {
                    continue;
                }
                let attributes = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                let security = auth_name(attributes.wlanSecurityAttributes.dot11AuthAlgorithm);
                WlanFreeMemory(data);
                if let Some(security) = security {
                    out.push(("wifi_security".into(), security.into()));
                    break;
                }
            }
            WlanFreeMemory(list as *const c_void);
        }
        WlanCloseHandle(client, None);
    }
}

fn auth_name(algorithm: DOT11_AUTH_ALGORITHM) -> Option<&'static str> {
    Some(match algorithm.0 {
        1 => "open",
        2 => "wep",
        3 | 4 => "wpa",
        6 | 7 => "wpa2",
        8 | 9 | 10 | 11 => "wpa3",
        _ => return None,
    })
}

/// Raccolta completa: coppie chiave/valore come le righe dello script
/// PowerShell. Va chiamata da un thread che può inizializzare COM (MTA).
/// Raccolta a passi: `emit` riceve le voci di ogni passo appena pronte, così
/// un passo lento non fa perdere quelle già lette a chi ha una scadenza.
pub fn collect_each(mut emit: impl FnMut(Vec<(String, String)>)) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let steps: [fn(&mut Vec<(String, String)>); 9] =
        [os, registry_flags, updates, tpm, security_center, bitlocker, vbs, network, wifi];
    for step in steps {
        let mut out = Vec::new();
        step(&mut out);
        if !out.is_empty() {
            emit(out);
        }
    }
}

pub fn collect() -> Vec<(String, String)> {
    let mut all = Vec::new();
    collect_each(|out| all.extend(out));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_and_auth_mapping() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let three_days_ago = (now - 3 * 86_400) * 10_000_000 + 116_444_736_000_000_000;
        assert_eq!(days_since_filetime(three_days_ago), Some(3));
        assert_eq!(auth_name(DOT11_AUTH_ALGORITHM(7)), Some("wpa2"));
        assert_eq!(auth_name(DOT11_AUTH_ALGORITHM(9)), Some("wpa3"));
        assert_eq!(auth_name(DOT11_AUTH_ALGORITHM(0)), None);
    }
}
