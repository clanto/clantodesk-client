// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Lato servizio: registra la media source e crea la camera virtuale per la
//! durata del flusso. Serve un processo con diritti di amministratore (il
//! servizio ClantoDesk installato): scrive in HKLM e crea un oggetto `Global\`.

use crate::{shared::Mapping, CLSID_STR, FRIENDLY_NAME};
use std::path::Path;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::ERROR_SUCCESS,
        Media::MediaFoundation::{
            IMFAsyncCallback, IMFVirtualCamera, MFCreateVirtualCamera, MFIsVirtualCameraTypeSupported, MFStartup,
            MFSTARTUP_NOSOCKET, MFVirtualCameraAccess_AllUsers, MFVirtualCameraLifetime_Session,
            MFVirtualCameraType_SoftwareCameraSource, MF_VERSION,
        },
        System::{
            Com::{CoInitializeEx, COINIT_MULTITHREADED},
            Registry::{
                RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE,
                REG_SZ,
            },
        },
    },
};

pub const NEEDS_WINDOWS_11: &str = "The virtual camera requires Windows 11";

fn init() -> Result<(), String> {
    unsafe {
        // S_FALSE o RPC_E_CHANGED_MODE: COM è già pronto in questo thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        // Una volta per processo: Media Foundation resta attivo fino all'uscita.
        static STARTUP: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        STARTUP
            .get_or_init(|| MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).map_err(|e| format!("MFStartup: {e}")))
            .clone()?;
        match MFIsVirtualCameraTypeSupported(MFVirtualCameraType_SoftwareCameraSource) {
            Ok(supported) if supported.as_bool() => Ok(()),
            _ => Err(NEEDS_WINDOWS_11.to_owned()),
        }
    }
}

fn set_string(key: HKEY, name: Option<&str>, value: &str) -> Result<(), String> {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    let name = name.map(HSTRING::from);
    let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let ret = unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(bytes)) };
    if ret != ERROR_SUCCESS {
        return Err(format!("RegSetValueEx: {}", ret.0));
    }
    Ok(())
}

/// HKLM\SOFTWARE\Classes\CLSID\{...}\InprocServer32 -> la DLL accanto all'eseguibile.
pub fn register(dll: &Path) -> Result<(), String> {
    if !dll.is_file() {
        return Err(format!("{} not found", dll.display()));
    }
    let subkey = HSTRING::from(format!("SOFTWARE\\Classes\\CLSID\\{CLSID_STR}\\InprocServer32"));
    let mut key = HKEY::default();
    let ret = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if ret != ERROR_SUCCESS {
        return Err(format!("Cannot register the virtual camera (run the installed ClantoDesk service): {}", ret.0));
    }
    let result = set_string(key, None, &dll.to_string_lossy()).and_then(|_| set_string(key, Some("ThreadingModel"), "Both"));
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

/// Camera virtuale attiva. Al drop la camera sparisce dalle applicazioni.
pub struct VirtualCamera {
    camera: IMFVirtualCamera,
    mapping: Mapping,
}

// Oggetto MTA: usato solo da thread inizializzati con `init`.
unsafe impl Send for VirtualCamera {}

impl VirtualCamera {
    pub fn start(dll: &Path) -> Result<Self, String> {
        init()?;
        register(dll)?;
        let mapping = Mapping::create()?;
        let camera = unsafe {
            MFCreateVirtualCamera(
                MFVirtualCameraType_SoftwareCameraSource,
                MFVirtualCameraLifetime_Session,
                MFVirtualCameraAccess_AllUsers,
                &HSTRING::from(FRIENDLY_NAME),
                &HSTRING::from(CLSID_STR),
                None,
            )
        }
        .map_err(|e| format!("MFCreateVirtualCamera: {e}"))?;
        unsafe { camera.Start(None::<&IMFAsyncCallback>) }.map_err(|e| format!("Virtual camera start: {e}"))?;
        Ok(Self { camera, mapping })
    }

    /// Frame NV12; `false` se le dimensioni non sono accettabili.
    pub fn write(&self, width: usize, height: usize, nv12: &[u8]) -> bool {
        self.mapping.view.write(width, height, nv12)
    }
}

impl Drop for VirtualCamera {
    fn drop(&mut self) {
        self.mapping.view.set_active(false);
        let _ = init();
        unsafe {
            let _ = self.camera.Remove();
            let _ = self.camera.Shutdown();
        }
    }
}
