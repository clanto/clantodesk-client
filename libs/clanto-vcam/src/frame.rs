// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Buffer di un frame NV12 condiviso fra due processi, con un seqlock: chi
//! scrive rende dispari la sequenza durante la copia, chi legge riprova se la
//! sequenza cambia o è dispari. Più l'adattamento al formato dell'applicazione
//! (scala e bande nere), fatto da chi legge.

use std::sync::atomic::{fence, AtomicU64, Ordering};

pub const MAX_WIDTH: usize = 1920;
pub const MAX_HEIGHT: usize = 1080;
const MAGIC: u32 = 0x4D41_4356; // "VCAM"
const VERSION: u32 = 1;
const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_WIDTH: usize = 8;
const OFF_HEIGHT: usize = 12;
const OFF_SEQ: usize = 16;
const OFF_ACTIVE: usize = 24;
pub const HEADER: usize = 64;
pub const CAPACITY: usize = MAX_WIDTH * MAX_HEIGHT * 3 / 2;
pub const SIZE: usize = HEADER + CAPACITY;

pub fn nv12_len(width: usize, height: usize) -> usize {
    width * height * 3 / 2
}

/// Vista su una regione di `SIZE` byte allineata a 8 (una mappatura lo è).
pub struct View {
    ptr: *mut u8,
}

// La regione vive quanto la mappatura che la possiede; l'accesso concorrente
// passa dal seqlock.
unsafe impl Send for View {}

impl View {
    /// # Safety
    /// `ptr` punta a `SIZE` byte validi, allineati a 8, per tutta la vita della vista.
    pub unsafe fn new(ptr: *mut u8) -> Self {
        Self { ptr }
    }

    fn u32_at(&self, off: usize) -> u32 {
        unsafe { std::ptr::read_volatile(self.ptr.add(off) as *const u32) }
    }

    fn set_u32(&self, off: usize, v: u32) {
        unsafe { std::ptr::write_volatile(self.ptr.add(off) as *mut u32, v) }
    }

    fn seq(&self) -> &AtomicU64 {
        unsafe { &*(self.ptr.add(OFF_SEQ) as *const AtomicU64) }
    }

    pub fn init(&self) {
        self.set_u32(OFF_MAGIC, MAGIC);
        self.set_u32(OFF_VERSION, VERSION);
        self.set_u32(OFF_ACTIVE, 0);
    }

    pub fn valid(&self) -> bool {
        self.u32_at(OFF_MAGIC) == MAGIC && self.u32_at(OFF_VERSION) == VERSION
    }

    pub fn set_active(&self, active: bool) {
        self.set_u32(OFF_ACTIVE, active as u32);
    }

    pub fn active(&self) -> bool {
        self.u32_at(OFF_ACTIVE) != 0
    }

    /// Un solo scrittore. Frame troppo grandi o malformati vengono rifiutati.
    pub fn write(&self, width: usize, height: usize, nv12: &[u8]) -> bool {
        if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT || width % 2 != 0 || height % 2 != 0 {
            return false;
        }
        if nv12.len() != nv12_len(width, height) {
            return false;
        }
        let seq = self.seq().load(Ordering::Relaxed);
        self.seq().store(seq | 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.set_u32(OFF_WIDTH, width as u32);
        self.set_u32(OFF_HEIGHT, height as u32);
        unsafe { std::ptr::copy_nonoverlapping(nv12.as_ptr(), self.ptr.add(HEADER), nv12.len()) };
        fence(Ordering::Release);
        self.seq().store((seq | 1) + 1, Ordering::Release);
        self.set_active(true);
        true
    }

    /// L'ultimo frame completo in `out`: `(larghezza, altezza, sequenza)`.
    pub fn read(&self, out: &mut Vec<u8>) -> Option<(usize, usize, u64)> {
        if !self.valid() || !self.active() {
            return None;
        }
        for _ in 0..4 {
            let before = self.seq().load(Ordering::Acquire);
            if before == 0 || before % 2 == 1 {
                std::thread::yield_now();
                continue;
            }
            let (w, h) = (self.u32_at(OFF_WIDTH) as usize, self.u32_at(OFF_HEIGHT) as usize);
            if w == 0 || h == 0 || w > MAX_WIDTH || h > MAX_HEIGHT {
                return None;
            }
            let len = nv12_len(w, h);
            out.resize(len, 0);
            unsafe { std::ptr::copy_nonoverlapping(self.ptr.add(HEADER), out.as_mut_ptr(), len) };
            fence(Ordering::Acquire);
            if self.seq().load(Ordering::Relaxed) == before {
                return Some((w, h, before));
            }
        }
        None
    }
}

/// Nero in NV12 (Y=16, UV=128).
pub fn black(dst: &mut [u8], width: usize, height: usize) {
    let y = width * height;
    dst[..y].fill(16);
    dst[y..nv12_len(width, height)].fill(128);
}

/// Adatta `src` a `dst` mantenendo le proporzioni, con bande nere. Scala al
/// pixel più vicino: costa poco ed è sufficiente per una webcam.
pub fn fit(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize) {
    black(dst, dw, dh);
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return;
    }
    // Area di destinazione con le proporzioni della sorgente, a coordinate pari.
    let (mut tw, mut th) = if sw * dh > sh * dw { (dw, sh * dw / sw) } else { (sw * dh / sh, dh) };
    tw = (tw & !1).max(2).min(dw);
    th = (th & !1).max(2).min(dh);
    let ox = ((dw - tw) / 2) & !1;
    let oy = ((dh - th) / 2) & !1;
    let (src_y, src_uv) = src.split_at(sw * sh);
    let (dst_y, dst_uv) = dst.split_at_mut(dw * dh);
    for y in 0..th {
        let sy = y * sh / th;
        let srow = &src_y[sy * sw..(sy + 1) * sw];
        let drow = &mut dst_y[(oy + y) * dw + ox..(oy + y) * dw + ox + tw];
        for (x, d) in drow.iter_mut().enumerate() {
            *d = srow[x * sw / tw];
        }
    }
    // Piano UV interlacciato a metà risoluzione: coppie (U,V).
    let (tcw, tch, scw, sch) = (tw / 2, th / 2, sw / 2, sh / 2);
    for y in 0..tch {
        let sy = y * sch / tch;
        for x in 0..tcw {
            let sx = x * scw / tcw;
            let s = sy * sw + sx * 2;
            let d = (oy / 2 + y) * dw + ox + x * 2;
            dst_uv[d] = src_uv[s];
            dst_uv[d + 1] = src_uv[s + 1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region() -> Vec<u64> {
        vec![0u64; SIZE / 8 + 1]
    }

    #[test]
    fn seqlock_roundtrip() {
        let mut mem = region();
        let view = unsafe { View::new(mem.as_mut_ptr() as *mut u8) };
        let mut out = Vec::new();
        assert!(view.read(&mut out).is_none(), "not initialized");
        view.init();
        assert!(view.read(&mut out).is_none(), "no frame yet");
        let frame: Vec<u8> = (0..nv12_len(64, 48)).map(|i| (i % 251) as u8).collect();
        assert!(view.write(64, 48, &frame));
        let (w, h, seq) = view.read(&mut out).unwrap();
        assert_eq!((w, h), (64, 48));
        assert_eq!(out, frame);
        assert!(seq > 0 && seq % 2 == 0);
        assert!(view.write(64, 48, &frame));
        assert!(view.read(&mut out).unwrap().2 > seq);
        view.set_active(false);
        assert!(view.read(&mut out).is_none());
        assert!(!view.write(65, 48, &frame), "odd width");
        assert!(!view.write(64, 48, &frame[1..]), "short frame");
        assert!(!view.write(4096, 2160, &[]), "too large");
    }

    #[test]
    fn fit_letterboxes_and_keeps_content() {
        // 4:3 bianco in 16:9: bande nere ai lati.
        let (sw, sh, dw, dh) = (64, 48, 128, 72);
        let mut src = vec![235u8; nv12_len(sw, sh)];
        src[sw * sh..].fill(128);
        let mut dst = vec![0u8; nv12_len(dw, dh)];
        fit(&src, sw, sh, &mut dst, dw, dh);
        assert_eq!(dst[0], 16, "left band is black");
        assert_eq!(dst[dw / 2 + (dh / 2) * dw], 235, "centre is the image");
        assert_eq!(dst[dw - 1], 16, "right band is black");
        assert!(dst[dw * dh..].iter().all(|&c| c == 128));
        // Stesse dimensioni: copia identica.
        let src: Vec<u8> = (0..nv12_len(32, 16)).map(|i| (i % 200) as u8).collect();
        let mut dst = vec![0u8; src.len()];
        fit(&src, 32, 16, &mut dst, 32, 16);
        assert_eq!(dst, src);
    }
}
