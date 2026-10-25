// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! La memoria condivisa fra il servizio ClantoDesk (scrive) e il Frame Server
//! (legge). Lettura solo per LocalService: il flusso è il volto del tecnico.

use crate::{
    frame::{View, SIZE},
    MAPPING_NAME,
};
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, INVALID_HANDLE_VALUE},
        Security::{
            Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        System::Memory::{
            CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, FILE_MAP_READ,
            MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
        },
    },
};

/// SYSTEM e amministratori: tutto; LocalService (Frame Server): lettura.
const SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;LS)";

pub struct Mapping {
    handle: HANDLE,
    addr: MEMORY_MAPPED_VIEW_ADDRESS,
    pub view: View,
}

unsafe impl Send for Mapping {}

impl Mapping {
    /// Lato servizio: crea la memoria condivisa.
    pub fn create() -> Result<Self, String> {
        unsafe {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(SDDL), SDDL_REVISION_1, &mut sd, None)
                .map_err(|e| format!("security descriptor: {e}"))?;
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            };
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                Some(&sa),
                PAGE_READWRITE,
                0,
                SIZE as u32,
                &HSTRING::from(MAPPING_NAME),
            );
            LocalFree(Some(HLOCAL(sd.0)));
            let handle = handle.map_err(|e| format!("CreateFileMapping: {e}"))?;
            let mapping = Self::map(handle, FILE_MAP_ALL_ACCESS.0)?;
            mapping.view.init();
            Ok(mapping)
        }
    }

    /// Lato Frame Server: apre la memoria, se il servizio l'ha creata.
    pub fn open() -> Option<Self> {
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_READ.0, false, &HSTRING::from(MAPPING_NAME)).ok()?;
            Self::map(handle, FILE_MAP_READ.0).ok()
        }
    }

    unsafe fn map(handle: HANDLE, access: u32) -> Result<Self, String> {
        let addr = MapViewOfFile(handle, windows::Win32::System::Memory::FILE_MAP(access), 0, 0, SIZE);
        if addr.Value.is_null() {
            let e = windows::core::Error::from_win32();
            CloseHandle(handle).ok();
            return Err(format!("MapViewOfFile: {e}"));
        }
        Ok(Self { handle, addr, view: View::new(addr.Value as *mut u8) })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.addr).ok();
            CloseHandle(self.handle).ok();
        }
    }
}
