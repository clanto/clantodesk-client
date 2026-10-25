// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Forwarding Clanto: canali nei due sensi dentro una sessione autorizzata,
//! separati dal port forward di upstream (che resta spento). Un tipo di
//! canale per funzione: oggi porte TCP; webcam, audio e USB si aggiungono come
//! nuovi tipi sullo stesso motore. La webcam del controller viaggia come flusso
//! media (`media`, `camera_*`). Vedi CLANTO.md, "Forwarding".

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod camera_capture;
#[cfg(any(target_os = "android", target_os = "ios"))]
#[path = "camera_capture_mobile.rs"]
pub mod camera_capture;
pub mod camera_host;
pub mod camera_sink;
pub mod catalog;
pub mod controller;
pub mod engine;
pub mod host;
pub mod media;
pub mod policy;
