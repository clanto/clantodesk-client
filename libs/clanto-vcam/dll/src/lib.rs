// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Media source della camera virtuale ClantoDesk, caricata dal Frame Server di
//! Windows 11 tramite il CLSID registrato dal servizio ClantoDesk. Consegna
//! l'ultimo frame scritto dal servizio nella memoria condivisa; senza frame,
//! immagine nera. Nessun driver: è una DLL COM in modalità utente.

#![cfg(windows)]
#![allow(non_snake_case)]

mod source;

use std::{ffi::c_void, ptr::null_mut, sync::Mutex};
use windows::{
    core::{implement, Interface, Ref, Result, BOOL, GUID, HRESULT},
    Win32::{
        Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, S_FALSE},
        Media::MediaFoundation::*,
        System::Com::{IClassFactory, IClassFactory_Impl, StructuredStorage::PROPVARIANT},
    },
};

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = null_mut();
    if rclsid.is_null() || *rclsid != GUID::from_u128(clanto_vcam::CLSID) {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    factory.query(riid, ppv)
}

/// Il Frame Server tiene caricata la DLL finché serve: niente scaricamento.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<'_, windows::core::IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        if ppv.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = null_mut() };
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let activate: IMFActivate = Activate::new()?.into();
        unsafe { activate.query(riid, ppv).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// L'oggetto registrato: un `IMFActivate` che crea la media source.
#[implement(IMFActivate)]
struct Activate {
    attrs: IMFAttributes,
    source: Mutex<Option<IMFMediaSource>>,
}

impl Activate {
    fn new() -> Result<Self> {
        let mut attrs = None;
        unsafe { MFCreateAttributes(&mut attrs, 1)? };
        Ok(Self { attrs: attrs.ok_or(E_POINTER)?, source: Mutex::new(None) })
    }

    fn raw(&self) -> (*mut c_void, &IMFAttributes_Vtbl) {
        (self.attrs.as_raw(), Interface::vtable(&self.attrs))
    }
}

impl IMFActivate_Impl for Activate_Impl {
    fn ActivateObject(&self, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        let mut source = self.source.lock().unwrap();
        if source.is_none() {
            *source = Some(source::create()?);
        }
        unsafe { source.as_ref().unwrap().query(riid, ppv).ok() }
    }

    fn ShutdownObject(&self) -> Result<()> {
        if let Some(source) = self.source.lock().unwrap().take() {
            unsafe {
                let _ = source.Shutdown();
            }
        }
        Ok(())
    }

    fn DetachObject(&self) -> Result<()> {
        self.source.lock().unwrap().take();
        Ok(())
    }
}

fn ptr<T: Interface>(r: Ref<'_, T>) -> *mut c_void {
    r.as_ref().map_or(null_mut(), |i| i.as_raw())
}

// Gli attributi dell'Activate sono quelli di un IMFAttributes standard.
impl IMFAttributes_Impl for Activate_Impl {
    fn GetItem(&self, k: *const GUID, v: *mut PROPVARIANT) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetItem)(p, k, v).ok() }
    }
    fn GetItemType(&self, k: *const GUID) -> Result<MF_ATTRIBUTE_TYPE> {
        let (p, vt) = self.raw();
        let mut out = MF_ATTRIBUTE_TYPE::default();
        unsafe { (vt.GetItemType)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn CompareItem(&self, k: *const GUID, v: *const PROPVARIANT) -> Result<BOOL> {
        let (p, vt) = self.raw();
        let mut out = BOOL::default();
        unsafe { (vt.CompareItem)(p, k, v, &mut out).ok()? };
        Ok(out)
    }
    fn Compare(&self, theirs: Ref<'_, IMFAttributes>, t: MF_ATTRIBUTES_MATCH_TYPE) -> Result<BOOL> {
        let (p, vt) = self.raw();
        let mut out = BOOL::default();
        unsafe { (vt.Compare)(p, ptr(theirs), t, &mut out).ok()? };
        Ok(out)
    }
    fn GetUINT32(&self, k: *const GUID) -> Result<u32> {
        let (p, vt) = self.raw();
        let mut out = 0;
        unsafe { (vt.GetUINT32)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn GetUINT64(&self, k: *const GUID) -> Result<u64> {
        let (p, vt) = self.raw();
        let mut out = 0;
        unsafe { (vt.GetUINT64)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn GetDouble(&self, k: *const GUID) -> Result<f64> {
        let (p, vt) = self.raw();
        let mut out = 0.0;
        unsafe { (vt.GetDouble)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn GetGUID(&self, k: *const GUID) -> Result<GUID> {
        let (p, vt) = self.raw();
        let mut out = GUID::zeroed();
        unsafe { (vt.GetGUID)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn GetStringLength(&self, k: *const GUID) -> Result<u32> {
        let (p, vt) = self.raw();
        let mut out = 0;
        unsafe { (vt.GetStringLength)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn GetString(&self, k: *const GUID, v: windows::core::PWSTR, size: u32, len: *mut u32) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetString)(p, k, v, size, len).ok() }
    }
    fn GetAllocatedString(&self, k: *const GUID, v: *mut windows::core::PWSTR, len: *mut u32) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetAllocatedString)(p, k, v, len).ok() }
    }
    fn GetBlobSize(&self, k: *const GUID) -> Result<u32> {
        let (p, vt) = self.raw();
        let mut out = 0;
        unsafe { (vt.GetBlobSize)(p, k, &mut out).ok()? };
        Ok(out)
    }
    fn GetBlob(&self, k: *const GUID, buf: *mut u8, size: u32, len: *mut u32) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetBlob)(p, k, buf, size, len).ok() }
    }
    fn GetAllocatedBlob(&self, k: *const GUID, buf: *mut *mut u8, size: *mut u32) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetAllocatedBlob)(p, k, buf, size).ok() }
    }
    fn GetUnknown(&self, k: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetUnknown)(p, k, riid, ppv).ok() }
    }
    fn SetItem(&self, k: *const GUID, v: *const PROPVARIANT) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetItem)(p, k, v).ok() }
    }
    fn DeleteItem(&self, k: *const GUID) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.DeleteItem)(p, k).ok() }
    }
    fn DeleteAllItems(&self) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.DeleteAllItems)(p).ok() }
    }
    fn SetUINT32(&self, k: *const GUID, v: u32) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetUINT32)(p, k, v).ok() }
    }
    fn SetUINT64(&self, k: *const GUID, v: u64) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetUINT64)(p, k, v).ok() }
    }
    fn SetDouble(&self, k: *const GUID, v: f64) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetDouble)(p, k, v).ok() }
    }
    fn SetGUID(&self, k: *const GUID, v: *const GUID) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetGUID)(p, k, v).ok() }
    }
    fn SetString(&self, k: *const GUID, v: &windows::core::PCWSTR) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetString)(p, k, *v).ok() }
    }
    fn SetBlob(&self, k: *const GUID, buf: *const u8, size: u32) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetBlob)(p, k, buf, size).ok() }
    }
    fn SetUnknown(&self, k: *const GUID, v: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.SetUnknown)(p, k, ptr(v)).ok() }
    }
    fn LockStore(&self) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.LockStore)(p).ok() }
    }
    fn UnlockStore(&self) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.UnlockStore)(p).ok() }
    }
    fn GetCount(&self) -> Result<u32> {
        let (p, vt) = self.raw();
        let mut out = 0;
        unsafe { (vt.GetCount)(p, &mut out).ok()? };
        Ok(out)
    }
    fn GetItemByIndex(&self, i: u32, k: *mut GUID, v: *mut PROPVARIANT) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.GetItemByIndex)(p, i, k, v).ok() }
    }
    fn CopyAllItems(&self, dest: Ref<'_, IMFAttributes>) -> Result<()> {
        let (p, vt) = self.raw();
        unsafe { (vt.CopyAllItems)(p, ptr(dest)).ok() }
    }
}
