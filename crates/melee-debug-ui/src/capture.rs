use std::ffi::{OsString, c_void};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::audio::{self, AudioTrack};
use crate::gpu::*;
use crate::lock;

const ROW_ALIGNMENT: u32 = 256;
const DEFAULT_BITRATE: &str = "40M";
const MAX_CREDITS: i64 = 2;
const AURORA_FRAME_SLOT_COUNT: i64 = 2;
const RING_SLOTS: usize = (MAX_CREDITS + AURORA_FRAME_SLOT_COUNT + 1) as usize;
const CHANNEL_CAPACITY: usize = RING_SLOTS;

#[repr(C)]
pub struct CaptureFrame {
    device: WGPUDevice,
    encoder: WGPUCommandEncoder,
    texture: WGPUTexture,
    format: WGPUTextureFormat,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Encoder {
    VideoToolbox,
    X264,
    Other(String),
}

impl Encoder {
    fn from_name(name: &str) -> Self {
        match name {
            "h264_videotoolbox" => Self::VideoToolbox,
            "libx264" => Self::X264,
            other => Self::Other(other.to_owned()),
        }
    }

    fn platform_default() -> Self {
        if cfg!(target_os = "macos") {
            Self::VideoToolbox
        } else {
            Self::X264
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::VideoToolbox => "h264_videotoolbox",
            Self::X264 => "libx264",
            Self::Other(name) => name,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Geometry {
    width: u32,
    height: u32,
}

impl Geometry {
    fn even(width: u32, height: u32) -> Self {
        Self {
            width: width & !1,
            height: height & !1,
        }
    }

    fn out_stride(self) -> usize {
        self.width as usize * 4
    }

    fn frame_bytes(self) -> usize {
        self.out_stride().saturating_mul(self.height as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Source {
    width: u32,
    height: u32,
}

impl Source {
    fn of(frame: &CaptureFrame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotState {
    Free,
    Submitted,
    Mapping,
}

struct Spec {
    width: u32,
    height: u32,
    frame_rate: u32,
    pix_fmt: &'static str,
    encoder: Encoder,
    bitrate: String,
    out: PathBuf,
}

fn padded_stride(width: u32) -> u32 {
    let bytes = width.saturating_mul(4);
    bytes
        .checked_next_multiple_of(ROW_ALIGNMENT)
        .unwrap_or(bytes)
}

fn pack_rows(src: &[u8], padded: usize, out_stride: usize, rows: usize, dst: &mut Vec<u8>) -> bool {
    dst.clear();
    dst.reserve(out_stride.saturating_mul(rows));
    for row in 0..rows {
        let start = row.saturating_mul(padded);
        let Some(slice) = start
            .checked_add(out_stride)
            .and_then(|end| src.get(start..end))
        else {
            return false;
        };
        dst.extend_from_slice(slice);
    }
    true
}

fn pix_fmt(format: WGPUTextureFormat) -> Option<&'static str> {
    if format == WGPUTextureFormat_BGRA8Unorm || format == WGPUTextureFormat_BGRA8UnormSrgb {
        Some("bgra")
    } else if format == WGPUTextureFormat_RGBA8Unorm || format == WGPUTextureFormat_RGBA8UnormSrgb {
        Some("rgba")
    } else {
        None
    }
}

fn sim_frame_rate() -> u32 {
    crate::game::sim_hz().max(1)
}

fn ffmpeg_args(spec: &Spec) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        spec.pix_fmt,
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    args.push("-video_size".to_owned());
    args.push(format!("{}x{}", spec.width, spec.height));
    args.push("-framerate".to_owned());
    args.push(spec.frame_rate.to_string());
    args.push("-i".to_owned());
    args.push("pipe:0".to_owned());
    args.push("-an".to_owned());
    args.push("-c:v".to_owned());
    args.push(spec.encoder.name().to_owned());
    match spec.encoder {
        Encoder::X264 => {
            args.extend(["-preset", "veryfast", "-crf", "18"].map(str::to_owned));
        }
        Encoder::VideoToolbox | Encoder::Other(_) => {
            args.push("-b:v".to_owned());
            args.push(spec.bitrate.clone());
        }
    }
    args.extend(
        [
            "-pix_fmt",
            "yuv420p",
            "-fps_mode",
            "passthrough",
            "-movflags",
            "+faststart",
        ]
        .map(str::to_owned),
    );
    args.push(spec.out.to_string_lossy().into_owned());
    args
}

#[derive(Default)]
struct Pressure {
    inflight: AtomicI64,
    encoded: AtomicU64,
    disabled: AtomicBool,
    started: AtomicBool,
}

impl Pressure {
    fn recorded(&self) {
        self.inflight.fetch_add(1, Ordering::AcqRel);
    }

    fn released(&self) {
        self.inflight.fetch_sub(1, Ordering::AcqRel);
    }

    fn encoded(&self) {
        self.encoded.fetch_add(1, Ordering::AcqRel);
    }

    fn encoded_frames(&self) -> u64 {
        self.encoded.load(Ordering::Acquire)
    }

    fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::Acquire)
    }

    fn disable(&self) {
        self.disabled.store(true, Ordering::Release);
    }

    fn should_wait(&self) -> bool {
        !self.is_disabled() && self.inflight.load(Ordering::Acquire) >= MAX_CREDITS
    }

    fn draining(&self) -> bool {
        !self.is_disabled() && self.inflight.load(Ordering::Acquire) > 0
    }

    fn has_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    fn active(&self) -> bool {
        !self.is_disabled() && self.has_started()
    }
}

struct Slot {
    buffer: WGPUBuffer,
    state: SlotState,
}

struct Writer {
    frames: SyncSender<Vec<u8>>,
    recycle: Receiver<Vec<u8>>,
    thread: JoinHandle<()>,
}

struct Running {
    source: Source,
    geometry: Geometry,
    frame_rate: u32,
    padded_stride: usize,
    buffer_size: usize,
    ring: Vec<Slot>,
    writer: Writer,
    recorded: u64,
    audio: Option<AudioTrack>,
}

struct Request {
    out: PathBuf,
    encoder: Encoder,
    bitrate: String,
    binary: OsString,
}

impl Request {
    fn from_env() -> Option<Self> {
        let out = PathBuf::from(std::env::var_os("MELEE_CAPTURE")?);
        let encoder = std::env::var("MELEE_CAPTURE_ENCODER")
            .ok()
            .map_or_else(Encoder::platform_default, |name| {
                Encoder::from_name(name.trim())
            });
        let bitrate =
            std::env::var("MELEE_CAPTURE_BITRATE").unwrap_or_else(|_| DEFAULT_BITRATE.to_owned());
        let binary =
            std::env::var_os("MELEE_CAPTURE_FFMPEG").unwrap_or_else(|| OsString::from("ffmpeg"));
        Some(Self {
            out,
            encoder,
            bitrate,
            binary,
        })
    }
}

#[derive(Default)]
struct CaptureState {
    request: Option<Request>,
    running: Option<Running>,
    finished: bool,
}

#[derive(Default)]
pub struct Capture {
    state: Mutex<CaptureState>,
    pressure: Arc<Pressure>,
}

impl Capture {
    pub fn from_env() -> Self {
        Self {
            state: Mutex::new(CaptureState {
                request: Request::from_env(),
                ..CaptureState::default()
            }),
            pressure: Arc::default(),
        }
    }

    pub fn should_wait(&self) -> bool {
        self.pressure.should_wait()
    }

    pub fn draining(&self) -> bool {
        self.pressure.draining()
    }

    pub fn active(&self) -> bool {
        self.pressure.active()
    }

    pub fn has_started(&self) -> bool {
        self.pressure.has_started()
    }

    pub fn stalled(&self, waited_ms: u32) {
        if !self.pressure.active() {
            return;
        }
        self.fail(&format!("the encoder took no frame for {waited_ms} ms"));
    }

    fn fail(&self, message: &str) {
        eprintln!("capture: {message}, disabling capture");
        self.pressure.disable();
    }

    /// # Safety
    /// `frame` must describe a live surface texture and an open command encoder.
    pub unsafe fn record(&self, frame: &CaptureFrame) {
        let mut state = lock(&self.state);
        if state.finished || self.pressure.is_disabled() {
            return;
        }
        if state.running.is_none() {
            match self.start(&state, frame) {
                Some(running) => state.running = Some(running),
                None => return,
            }
        }
        let Some(running) = state.running.as_mut() else {
            return;
        };
        if running.source != Source::of(frame) {
            self.fail("the window changed size mid-capture");
            return;
        }
        if running.frame_rate != sim_frame_rate() {
            self.fail("the simulation rate changed mid-capture");
            return;
        }
        let padded_stride = running.padded_stride;
        let Some(index) = running
            .ring
            .iter()
            .position(|slot| slot.state == SlotState::Free)
        else {
            self.fail("the readback ring ran dry");
            return;
        };
        running.recorded += 1;
        let Some(slot) = running.ring.get_mut(index) else {
            return;
        };
        let source = WGPUTexelCopyTextureInfo {
            texture: frame.texture,
            mipLevel: 0,
            origin: WGPUOrigin3D { x: 0, y: 0, z: 0 },
            aspect: WGPUTextureAspect_All,
        };
        let destination = WGPUTexelCopyBufferInfo {
            layout: WGPUTexelCopyBufferLayout {
                offset: 0,
                bytesPerRow: padded_stride as u32,
                rowsPerImage: frame.height,
            },
            buffer: slot.buffer,
        };
        let extent = WGPUExtent3D {
            width: frame.width,
            height: frame.height,
            depthOrArrayLayers: 1,
        };
        unsafe {
            wgpuCommandEncoderCopyTextureToBuffer(frame.encoder, &source, &destination, &extent);
        }
        slot.state = SlotState::Submitted;
        self.pressure.recorded();
    }

    fn start(&self, state: &CaptureState, frame: &CaptureFrame) -> Option<Running> {
        let request = state.request.as_ref()?;
        let Some(pix) = pix_fmt(frame.format) else {
            eprintln!(
                "capture: surface format {} is neither BGRA8 nor RGBA8, disabling capture",
                frame.format
            );
            self.pressure.disable();
            return None;
        };
        let geometry = Geometry::even(frame.width, frame.height);
        if geometry.width == 0 || geometry.height == 0 {
            eprintln!(
                "capture: surface is {}x{}, too small to encode, disabling capture",
                frame.width, frame.height
            );
            self.pressure.disable();
            return None;
        }
        let padded_stride = padded_stride(frame.width) as usize;
        let buffer_size = padded_stride.saturating_mul(frame.height as usize);
        let frame_rate = sim_frame_rate();
        let spec = Spec {
            width: geometry.width,
            height: geometry.height,
            frame_rate,
            pix_fmt: pix,
            encoder: request.encoder.clone(),
            bitrate: request.bitrate.clone(),
            out: request.out.clone(),
        };
        let writer = match spawn_writer(&request.binary, &spec, Arc::clone(&self.pressure)) {
            Ok(writer) => writer,
            Err(error) => {
                eprintln!(
                    "capture: cannot start {} ({error}), disabling capture; VSync stays off for this session",
                    request.binary.to_string_lossy()
                );
                self.pressure.disable();
                return None;
            }
        };
        let mut ring: Vec<Slot> = Vec::with_capacity(RING_SLOTS);
        for _ in 0..RING_SLOTS {
            let desc = WGPUBufferDescriptor {
                usage: WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst,
                size: buffer_size as u64,
                ..Default::default()
            };
            let buffer = unsafe { wgpuDeviceCreateBuffer(frame.device, &desc) };
            if buffer.is_null() {
                for slot in &ring {
                    unsafe { wgpuBufferRelease(slot.buffer) };
                }
                eprintln!("capture: cannot allocate a readback buffer, disabling capture");
                self.pressure.disable();
                return None;
            }
            ring.push(Slot {
                buffer,
                state: SlotState::Free,
            });
        }
        eprintln!(
            "capture: recording {}x{} {pix} to {} via {}",
            geometry.width,
            geometry.height,
            spec.out.display(),
            spec.encoder.name()
        );
        let audio = AudioTrack::create(audio::track_path(&spec.out))
            .inspect_err(|error| {
                eprintln!("capture: no audio track ({error}), recording video only")
            })
            .ok();
        self.pressure.started.store(true, Ordering::Release);
        Some(Running {
            source: Source::of(frame),
            geometry,
            frame_rate,
            padded_stride,
            buffer_size,
            ring,
            writer,
            recorded: 0,
            audio,
        })
    }

    pub fn submitted(&self) {
        let pending = {
            let mut state = lock(&self.state);
            let Some(running) = state.running.as_mut() else {
                return;
            };
            let size = running.buffer_size;
            let mut pending = Vec::new();
            for (index, slot) in running.ring.iter_mut().enumerate() {
                if slot.state != SlotState::Submitted {
                    continue;
                }
                slot.state = SlotState::Mapping;
                pending.push((index, slot.buffer, size));
            }
            pending
        };
        let user = std::ptr::from_ref(self).cast_mut().cast::<c_void>();
        for (index, buffer, size) in pending {
            let info = WGPUBufferMapCallbackInfo {
                mode: WGPUCallbackMode_AllowSpontaneous,
                callback: Some(map_done),
                userdata1: user,
                userdata2: std::ptr::without_provenance_mut(index),
                ..Default::default()
            };
            unsafe { wgpuBufferMapAsync(buffer, WGPUMapMode_Read, 0, size, info) };
        }
    }

    fn mapped(&self, status: WGPUMapAsyncStatus, index: usize) {
        let mut state = lock(&self.state);
        let Some(running) = state.running.as_mut() else {
            return;
        };
        let Some(buffer) = running
            .ring
            .get(index)
            .filter(|slot| slot.state == SlotState::Mapping)
            .map(|slot| slot.buffer)
        else {
            return;
        };
        if let Some(slot) = running.ring.get_mut(index) {
            slot.state = SlotState::Free;
        }
        if status != WGPUMapAsyncStatus_Success {
            self.pressure.released();
            if status != WGPUMapAsyncStatus_CallbackCancelled
                && status != WGPUMapAsyncStatus_Aborted
            {
                self.fail("a readback map failed");
            }
            return;
        }
        let mut frame = running.writer.recycle.try_recv().unwrap_or_default();
        let mapped = unsafe { wgpuBufferGetConstMappedRange(buffer, 0, running.buffer_size) };
        if mapped.is_null() {
            unsafe { wgpuBufferUnmap(buffer) };
            self.pressure.released();
            self.fail("a mapped readback buffer had no range");
            return;
        }
        let src = unsafe { std::slice::from_raw_parts(mapped.cast::<u8>(), running.buffer_size) };
        let packed = pack_rows(
            src,
            running.padded_stride,
            running.geometry.out_stride(),
            running.geometry.height as usize,
            &mut frame,
        );
        unsafe { wgpuBufferUnmap(buffer) };
        if !packed || frame.len() != running.geometry.frame_bytes() {
            self.pressure.released();
            self.fail("a readback frame came back short");
            return;
        }
        match running.writer.frames.try_send(frame) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.pressure.released();
                self.fail("the encoder queue overflowed");
            }
            Err(TrySendError::Disconnected(_)) => {
                self.pressure.released();
                self.fail("the encoder queue is gone");
            }
        }
    }

    pub fn audio(&self, samples: &[f32]) {
        let mut state = lock(&self.state);
        if state.finished || self.pressure.is_disabled() {
            return;
        }
        let Some(running) = state.running.as_mut() else {
            return;
        };
        let Some(track) = running.audio.as_mut() else {
            return;
        };
        if let Err(error) = track.write(samples) {
            eprintln!("capture: audio track failed ({error}), the rest is video only");
            running.audio = None;
        }
    }

    pub fn finish(&self) {
        let (running, target) = {
            let mut state = lock(&self.state);
            if state.finished {
                return;
            }
            state.finished = true;
            let target = state
                .request
                .as_ref()
                .map(|request| (request.binary.clone(), request.out.clone()));
            (state.running.take(), target)
        };
        let Some(mut running) = running else {
            return;
        };
        let audio = running.audio.take();
        for slot in &running.ring {
            unsafe { wgpuBufferRelease(slot.buffer) };
        }
        let Writer {
            frames,
            recycle,
            thread,
        } = running.writer;
        drop(frames);
        drop(recycle);
        if thread.join().is_err() {
            eprintln!("capture: the writer thread panicked");
        }
        let encoded = self.pressure.encoded_frames();
        if encoded == running.recorded {
            eprintln!("capture: {encoded} frames reached ffmpeg");
        } else {
            eprintln!(
                "capture: read back {} frames but only {encoded} reached ffmpeg, the file is incomplete",
                running.recorded
            );
        }
        let (Some(track), Some((binary, out))) = (audio, target) else {
            return;
        };
        let seconds = track.seconds();
        if track.frames() == 0 {
            return;
        }
        match track
            .finish()
            .and_then(|path| audio::mux(&binary, &out, &path))
        {
            Ok(()) => eprintln!(
                "capture: muxed {seconds:.2} s of audio into {}",
                out.display()
            ),
            Err(error) => {
                eprintln!("capture: cannot mux the audio ({error}), the file is video only")
            }
        }
    }
}

unsafe extern "C" fn map_done(
    status: WGPUMapAsyncStatus,
    _message: WGPUStringView,
    userdata1: *mut c_void,
    userdata2: *mut c_void,
) {
    let Some(capture) = (unsafe { userdata1.cast::<Capture>().as_ref() }) else {
        return;
    };
    capture.mapped(status, userdata2.addr());
}

fn spawn_writer(binary: &OsString, spec: &Spec, pressure: Arc<Pressure>) -> io::Result<Writer> {
    let mut child = Command::new(binary)
        .args(ffmpeg_args(spec))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("ffmpeg stdin was not a pipe"));
    };
    let (frames_tx, frames_rx) = sync_channel(CHANNEL_CAPACITY);
    let (recycle_tx, recycle_rx) = sync_channel(CHANNEL_CAPACITY);
    let thread = thread::Builder::new()
        .name("capture-writer".into())
        .spawn(move || write_frames(child, stdin, &frames_rx, &recycle_tx, &pressure));
    match thread {
        Ok(thread) => Ok(Writer {
            frames: frames_tx,
            recycle: recycle_rx,
            thread,
        }),
        Err(error) => Err(error),
    }
}

fn write_frames(
    mut child: Child,
    mut stdin: ChildStdin,
    frames: &Receiver<Vec<u8>>,
    recycle: &SyncSender<Vec<u8>>,
    pressure: &Pressure,
) {
    let mut written: u64 = 0;
    let mut broken = false;
    while let Ok(frame) = frames.recv() {
        if !broken {
            match stdin.write_all(&frame) {
                Ok(()) => {
                    written += 1;
                    pressure.encoded();
                }
                Err(error) => {
                    eprintln!("capture: ffmpeg stopped reading ({error}), disabling capture");
                    pressure.disable();
                    broken = true;
                }
            }
        }
        let _ = recycle.try_send(frame);
        pressure.released();
    }
    drop(stdin);
    match child.wait() {
        Ok(status) => eprintln!("capture: ffmpeg exited with {status} after {written} frames"),
        Err(error) => eprintln!("capture: cannot reap ffmpeg ({error})"),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::capture::{
        AURORA_FRAME_SLOT_COUNT, CHANNEL_CAPACITY, Encoder, Geometry, MAX_CREDITS, Pressure,
        RING_SLOTS, Spec, ffmpeg_args, pack_rows, padded_stride, pix_fmt, spawn_writer,
    };
    use crate::gpu::{
        WGPUTextureFormat_BGRA8Unorm, WGPUTextureFormat_BGRA8UnormSrgb,
        WGPUTextureFormat_Depth32Float, WGPUTextureFormat_RGBA8Unorm,
        WGPUTextureFormat_RGBA8UnormSrgb,
    };

    #[test]
    fn padded_stride_rounds_up_to_256() {
        assert_eq!(padded_stride(960), 3840);
        assert_eq!(padded_stride(2560), 10240);
        assert_eq!(padded_stride(100), 512);
        for width in [1u32, 3, 63, 64, 65, 333, 1279, 1920, 2561] {
            let stride = padded_stride(width);
            assert_eq!(stride % 256, 0, "width {width} gave {stride}");
            assert!(stride >= width * 4, "width {width} gave {stride}");
            assert!(stride < width * 4 + 256, "width {width} gave {stride}");
        }
    }

    fn padded_image(width: u32, height: u32) -> (Vec<u8>, usize) {
        let padded = padded_stride(width) as usize;
        let mut src = vec![0u8; padded * height as usize];
        for row in 0..height as usize {
            let marker = (row + 1) as u8;
            let start = row * padded;
            src[start..start + width as usize * 4].fill(marker);
        }
        (src, padded)
    }

    #[test]
    fn pack_rows_drops_the_row_padding() {
        let (src, padded) = padded_image(100, 4);
        let out_stride = 100 * 4;
        assert!(padded > out_stride);
        let mut dst = Vec::new();
        assert!(pack_rows(&src, padded, out_stride, 4, &mut dst));
        assert_eq!(dst.len(), out_stride * 4);
        for row in 0..4 {
            let marker = (row + 1) as u8;
            assert!(
                dst[row * out_stride..(row + 1) * out_stride]
                    .iter()
                    .all(|byte| *byte == marker),
                "row {row} is not all {marker}"
            );
        }
    }

    #[test]
    fn pack_rows_crops_an_odd_surface_to_even_dimensions() {
        let (width, height) = (101u32, 7u32);
        let (src, padded) = padded_image(width, height);
        let geometry = Geometry::even(width, height);
        assert_eq!(
            geometry,
            Geometry {
                width: 100,
                height: 6
            }
        );
        let mut dst = Vec::new();
        assert!(pack_rows(
            &src,
            padded,
            geometry.out_stride(),
            geometry.height as usize,
            &mut dst,
        ));
        assert_eq!(dst.len(), geometry.frame_bytes());
        assert_eq!(dst.len(), 100 * 4 * 6);
        assert!(dst.iter().all(|byte| *byte != 0));
    }

    #[test]
    fn pack_rows_reuses_the_destination() {
        let (src, padded) = padded_image(8, 2);
        let mut dst = vec![0xAAu8; 9999];
        assert!(pack_rows(&src, padded, 32, 2, &mut dst));
        assert_eq!(dst.len(), 64);
    }

    #[test]
    fn pack_rows_copies_every_byte_when_the_stride_needs_no_padding() {
        let (width, height) = (64u32, 3u32);
        let out_stride = width as usize * 4;
        assert_eq!(padded_stride(width) as usize, out_stride);
        let mut src = vec![0u8; out_stride * height as usize];
        for (index, byte) in src.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        let mut dst = Vec::new();
        assert!(pack_rows(
            &src,
            out_stride,
            out_stride,
            height as usize,
            &mut dst
        ));
        assert_eq!(dst, src);
    }

    #[test]
    fn pack_rows_handles_an_empty_image_and_refuses_a_short_source() {
        let mut dst = vec![0xAAu8; 16];
        assert!(pack_rows(&[], 256, 256, 0, &mut dst));
        assert!(dst.is_empty());

        let (src, padded) = padded_image(8, 2);
        assert!(!pack_rows(&src[..padded], padded, 32, 2, &mut dst));
        assert_eq!(dst.len(), 32);
    }

    #[test]
    fn geometry_rounds_a_tiny_surface_down_to_nothing() {
        assert_eq!(
            Geometry::even(1, 1),
            Geometry {
                width: 0,
                height: 0
            }
        );
        assert_eq!(
            Geometry::even(0, 0),
            Geometry {
                width: 0,
                height: 0
            }
        );
        assert_eq!(Geometry::even(1, 1).frame_bytes(), 0);
        assert_eq!(
            Geometry::even(3, 5),
            Geometry {
                width: 2,
                height: 4
            }
        );
    }

    #[test]
    fn the_ring_holds_every_frame_the_throttle_can_admit() {
        let admitted = MAX_CREDITS - 1;
        let queued = AURORA_FRAME_SLOT_COUNT + 1;
        assert!(
            RING_SLOTS as i64 >= admitted + queued,
            "{RING_SLOTS} slots cannot hold {} readbacks",
            admitted + queued
        );
        assert!(CHANNEL_CAPACITY as i64 >= admitted + queued);
    }

    #[test]
    fn pix_fmt_maps_only_the_eight_bit_formats() {
        assert_eq!(pix_fmt(WGPUTextureFormat_BGRA8Unorm), Some("bgra"));
        assert_eq!(pix_fmt(WGPUTextureFormat_BGRA8UnormSrgb), Some("bgra"));
        assert_eq!(pix_fmt(WGPUTextureFormat_RGBA8Unorm), Some("rgba"));
        assert_eq!(pix_fmt(WGPUTextureFormat_RGBA8UnormSrgb), Some("rgba"));
        assert_eq!(pix_fmt(WGPUTextureFormat_Depth32Float), None);
    }

    fn spec(encoder: Encoder, out: &Path) -> Spec {
        Spec {
            width: 2560,
            height: 1920,
            frame_rate: 60,
            pix_fmt: "bgra",
            encoder,
            bitrate: "40M".to_owned(),
            out: out.to_path_buf(),
        }
    }

    fn index_of(args: &[String], value: &str) -> usize {
        args.iter()
            .position(|arg| arg == value)
            .unwrap_or_else(|| panic!("{value} missing from {args:?}"))
    }

    #[test]
    fn videotoolbox_args_are_ordered_and_bitrate_based() {
        let out = PathBuf::from("/tmp/out.mp4");
        let args = ffmpeg_args(&spec(Encoder::VideoToolbox, &out));
        let input = index_of(&args, "-i");
        assert_eq!(args[input + 1], "pipe:0");
        for flag in ["-f", "-pix_fmt", "-video_size", "-framerate"] {
            assert!(index_of(&args, flag) < input, "{flag} must precede -i");
        }
        assert_eq!(args[index_of(&args, "-f") + 1], "rawvideo");
        assert_eq!(args[index_of(&args, "-video_size") + 1], "2560x1920");
        assert_eq!(args[index_of(&args, "-framerate") + 1], "60");
        assert_eq!(args[index_of(&args, "-c:v") + 1], "h264_videotoolbox");
        assert_eq!(args[index_of(&args, "-b:v") + 1], "40M");
        assert_eq!(args[index_of(&args, "-fps_mode") + 1], "passthrough");
        assert_eq!(args.last().map(String::as_str), Some("/tmp/out.mp4"));
        let pix_fmts: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(i, arg)| *arg == "-pix_fmt" && *i < args.len() - 1)
            .map(|(i, _)| &args[i + 1])
            .collect();
        assert_eq!(pix_fmts, ["bgra", "yuv420p"]);
    }

    #[test]
    fn libx264_args_swap_the_bitrate_for_a_crf() {
        let out = PathBuf::from("/tmp/out.mp4");
        let args = ffmpeg_args(&spec(Encoder::X264, &out));
        assert_eq!(args[index_of(&args, "-c:v") + 1], "libx264");
        assert_eq!(args[index_of(&args, "-preset") + 1], "veryfast");
        assert_eq!(args[index_of(&args, "-crf") + 1], "18");
        assert!(!args.iter().any(|arg| arg == "-b:v"));
    }

    #[test]
    fn the_output_frame_rate_follows_the_simulation_rate() {
        let out = PathBuf::from("/tmp/out.mp4");
        let mut spec = spec(Encoder::X264, &out);
        spec.frame_rate = 30;
        let args = ffmpeg_args(&spec);
        assert_eq!(args[index_of(&args, "-framerate") + 1], "30");
    }

    #[test]
    fn an_unknown_encoder_name_is_passed_through_with_a_bitrate() {
        let out = PathBuf::from("/tmp/out.mp4");
        let args = ffmpeg_args(&spec(Encoder::from_name("hevc_videotoolbox"), &out));
        assert_eq!(args[index_of(&args, "-c:v") + 1], "hevc_videotoolbox");
        assert_eq!(args[index_of(&args, "-b:v") + 1], "40M");
    }

    #[test]
    fn credits_gate_the_throttle_and_the_drain() {
        let pressure = Pressure::default();
        assert!(!pressure.should_wait());
        assert!(!pressure.draining());
        pressure.recorded();
        assert!(!pressure.should_wait());
        assert!(pressure.draining());
        pressure.recorded();
        assert_eq!(MAX_CREDITS, 2);
        assert!(pressure.should_wait());
        pressure.released();
        assert!(!pressure.should_wait());
        pressure.released();
        assert!(!pressure.draining());
    }

    #[test]
    fn disabling_releases_both_predicates() {
        let pressure = Pressure::default();
        pressure.recorded();
        pressure.recorded();
        assert!(pressure.should_wait());
        pressure.disable();
        assert!(!pressure.should_wait());
        assert!(!pressure.draining());
        assert!(!pressure.active());
    }

    #[test]
    fn the_throttle_releases_once_the_writer_drains() {
        let pressure = Arc::new(Pressure::default());
        for _ in 0..3 {
            pressure.recorded();
        }
        let worker = Arc::clone(&pressure);
        let drain = thread::spawn(move || {
            for _ in 0..3 {
                thread::sleep(Duration::from_millis(5));
                worker.released();
            }
        });
        let mut passes = 0u32;
        while pressure.should_wait() {
            passes += 1;
            assert!(passes < 100_000, "the throttle never released");
            thread::sleep(Duration::from_millis(1));
        }
        drain.join().expect("writer");
        assert_eq!(
            pressure.inflight.load(std::sync::atomic::Ordering::Acquire),
            0
        );
    }

    fn on_path(binary: &str) -> Option<OsString> {
        let ran = Command::new(binary)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match ran {
            Ok(status) if status.success() => Some(OsString::from(binary)),
            _ => None,
        }
    }

    #[test]
    fn spawning_a_missing_encoder_fails_without_leaving_a_writer_behind() {
        let spec = spec(Encoder::X264, Path::new("/nonexistent/out.mp4"));
        let pressure = Arc::new(Pressure::default());
        let Err(error) = spawn_writer(
            &OsString::from("/nonexistent/ffmpeg"),
            &spec,
            Arc::clone(&pressure),
        ) else {
            panic!("a missing binary must not spawn a writer");
        };
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(Arc::strong_count(&pressure), 1);
        assert!(!pressure.is_disabled());
        assert_eq!(pressure.encoded_frames(), 0);
    }

    #[test]
    fn a_real_ffmpeg_encodes_every_frame_pushed_through_the_writer() {
        let Some(binary) = on_path("ffmpeg") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("melee-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let out = dir.join("writer.mp4");
        let _ = std::fs::remove_file(&out);
        let (width, height) = (64u32, 48u32);
        let spec = Spec {
            width,
            height,
            frame_rate: 60,
            pix_fmt: "bgra",
            encoder: Encoder::X264,
            bitrate: "1M".to_owned(),
            out: out.clone(),
        };
        let pressure = Arc::new(Pressure::default());
        let writer = spawn_writer(&binary, &spec, Arc::clone(&pressure)).expect("spawn ffmpeg");
        let frame_bytes = width as usize * 4 * height as usize;
        for index in 0..30u8 {
            pressure.recorded();
            let frame = vec![index.wrapping_mul(8); frame_bytes];
            writer.frames.send(frame).expect("send frame");
        }
        let crate::capture::Writer {
            frames,
            recycle,
            thread,
        } = writer;
        drop(frames);
        drop(recycle);
        thread.join().expect("writer thread");
        assert_eq!(
            pressure.inflight.load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert!(!pressure.is_disabled(), "ffmpeg rejected the stream");
        assert_eq!(pressure.encoded_frames(), 30);

        let Some(probe) = on_path("ffprobe") else {
            return;
        };
        let output = Command::new(probe)
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=nb_read_frames,width,height,r_frame_rate",
                "-of",
                "default=nw=1",
            ])
            .arg(&out)
            .output()
            .expect("ffprobe");
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(text.contains("nb_read_frames=30"), "{text}");
        assert!(text.contains("width=64"), "{text}");
        assert!(text.contains("height=48"), "{text}");
        assert!(text.contains("r_frame_rate=60/1"), "{text}");
        let _ = std::fs::remove_file(&out);
    }
}
