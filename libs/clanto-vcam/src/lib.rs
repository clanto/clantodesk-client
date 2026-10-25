// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Camera virtuale di ClantoDesk su Windows 11, senza driver.
//!
//! - L'host (servizio ClantoDesk) registra la media source, crea la camera con
//!   `MFCreateVirtualCamera` e scrive i frame NV12 in una memoria condivisa.
//! - La media source (`clanto_vcam.dll`, crate `dll/`) gira nel Frame Server di
//!   Windows e consegna alle applicazioni l'ultimo frame, adattato al formato
//!   scelto dall'applicazione; senza frame, immagine nera.
//!
//! Su macOS la camera è una Camera Extension (flutter/macos/CameraExtension) e
//! `mac` scrive nel suo stream sink.
//!
//! `frame` è codice puro, provato su ogni piattaforma.

pub mod frame;
#[cfg(windows)]
pub mod control;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(windows)]
pub mod shared;

/// CLSID della media source (registrato dall'host in HKLM, InprocServer32).
pub const CLSID: u128 = 0x5c3f8d4e_2a71_4b9e_9d6c_7e1f0a3b8c52;
pub const CLSID_STR: &str = "{5C3F8D4E-2A71-4B9E-9D6C-7E1F0A3B8C52}";
pub const DLL_NAME: &str = "clanto_vcam.dll";
pub const FRIENDLY_NAME: &str = "ClantoDesk Camera";
/// Nome della memoria condivisa: `Global\` perché il Frame Server gira nella
/// sessione 0 come LocalService.
pub const MAPPING_NAME: &str = "Global\\ClantoDeskVirtualCamera";
