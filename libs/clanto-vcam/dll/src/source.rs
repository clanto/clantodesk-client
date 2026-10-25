// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Media source a un solo flusso video NV12, 1280x720 o 640x480 a 30 fps.
//! Ogni `RequestSample` consegna subito l'ultimo frame del servizio.

use clanto_vcam::{
    frame::{black, fit, nv12_len},
    shared::Mapping,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use windows::{
    core::{implement, ComObject, Interface, Ref, Result, BOOL, GUID, HRESULT},
    Win32::{
        Foundation::{ERROR_SET_NOT_FOUND, E_UNEXPECTED, S_OK},
        Media::{
            KernelStreaming::{IKsControl, IKsControl_Impl, KSIDENTIFIER, PINNAME_VIDEO_CAPTURE},
            MediaFoundation::*,
        },
        System::Com::StructuredStorage::PROPVARIANT,
    },
};

const FORMATS: [(u32, u32); 2] = [(1280, 720), (640, 480)];
const FPS: u32 = 30;
const FRAME_100NS: i64 = 10_000_000 / FPS as i64;
const REOPEN_EVERY: Duration = Duration::from_secs(1);

fn media_type(w: u32, h: u32) -> Result<IMFMediaType> {
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        t.SetUINT64(&MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)?;
        t.SetUINT64(&MF_MT_FRAME_RATE, ((FPS as u64) << 32) | 1)?;
        t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        t.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        t.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
        t.SetUINT32(&MF_MT_DEFAULT_STRIDE, w)?;
        t.SetUINT32(&MF_MT_SAMPLE_SIZE, w * h * 3 / 2)?;
        t.SetUINT32(&MF_MT_AVG_BITRATE, w * h * 3 / 2 * 8 * FPS)?;
        Ok(t)
    }
}

pub fn create() -> Result<IMFMediaSource> {
    let source = ComObject::new(Source::new()?);
    let ex: IMFMediaSourceEx = source.to_interface();
    let iface: IMFMediaSource = ex.cast()?;
    let stream = ComObject::new(Stream::new(iface.clone(), source.sd.clone())?);
    *source.stream.lock().unwrap() = Some(stream);
    Ok(iface)
}

#[derive(Default)]
struct SourceState {
    started: bool,
    shutdown: bool,
}

#[implement(IMFMediaSourceEx, IMFGetService, IKsControl)]
struct Source {
    queue: IMFMediaEventQueue,
    pd: IMFPresentationDescriptor,
    sd: IMFStreamDescriptor,
    attrs: IMFAttributes,
    stream: Mutex<Option<ComObject<Stream>>>,
    state: Mutex<SourceState>,
}

impl Source {
    fn new() -> Result<Self> {
        unsafe {
            let types = FORMATS.iter().map(|&(w, h)| media_type(w, h).map(Some)).collect::<Result<Vec<_>>>()?;
            let sd = MFCreateStreamDescriptor(0, &types)?;
            sd.GetMediaTypeHandler()?.SetCurrentMediaType(types[0].as_ref())?;
            sd.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
            sd.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            sd.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            sd.SetUINT32(&MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MFFrameSourceTypes_Color.0 as u32)?;
            let pd = MFCreatePresentationDescriptor(Some(&[Some(sd.clone())]))?;
            pd.SelectStream(0)?;
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 1)?;
            Ok(Self {
                queue: MFCreateEventQueue()?,
                pd,
                sd,
                attrs: attrs.ok_or(E_UNEXPECTED)?,
                stream: Mutex::new(None),
                state: Mutex::new(SourceState::default()),
            })
        }
    }

    fn check(&self) -> Result<()> {
        if self.state.lock().unwrap().shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(())
    }

    fn stream(&self) -> Result<ComObject<Stream>> {
        self.stream.lock().unwrap().clone().ok_or_else(|| MF_E_SHUTDOWN.into())
    }
}

impl IMFMediaEventGenerator_Impl for Source_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        unsafe { self.queue.GetEvent(flags.0) }
    }
    fn BeginGetEvent(&self, cb: Ref<'_, IMFAsyncCallback>, state: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        unsafe { self.queue.BeginGetEvent(cb.as_ref(), state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<'_, IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.queue.EndGetEvent(result.as_ref()) }
    }
    fn QueueEvent(&self, met: u32, ext: *const GUID, hr: HRESULT, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.queue.QueueEventParamVar(met, ext, hr, value) }
    }
}

impl IMFMediaSource_Impl for Source_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        self.check()?;
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }

    fn CreatePresentationDescriptor(&self) -> Result<IMFPresentationDescriptor> {
        self.check()?;
        unsafe { self.pd.Clone() }
    }

    fn Start(&self, pd: Ref<'_, IMFPresentationDescriptor>, _time_format: *const GUID, start: *const PROPVARIANT) -> Result<()> {
        self.check()?;
        let pd = pd.ok()?;
        let (w, h) = unsafe {
            let mut selected = BOOL::default();
            let mut sd = None;
            pd.GetStreamDescriptorByIndex(0, &mut selected, &mut sd)?;
            let sd = sd.ok_or(E_UNEXPECTED)?;
            let size = sd.GetMediaTypeHandler()?.GetCurrentMediaType()?.GetUINT64(&MF_MT_FRAME_SIZE)?;
            ((size >> 32) as usize, (size & 0xffff_ffff) as usize)
        };
        if !FORMATS.iter().any(|&(fw, fh)| (fw as usize, fh as usize) == (w, h)) {
            return Err(MF_E_INVALIDMEDIATYPE.into());
        }
        let stream = self.stream()?;
        let first = {
            let mut st = self.state.lock().unwrap();
            !std::mem::replace(&mut st.started, true)
        };
        let unknown: windows::core::IUnknown = stream.to_interface();
        let event = if first { MENewStream } else { MEUpdatedStream };
        unsafe { self.queue.QueueEventParamUnk(event.0 as u32, &GUID::zeroed(), S_OK, &unknown)? };
        stream.start(w, h)?;
        unsafe { self.queue.QueueEventParamVar(MESourceStarted.0 as u32, &GUID::zeroed(), S_OK, start) }
    }

    fn Stop(&self) -> Result<()> {
        self.check()?;
        self.stream()?.stop()?;
        unsafe { self.queue.QueueEventParamVar(MESourceStopped.0 as u32, &GUID::zeroed(), S_OK, std::ptr::null()) }
    }

    fn Pause(&self) -> Result<()> {
        Err(MF_E_INVALID_STATE_TRANSITION.into())
    }

    fn Shutdown(&self) -> Result<()> {
        self.state.lock().unwrap().shutdown = true;
        if let Some(stream) = self.stream.lock().unwrap().take() {
            stream.shutdown();
        }
        unsafe { self.queue.Shutdown() }
    }
}

impl IMFMediaSourceEx_Impl for Source_Impl {
    fn GetSourceAttributes(&self) -> Result<IMFAttributes> {
        self.check()?;
        Ok(self.attrs.clone())
    }

    fn GetStreamAttributes(&self, id: u32) -> Result<IMFAttributes> {
        self.check()?;
        if id != 0 {
            return Err(MF_E_INVALIDSTREAMNUMBER.into());
        }
        self.sd.cast()
    }

    fn SetD3DManager(&self, _manager: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        Ok(())
    }
}

impl IMFGetService_Impl for Source_Impl {
    fn GetService(&self, _service: *const GUID, _riid: *const GUID, _ppv: *mut *mut core::ffi::c_void) -> Result<()> {
        Err(MF_E_UNSUPPORTED_SERVICE.into())
    }
}

/// Nessuna proprietà della camera: il Frame Server lo accetta.
impl IKsControl_Impl for Source_Impl {
    fn KsProperty(&self, _p: *const KSIDENTIFIER, _pl: u32, _d: *mut core::ffi::c_void, _dl: u32, _r: *mut u32) -> Result<()> {
        Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND.0).into())
    }
    fn KsMethod(&self, _m: *const KSIDENTIFIER, _ml: u32, _d: *mut core::ffi::c_void, _dl: u32, _r: *mut u32) -> Result<()> {
        Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND.0).into())
    }
    fn KsEvent(&self, _e: *const KSIDENTIFIER, _el: u32, _d: *mut core::ffi::c_void, _dl: u32, _r: *mut u32) -> Result<()> {
        Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND.0).into())
    }
}

struct StreamState {
    parent: Option<IMFMediaSource>,
    state: MF_STREAM_STATE,
    width: usize,
    height: usize,
    mapping: Option<Mapping>,
    last_open: Option<Instant>,
    frame: Vec<u8>,
    shutdown: bool,
}

impl StreamState {
    /// L'ultimo frame del servizio adattato a `dst`, o nero.
    fn fill(&mut self, dst: &mut [u8]) {
        let (w, h) = (self.width, self.height);
        if self.mapping.is_none() && self.last_open.map_or(true, |t| t.elapsed() >= REOPEN_EVERY) {
            self.last_open = Some(Instant::now());
            self.mapping = Mapping::open();
        }
        match self.mapping.as_ref().and_then(|m| m.view.read(&mut self.frame)) {
            Some((sw, sh, _)) => fit(&self.frame, sw, sh, dst, w, h),
            None => black(dst, w, h),
        }
    }
}

#[implement(IMFMediaStream2)]
struct Stream {
    queue: IMFMediaEventQueue,
    sd: IMFStreamDescriptor,
    inner: Mutex<StreamState>,
}

impl Stream {
    fn new(parent: IMFMediaSource, sd: IMFStreamDescriptor) -> Result<Self> {
        Ok(Self {
            queue: unsafe { MFCreateEventQueue()? },
            sd,
            inner: Mutex::new(StreamState {
                parent: Some(parent),
                state: MF_STREAM_STATE_STOPPED,
                width: FORMATS[0].0 as usize,
                height: FORMATS[0].1 as usize,
                mapping: None,
                last_open: None,
                frame: Vec::new(),
                shutdown: false,
            }),
        })
    }

    fn start(&self, width: usize, height: usize) -> Result<()> {
        {
            let mut st = self.inner.lock().unwrap();
            st.width = width;
            st.height = height;
            st.state = MF_STREAM_STATE_RUNNING;
        }
        unsafe { self.queue.QueueEventParamVar(MEStreamStarted.0 as u32, &GUID::zeroed(), S_OK, std::ptr::null()) }
    }

    fn stop(&self) -> Result<()> {
        self.inner.lock().unwrap().state = MF_STREAM_STATE_STOPPED;
        unsafe { self.queue.QueueEventParamVar(MEStreamStopped.0 as u32, &GUID::zeroed(), S_OK, std::ptr::null()) }
    }

    /// Rompe il riferimento circolare con la source.
    fn shutdown(&self) {
        let mut st = self.inner.lock().unwrap();
        st.shutdown = true;
        st.parent = None;
        st.mapping = None;
        drop(st);
        unsafe {
            let _ = self.queue.Shutdown();
        }
    }
}

impl IMFMediaEventGenerator_Impl for Stream_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        unsafe { self.queue.GetEvent(flags.0) }
    }
    fn BeginGetEvent(&self, cb: Ref<'_, IMFAsyncCallback>, state: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        unsafe { self.queue.BeginGetEvent(cb.as_ref(), state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<'_, IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.queue.EndGetEvent(result.as_ref()) }
    }
    fn QueueEvent(&self, met: u32, ext: *const GUID, hr: HRESULT, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.queue.QueueEventParamVar(met, ext, hr, value) }
    }
}

impl IMFMediaStream_Impl for Stream_Impl {
    fn GetMediaSource(&self) -> Result<IMFMediaSource> {
        self.inner.lock().unwrap().parent.clone().ok_or_else(|| MF_E_SHUTDOWN.into())
    }

    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        if self.inner.lock().unwrap().shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(self.sd.clone())
    }

    fn RequestSample(&self, token: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        let sample = {
            let mut st = self.inner.lock().unwrap();
            if st.shutdown {
                return Err(MF_E_SHUTDOWN.into());
            }
            if st.state != MF_STREAM_STATE_RUNNING {
                return Err(MF_E_MEDIA_SOURCE_WRONGSTATE.into());
            }
            let len = nv12_len(st.width, st.height);
            unsafe {
                let buffer = MFCreateMemoryBuffer(len as u32)?;
                let mut data = std::ptr::null_mut();
                buffer.Lock(&mut data, None, None)?;
                st.fill(std::slice::from_raw_parts_mut(data, len));
                buffer.Unlock()?;
                buffer.SetCurrentLength(len as u32)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(MFGetSystemTime())?;
                sample.SetSampleDuration(FRAME_100NS)?;
                if let Some(token) = token.as_ref() {
                    sample.SetUnknown(&MFSampleExtension_Token, token)?;
                }
                sample
            }
        };
        unsafe { self.queue.QueueEventParamUnk(MEMediaSample.0 as u32, &GUID::zeroed(), S_OK, &sample) }
    }
}

impl IMFMediaStream2_Impl for Stream_Impl {
    fn SetStreamState(&self, value: MF_STREAM_STATE) -> Result<()> {
        self.inner.lock().unwrap().state = value;
        Ok(())
    }

    fn GetStreamState(&self) -> Result<MF_STREAM_STATE> {
        Ok(self.inner.lock().unwrap().state)
    }
}
