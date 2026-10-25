// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Funzione Rust (src/clanto/forwarding/camera_capture_mobile.rs) chiamata da
// ClantoCamera.swift per ogni frame NV12. Restituisce false quando il flusso è
// finito: la camera va chiusa.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

bool clanto_camera_push_nv12(const uint8_t *y, size_t y_len, int32_t y_stride,
                             const uint8_t *uv, size_t uv_len, int32_t uv_stride,
                             int32_t width, int32_t height);
