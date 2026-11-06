//! Raw frame streams for analysis.
//!
//! Frames are decoded in software on purpose: it is fast on any modern CPU
//! (≈ 800 fps for 1080p H.264 at 48×27 on 16 threads, faster than the GPU
//! decoder plus download), it is exactly what exports decode, and it is
//! bit-for-bit repeatable — a run resumed at frame K sees the same pixels a
//! full run saw from K on. A pass that looks at few frames of heavy sources
//! (4K HEVC) can have them decoded and scaled on an NVIDIA GPU instead
//! ([`Decoder::Nvdec`]): the same decoder for a run and its resumption.

use std::io::{BufReader, Read};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use as_core::{CancelToken, FrameIdx, FrameRange};

use crate::filters::{TONEMAP_SDR, deinterlace};
use crate::{HdrKind, MediaError, MediaInfo, MediaResult, Source, Tools};

/// Seconds of input pre-roll before the first wanted frame: the input seek
/// lands on the keyframe before it, `trim` then picks frames by timestamp.
const PREROLL: f64 = 3.0;

/// Decode frames `from..` of `src` scaled to `w`×`h` RGB24 (bicubic, the
/// scaling TransNetV2 was trained with) and hand each to `on_frame` in order.
/// `on_frame` returns `false` to stop early. Returns the number of frames
/// delivered; a full run delivers exactly `index.len() - from`.
pub fn decode_rgb(
    tools: &Tools,
    src: Source,
    from: FrameIdx,
    (w, h): (u32, u32),
    cancel: &CancelToken,
    on_frame: impl FnMut(FrameIdx, &[u8]) -> MediaResult<bool>,
) -> MediaResult<u32> {
    let chain = [format!("scale={w}:{h}:flags=bicubic"), "format=rgb24".into()];
    decode_raw(tools, src, from, 1, &[], &chain, (w * h * 3) as usize, cancel, on_frame).map(|(n, _)| n)
}

/// The longest side a picture is worked out at (16K). Only a damaged or
/// crafted pixel shape (a sample aspect ratio of 65535:1) gets there: such a
/// picture is scaled down to fit, instead of being given a size that
/// overflows or that ffmpeg refuses.
pub const MAX_SIDE: u32 = 16_384;

/// Size (even, square pixels, upright) at which [`decode_samples`] delivers
/// `info`'s frames when they must fit `max_w`×`max_h` (and [`MAX_SIDE`]);
/// never upscaled.
pub fn sample_size(info: &MediaInfo, (max_w, max_h): (u32, u32), multiple: u32) -> (u32, u32) {
    let v = &info.video;
    let (mut dw, mut dh) = (v.width as f64 * v.pixel_aspect(), v.height as f64);
    if v.rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut dw, &mut dh);
    }
    let (max_w, max_h) = (max_w.min(MAX_SIDE), max_h.min(MAX_SIDE));
    let k = (max_w as f64 / dw).min(max_h as f64 / dh).min(1.0);
    let round = |x: f64| ((x / multiple as f64).round() as u32).max(1) * multiple;
    (round(dw * k), round(dh * k))
}

/// Decode every `step`-th frame of `src` from `from` on (`from`, `from +
/// step`, …) as upright, square-pixel, SDR RGB24 at `w`×`h` (see
/// [`sample_size`]) and hand each to `on_frame` in order. Returns the number
/// of frames delivered; a full run delivers `(index.len() - from).div_ceil(step)`.
pub fn decode_samples(
    tools: &Tools,
    src: Source,
    from: FrameIdx,
    step: u32,
    (w, h): (u32, u32),
    cancel: &CancelToken,
    on_frame: impl FnMut(FrameIdx, &[u8]) -> MediaResult<bool>,
) -> MediaResult<u32> {
    assert!(step > 0);
    let mut chain: Vec<String> = Vec::new();
    // Right after the trim, so frame `from` is sample 0.
    if step > 1 {
        chain.push(format!("select='not(mod(n,{step}))'"));
    }
    if src.info.video.hdr != HdrKind::None {
        chain.push(TONEMAP_SDR.to_string());
    }
    chain.push(format!("scale={w}:{h}:flags=bilinear,setsar=1"));
    chain.push("format=rgb24".into());
    decode_raw(tools, src, from, step, &[], &chain, (w * h * 3) as usize, cancel, on_frame).map(|(n, _)| n)
}

/// Decode one picture file — a JPEG, PNG or WebP such as a character
/// portrait (ffmpeg opens them as a one-frame video) — to upright,
/// square-pixel RGB24 at its own size, scaled down to fit [`MAX_SIDE`] if
/// needed. Returns `(w, h, rgb)`.
pub fn decode_still(tools: &Tools, path: &std::path::Path, cancel: &CancelToken) -> MediaResult<(u32, u32, Vec<u8>)> {
    let info = crate::probe::probe(tools, path)?;
    let (w, h) = sample_size(&info, (MAX_SIDE, MAX_SIDE), 1);
    let mut cmd = tools.ffmpeg();
    cmd.args(["-v", "error", "-nostdin"]);
    cmd.arg("-i").arg(path);
    cmd.args(["-map", "0:v:0", "-frames:v", "1"]);
    cmd.args(["-filter:v", &format!("scale={w}:{h}:flags=bilinear,setsar=1,format=rgb24"), "-f", "rawvideo", "-"]);
    tracing::debug!(cmd = ?cmd, "decode a still");
    let mut child = crate::proc::spawn(cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut child = Watched::new(child, cancel);
    let stderr_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    });
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    let read = stdout.read_exact(&mut rgb);
    let status = child.wait()?;
    let tail = stderr_thread.join().unwrap_or_default();
    if cancel.is_cancelled() {
        return Err(MediaError::Cancelled);
    }
    if let Err(e) = read {
        return Err(MediaError::Ffmpeg(if tail.is_empty() { format!("reading the picture: {e}") } else { tail }));
    }
    if !status.success() {
        return Err(MediaError::Ffmpeg(if tail.is_empty() { format!("exit status {status}") } else { tail }));
    }
    Ok((w, h, rgb))
}

/// Where [`decode_samples_with`] decodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decoder {
    /// In software, as [`decode_samples`] does.
    #[default]
    Cpu,
    /// On an NVIDIA GPU (NVDEC), scaled there; only the samples come down.
    /// Every 6th frame of a 4K HEVC film: 500 fps on an RTX 3060 Ti, 230 in
    /// software on 16 threads (which a busy CPU slows down further). Sources
    /// it doesn't suit (interlaced, HDR, rotated, neither H.264 nor HEVC)
    /// are decoded in software.
    Nvdec,
    /// [`Self::Nvdec`] on this CUDA device, where several NVIDIA cards are
    /// in the computer.
    NvdecOn(u32),
}

/// The options that start `decoder` on the input: none in software. CUDA
/// picks its own first device unless one is named.
pub fn input_args(decoder: Decoder) -> Vec<String> {
    let device = match decoder {
        Decoder::Cpu => return Vec::new(),
        Decoder::Nvdec => None,
        Decoder::NvdecOn(device) => Some(device),
    };
    let mut args = ["-xerror", "-hwaccel", "cuda"].map(String::from).to_vec();
    if let Some(device) = device {
        args.extend(["-hwaccel_device".into(), device.to_string()]);
    }
    args.extend(["-hwaccel_output_format", "cuda"].map(String::from));
    args
}

/// Whether NVDEC can decode `info` the way [`decode_samples`] would: its
/// deinterlacing, tone mapping and rotation run in software. Only H.264 and
/// HEVC: on others (AV1, VP9) it can drop a picture without an error, and
/// every sample after it would then be taken for the frame before it.
fn nvdec_suits(info: &MediaInfo) -> bool {
    matches!(info.video.codec.as_str(), "h264" | "hevc")
        && !info.is_interlaced()
        && info.video.hdr == HdrKind::None
        && info.video.rotation == 0
}

/// What [`decode_samples_with`] starts on when asked for `decoder`: the
/// decoder itself where the file suits NVDEC and the check of this machine
/// found CUDA working, else software. Asking for software asks no check.
pub fn decoder_for(tools: &Tools, info: &MediaInfo, decoder: Decoder) -> Decoder {
    if decoder != Decoder::Cpu && nvdec_suits(info) && cuda_works(tools) { decoder } else { Decoder::Cpu }
}

/// [`decode_samples`] with `decoder`. NVDEC is tried only where the check of
/// this machine found CUDA working ([`Tools::caps`]): a driver that failed or
/// hung there isn't started again, as nothing but a cancel would end a
/// decoder stuck in it. When it can't be used or fails part way (a codec it
/// doesn't take, a lost device), the samples from the first one not delivered
/// on are decoded in software. A run and its resumption see the same pixels
/// as long as they decode the same way; one that fell back, or a resumption
/// on a machine where the GPU decoder changed, may differ slightly in its
/// later chunks.
#[allow(clippy::too_many_arguments)]
pub fn decode_samples_with(
    tools: &Tools,
    src: Source,
    from: FrameIdx,
    step: u32,
    (w, h): (u32, u32),
    decoder: Decoder,
    cancel: &CancelToken,
    mut on_frame: impl FnMut(FrameIdx, &[u8]) -> MediaResult<bool>,
) -> MediaResult<u32> {
    assert!(step > 0);
    let decoder = decoder_for(tools, src.info, decoder);
    if decoder == Decoder::Cpu {
        return decode_samples(tools, src, from, step, (w, h), cancel, on_frame);
    }
    let mut chain: Vec<String> = Vec::new();
    if step > 1 {
        chain.push(format!("select='not(mod(n,{step}))'"));
    }
    chain.push(format!("scale_cuda={w}:{h}:interp_algo=bilinear:format=yuv420p"));
    chain.push("hwdownload,format=yuv420p,setsar=1,format=rgb24".into());
    // A frame the GPU can't decode stops it (rather than being dropped,
    // which would shift every later frame): software takes over from there.
    let input = input_args(decoder);
    let (mut delivered, mut refused) = (0u32, false);
    let gpu = decode_raw(tools, src, from, step, &input, &chain, (w * h * 3) as usize, cancel, |f, rgb| {
        let r = on_frame(f, rgb);
        refused |= r.is_err();
        delivered += 1;
        r
    });
    match gpu {
        Ok((n, _)) => Ok(n),
        // A frame count that doesn't add up is no reason to try again.
        Err(e) if refused || e.is_cancelled() || matches!(e, MediaError::Verify(_)) => Err(e),
        Err(e) => {
            tracing::warn!(error = %e.detail(), delivered, "decoding on the GPU failed; decoding in software from here");
            let next = from + delivered * step;
            Ok(delivered + decode_samples(tools, src, next, step, (w, h), cancel, on_frame)?)
        }
    }
}

/// Whether this machine's check found CUDA working, the device NVDEC runs on.
fn cuda_works(tools: &Tools) -> bool {
    tools.caps().decode.as_ref().is_some_and(|d| d.hwaccel == "cuda")
}

/// Shared decoder: `-ss` pre-roll, deinterlacing (when the source needs it),
/// `trim` to frame `from`, then `chain`; the k-th raw frame out is frame
/// `from + k·step`. `input`: options for decoding the input (`-hwaccel`). Deinterlacing runs before the trim so it sees the frames
/// before `from` too: a run that starts mid-way produces exactly the pixels
/// of a run from the start. Returns (frames delivered, last frame).
#[allow(clippy::too_many_arguments)]
fn decode_raw(
    tools: &Tools,
    src: Source,
    from: FrameIdx,
    step: u32,
    input: &[String],
    chain: &[String],
    size: usize,
    cancel: &CancelToken,
    mut on_frame: impl FnMut(FrameIdx, &[u8]) -> MediaResult<bool>,
) -> MediaResult<(u32, FrameIdx)> {
    let Source { path, info, index } = src;
    let n = index.len();
    if from >= n {
        return Ok((0, from));
    }
    let (trim_start, _) = index.trim_us(FrameRange::new(from, n));
    // One frame out per frame in (send_frame), timestamps kept: trim still
    // picks exactly the indexed frames.
    let mut filters: Vec<String> = deinterlace(info).into_iter().collect();
    filters.push(format!("trim=start={trim_start}us"));
    filters.extend(chain.iter().cloned());

    let mut cmd = tools.ffmpeg();
    cmd.args(["-v", "error", "-nostdin", "-copyts"]);
    if from > 0 {
        let seek = (index.t(from) - index.t0() - PREROLL).max(0.0);
        cmd.args(["-noaccurate_seek", "-ss", &format!("{seek:.6}")]);
    }
    cmd.args(input);
    cmd.arg("-i").arg(path);
    cmd.args(["-map", &format!("0:{}", info.video.index), "-an", "-sn", "-dn"]);
    cmd.args(["-filter:v", &filters.join(","), "-fps_mode", "passthrough", "-f", "rawvideo", "-"]);
    tracing::debug!(cmd = ?cmd, "decode for analysis");
    let mut child = crate::proc::spawn(cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let mut stdout = BufReader::with_capacity(1 << 20, child.stdout.take().expect("piped stdout"));
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut child = Watched::new(child, cancel);
    let stderr_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    });

    let mut buf = vec![0u8; size];
    let mut f = from;
    let mut delivered = 0u32;
    let stopped = loop {
        if cancel.is_cancelled() {
            break Some(MediaError::Cancelled);
        }
        match stdout.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break None,
            Err(e) => break Some(e.into()),
        }
        if f >= n {
            break Some(MediaError::Verify(format!("decoder produced more than the {n} indexed frames")));
        }
        delivered += 1;
        match on_frame(f, &buf) {
            Ok(true) => f += step,
            Ok(false) => {
                f += step;
                break Some(MediaError::Cancelled);
            }
            Err(e) => break Some(e),
        }
    };
    // A kill from the watch ends the read as an end of data or a broken pipe.
    let stopped = if child.was_killed() { Some(MediaError::Cancelled) } else { stopped };
    if let Some(e) = stopped {
        child.kill();
        let _ = child.wait();
        let _ = stderr_thread.join();
        // Stopping on purpose (on_frame said so) isn't an error for the caller.
        return match e {
            MediaError::Cancelled if !cancel.is_cancelled() => Ok((delivered, f)),
            e => Err(e),
        };
    }
    let status = child.wait()?;
    let tail = stderr_thread.join().unwrap_or_default();
    // The watch runs until the process has ended: one that stalls after its
    // last frame (a driver tearing down) is cancelled like any other.
    if child.was_killed() {
        return Err(MediaError::Cancelled);
    }
    if !status.success() {
        return Err(MediaError::Ffmpeg(if tail.is_empty() { format!("exit status {status}") } else { tail }));
    }
    Ok((delivered, f))
}

/// A decoder's process, killed the moment `cancel` fires. The readers below
/// block on its pipe, and an ffmpeg that has stopped answering (a hung GPU
/// driver) never sends the next byte: without this, a cancel is only seen
/// once something arrives, which may be never.
struct Watched {
    child: Arc<Mutex<Child>>,
    stop: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Watched {
    fn new(child: Child, cancel: &CancelToken) -> Self {
        let child = Arc::new(Mutex::new(child));
        let (stop, killed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let (c, s, k, cancel) = (child.clone(), stop.clone(), killed.clone(), cancel.clone());
        let thread = thread::spawn(move || {
            while !s.load(Ordering::Acquire) {
                if cancel.is_cancelled() {
                    k.store(true, Ordering::Release);
                    let _ = c.lock().unwrap_or_else(PoisonError::into_inner).kill();
                    return;
                }
                thread::park_timeout(Duration::from_millis(40));
            }
        });
        Self { child, stop, killed, thread: Some(thread) }
    }

    /// Stop watching.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }

    /// Whether the watch has killed the process.
    fn was_killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }

    fn kill(&self) {
        let _ = self.child.lock().unwrap_or_else(PoisonError::into_inner).kill();
    }

    /// Wait for the process to end, while the watch goes on: a process that
    /// closed its output and then hangs is still ended by a cancel. The lock
    /// is only held for a look, or the watch could not kill.
    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let status = loop {
            if let Some(status) = self.child.lock().unwrap_or_else(PoisonError::into_inner).try_wait()? {
                break status;
            }
            thread::sleep(Duration::from_millis(5));
        };
        self.stop();
        Ok(status)
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use as_core::Rational;

    use super::*;
    use crate::{Caps, HwDecode};

    fn info(width: u32, height: u32, sar: Rational, rotation: i32) -> MediaInfo {
        serde_json::from_value(serde_json::json!({
            "container": "mov", "durationSec": null, "startSec": 0.0, "sizeBytes": null, "bitRate": null,
            "video": {
                "index": 0, "codec": "h264", "profile": null, "pixFmt": "yuv420p", "bitDepth": 8,
                "width": width, "height": height, "sar": sar, "rFrameRate": Rational::new(24, 1),
                "avgFrameRate": null, "timeBase": Rational::new(1, 1000), "colorPrimaries": null,
                "colorTransfer": null, "colorSpace": null, "colorRange": null, "fieldOrder": null,
                "rotation": rotation, "hdr": "none"
            },
            "audio": [], "subtitles": []
        }))
        .unwrap()
    }

    #[test]
    fn the_input_names_the_gpu_only_for_a_gpu_decoder() {
        assert!(input_args(Decoder::Cpu).is_empty());
        assert_eq!(input_args(Decoder::Nvdec).join(" "), "-xerror -hwaccel cuda -hwaccel_output_format cuda");
        assert_eq!(
            input_args(Decoder::NvdecOn(1)).join(" "),
            "-xerror -hwaccel cuda -hwaccel_device 1 -hwaccel_output_format cuda"
        );
    }

    /// What ffmpeg was asked for when a decode went to `decoder`.
    fn asked_of_ffmpeg(decoder: Decoder) -> String {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::recording(dir.path());
        let info = info(64, 64, Rational::new(1, 1), 0);
        let pts = (0..30).map(|i| i * 1000).collect();
        let index = as_core::FrameIndex::from_pts(Rational::new(1, 24_000), Rational::new(24, 1), pts).unwrap();
        let path = dir.path().join("clip.mp4");
        let src = Source { path: &path, info: &info, index: &index };
        // The stand-in gives no frames: what it was asked is all this is for.
        let _ = decode_samples_with(&tools, src, 0, 3, (16, 16), decoder, &CancelToken::new(), |_, _| Ok(true));
        std::fs::read_to_string(dir.path().join("asked")).unwrap()
    }

    /// The device reaches ffmpeg: a decode that dropped it would run on
    /// CUDA's first card, and its frames wouldn't show it.
    #[test]
    fn a_decode_asks_ffmpeg_for_the_device_it_was_given() {
        let on_99 = asked_of_ffmpeg(Decoder::NvdecOn(99));
        // A Mac has no CUDA, so a decode asked of it runs on the processor.
        if cfg!(target_os = "macos") {
            assert!(!on_99.contains("-hwaccel"), "{on_99}");
            return;
        }
        assert!(on_99.contains("-hwaccel cuda -hwaccel_device 99 -hwaccel_output_format cuda"), "{on_99}");
        let any = asked_of_ffmpeg(Decoder::Nvdec);
        assert!(any.contains("-hwaccel cuda -hwaccel_output_format cuda"), "{any}");
        assert!(!any.contains("-hwaccel_device"), "{any}");
        let software = asked_of_ffmpeg(Decoder::Cpu);
        assert!(software.contains("-filter:v") && !software.contains("-hwaccel"), "{software}");
    }

    #[test]
    fn samples_are_upright_square_and_never_too_big() {
        let all = (u32::MAX, u32::MAX);
        let r = Rational::new;
        assert_eq!(sample_size(&info(1920, 1080, r(1, 1), 0), (1280, 720), 4), (1280, 720));
        assert_eq!(sample_size(&info(720, 576, r(64, 45), 0), all, 2), (1024, 576));
        assert_eq!(sample_size(&info(1920, 1080, r(1, 1), 90), all, 2), (1080, 1920));
        // Pixels 20 and 2147483647 times as wide as tall: scaled down to 16K.
        assert_eq!(sample_size(&info(1920, 1080, r(20, 1), 0), all, 2), (MAX_SIDE, 460));
        let crafted = info(1920, 1080, r(i64::from(i32::MAX), 1), 0);
        assert_eq!(sample_size(&crafted, all, 2), (MAX_SIDE, 2));
        assert_eq!(sample_size(&crafted, (1280, 720), 4), (1280, 4));
        // A stored shape that isn't positive: square pixels.
        assert_eq!(sample_size(&info(1920, 1080, r(0, 1), 0), all, 2), (1920, 1080));
    }

    #[test]
    fn the_gpu_decodes_only_h264_and_hevc() {
        let coded = |codec: &str| {
            let mut i = info(1920, 1080, Rational::new(1, 1), 0);
            i.video.codec = codec.into();
            nvdec_suits(&i)
        };
        assert!(coded("h264") && coded("hevc"));
        for codec in ["av1", "vp9", "vp8", "mpeg2video", "vc1"] {
            assert!(!coded(codec), "{codec} is decoded in software");
        }
        assert!(!nvdec_suits(&info(1920, 1080, Rational::new(1, 1), 90)), "rotated");
    }

    /// What the decoders below are asked to cancel: a source of 100 frames.
    fn hundred_frames() -> (MediaInfo, as_core::FrameIndex) {
        let pts: Vec<i64> = (0..100).map(|i| i * 3600).collect();
        let index = as_core::FrameIndex::from_pts(Rational::new(1, 90_000), Rational::new(25, 1), pts).unwrap();
        (info(64, 64, Rational::new(1, 1), 0), index)
    }

    /// Runs `decode` while a cancel arrives from another thread after 300 ms;
    /// returns how long it took to come back.
    fn cancelled_midway<T>(decode: impl FnOnce(&CancelToken) -> T) -> (T, Duration) {
        let cancel = CancelToken::new();
        let later = cancel.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            later.cancel();
        });
        let start = std::time::Instant::now();
        let out = decode(&cancel);
        let took = start.elapsed();
        canceller.join().unwrap();
        (out, took)
    }

    /// An ffmpeg that has stopped answering sends no frame, so the read for
    /// the next one never returns: the person's cancel must end it anyway.
    #[test]
    fn a_decoder_that_never_answers_is_ended_by_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::hung(dir.path()).with_caps(cuda());
        let (info, index) = hundred_frames();
        let path = dir.path().join("clip.mp4");
        let src = Source { path: &path, info: &info, index: &index };
        let (out, took) = cancelled_midway(|cancel| {
            decode_samples_with(&tools, src, 0, 1, (16, 16), Decoder::Nvdec, cancel, |_, _| Ok(true))
        });
        assert!(matches!(out, Err(ref e) if e.is_cancelled()), "{out:?}");
        assert!(took >= Duration::from_millis(300) && took < Duration::from_secs(15), "came back after {took:?}");

        let (out, took) =
            cancelled_midway(|cancel| decode_samples(&tools, src, 0, 1, (16, 16), cancel, |_, _| Ok(true)));
        assert!(matches!(out, Err(ref e) if e.is_cancelled()), "{out:?}");
        assert!(took < Duration::from_secs(15), "came back after {took:?}");
    }

    /// What the check of a machine with working CUDA finds.
    fn cuda() -> Caps {
        let decode = HwDecode { hwaccel: "cuda".into(), output_format: "cuda".into() };
        Caps { decode: Some(decode), ..Caps::default() }
    }

    /// Whether a decode that asks `decoder`, on a machine whose check found
    /// `caps`, starts the driver: a stand-in that hangs on `-hwaccel` notes
    /// that, and any other run ends the decode at once.
    fn starts_the_driver(caps: Caps, decoder: Decoder) -> bool {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::picky(dir.path(), "-hwaccel").with_caps(caps);
        let (info, index) = hundred_frames();
        assert!(nvdec_suits(&info));
        let path = dir.path().join("clip.mp4");
        let src = Source { path: &path, info: &info, index: &index };
        let (hangs, cancel) = (dir.path().join("hangs"), CancelToken::new());
        thread::scope(|s| {
            let decoding =
                s.spawn(|| decode_samples_with(&tools, src, 0, 1, (16, 16), decoder, &cancel, |_, _| Ok(true)));
            let start = std::time::Instant::now();
            while !decoding.is_finished() && !hangs.exists() {
                assert!(start.elapsed() < Duration::from_secs(120), "the decode neither ended nor hung");
                thread::sleep(Duration::from_millis(20));
            }
            cancel.cancel();
            drop(decoding.join().unwrap());
        });
        hangs.exists()
    }

    /// Where the check found the NVIDIA driver failing or hanging, the decode
    /// that asks for the GPU never starts it: a driver stuck there would
    /// hold the scan until the person cancels. Another device isn't CUDA.
    /// A card the person chose is held to the same check.
    #[test]
    fn the_gpu_decoder_starts_only_where_the_check_found_cuda() {
        for decoder in [Decoder::Nvdec, Decoder::NvdecOn(1)] {
            assert!(!starts_the_driver(Caps::default(), decoder));
            let elsewhere = HwDecode { hwaccel: "d3d11va".into(), output_format: "d3d11".into() };
            assert!(!starts_the_driver(Caps { decode: Some(elsewhere), ..Caps::default() }, decoder));
            assert!(starts_the_driver(cuda(), decoder));
        }
    }

    /// What a job says it decodes on is what it starts on: software where
    /// the check refused CUDA or the file doesn't suit NVDEC, and without
    /// the check when software was asked for.
    #[test]
    fn the_decoder_named_is_the_one_a_decode_starts() {
        let (info, _) = hundred_frames();
        let dir = tempfile::tempdir().unwrap();
        let (refused, works) =
            (Tools::hung(dir.path()).with_caps(Caps::default()), Tools::hung(dir.path()).with_caps(cuda()));
        for decoder in [Decoder::Nvdec, Decoder::NvdecOn(1)] {
            assert_eq!(decoder_for(&works, &info, decoder), decoder);
            assert_eq!(decoder_for(&refused, &info, decoder), Decoder::Cpu);
            let mut av1 = info.clone();
            av1.video.codec = "av1".into();
            assert_eq!(decoder_for(&works, &av1, decoder), Decoder::Cpu);
        }
        // A check that would hang is never reached.
        assert_eq!(decoder_for(&Tools::hung(dir.path()), &info, Decoder::Cpu), Decoder::Cpu);
    }

    /// A decode nobody cancels still runs to its end.
    #[test]
    fn the_watch_leaves_a_decoder_that_finishes_alone() {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "echo hello"]);
            c
        } else {
            let mut c = std::process::Command::new("echo");
            c.arg("hello");
            c
        };
        let cancel = CancelToken::new();
        let child = crate::proc::spawn(cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())).unwrap();
        let mut watched = Watched::new(child, &cancel);
        let status = watched.wait().unwrap();
        assert!(status.success());
        assert!(!watched.was_killed(), "nothing was killed");
    }

    /// The still decoder too: its ffmpeg answers the probe and then goes quiet.
    #[test]
    fn a_still_decode_that_never_answers_is_ended_by_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::picky(dir.path(), "-frames:v");
        let (out, took) = cancelled_midway(|cancel| decode_still(&tools, &dir.path().join("face.png"), cancel));
        assert!(matches!(out, Err(ref e) if e.is_cancelled()), "{out:?}");
        assert!(took >= Duration::from_millis(300) && took < Duration::from_secs(15), "came back after {took:?}");
    }

    /// A process that has sent everything and then stalls, waiting to end, is
    /// still under the watch.
    #[test]
    fn the_watch_goes_on_while_the_process_is_waited_for() {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "ping -n 60 127.0.0.1 >NUL"]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", "sleep 60"]);
            c
        };
        let child = crate::proc::spawn(cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())).unwrap();
        let (status, took) = cancelled_midway(|cancel| Watched::new(child, cancel).wait());
        assert!(!status.unwrap().success());
        assert!(took >= Duration::from_millis(300) && took < Duration::from_secs(15), "came back after {took:?}");
    }
}
