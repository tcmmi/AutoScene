//! What this machine's ffmpeg can actually do — verified by smoke tests, not
//! just by what the build lists (an `h264_nvenc` entry means nothing on an AMD
//! machine).
//!
//! Every check is a short ffmpeg run with a bounded wait. A hardware driver
//! can hang for ever (several NVIDIA decoders or encoders starting at once
//! under WSL do); a check that overruns is ended with everything it started,
//! logged, and counts as "not available", so AutoScene goes on without that
//! capability instead of stopping at the first export or scan. Checks of one
//! driver run one after another (the hang needs several at once); only
//! different drivers are checked side by side.
//!
//! The build's lists are another matter: they start no driver, so one that
//! overruns says something about a cold disk or a virus scanner, not about
//! what ffmpeg can do. It gets a second, longer try, and a list that stays
//! unknown takes hardware away but never software encoding, which an export
//! can't do without for the rest of the session.

use std::collections::HashSet;
use std::process::{Command, Output};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Tools;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum CodecFamily {
    H264,
    Hevc,
    Av1,
    Prores,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HwEncoder {
    pub name: String,
    pub family: CodecFamily,
    pub vendor: Vendor,
}

/// Hardware decode setup for analysis/index passes.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HwDecode {
    pub hwaccel: String,
    /// Keep frames on the GPU when nothing downstream needs them in RAM.
    pub output_format: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    pub decode: Option<HwDecode>,
    pub hw_encoders: Vec<HwEncoder>,
    /// Software encoders present in this build (libx264, libx265, prores_ks, …).
    pub sw_encoders: Vec<String>,
    /// Pixel formats `hevc_nvenc` encodes without loss on this card
    /// (`-tune lossless`; older cards lack it, or 10 bits, or 4:4:4).
    #[serde(default)]
    pub nvenc_lossless: Vec<String>,
    /// HEVC hardware encoders that work, but not in 10 bits (older cards).
    #[serde(default)]
    pub no_ten_bit: Vec<String>,
}

const HW_ENCODERS: &[(&str, CodecFamily, Vendor)] = &[
    ("h264_nvenc", CodecFamily::H264, Vendor::Nvidia),
    ("hevc_nvenc", CodecFamily::Hevc, Vendor::Nvidia),
    ("av1_nvenc", CodecFamily::Av1, Vendor::Nvidia),
    ("h264_amf", CodecFamily::H264, Vendor::Amd),
    ("hevc_amf", CodecFamily::Hevc, Vendor::Amd),
    ("av1_amf", CodecFamily::Av1, Vendor::Amd),
    ("h264_qsv", CodecFamily::H264, Vendor::Intel),
    ("hevc_qsv", CodecFamily::Hevc, Vendor::Intel),
    ("av1_qsv", CodecFamily::Av1, Vendor::Intel),
    ("h264_videotoolbox", CodecFamily::H264, Vendor::Apple),
    ("hevc_videotoolbox", CodecFamily::Hevc, Vendor::Apple),
    ("prores_videotoolbox", CodecFamily::Prores, Vendor::Apple),
];

const SW_ENCODERS: &[&str] = &[
    "libx264",
    "libx265",
    "libsvtav1",
    "prores_ks",
    "dnxhd",
    "ffv1",
    "aac",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_s32le",
    "pcm_f32le",
    "pcm_f64le",
    "pcm_u8",
];

/// Decode accelerators in preference order: (hwaccel, device type, GPU-resident format).
const DECODERS: &[(&str, &str, &str)] = if cfg!(target_os = "macos") {
    &[("videotoolbox", "videotoolbox", "videotoolbox_vld")]
} else if cfg!(windows) {
    &[("cuda", "cuda", "cuda"), ("d3d11va", "d3d11va", "d3d11")]
} else {
    &[("cuda", "cuda", "cuda"), ("vaapi", "vaapi", "vaapi")]
};

/// How long a check may take before it counts as failed. A working one takes
/// about a second; a cold GPU that has to wake first, a few more.
#[derive(Clone, Copy, Debug)]
struct Waits {
    /// `-encoders` and `-hwaccels`: the build's lists, no hardware involved.
    list: Duration,
    /// The second try of a list that overran, with ffmpeg in the disk's cache
    /// by then.
    list_again: Duration,
    /// A run that starts a hardware encoder or decoder.
    smoke: Duration,
}

const WAITS: Waits =
    Waits { list: Duration::from_secs(10), list_again: Duration::from_secs(30), smoke: Duration::from_secs(20) };

/// The checks of one detection. After a check hung, the others that would
/// start the same driver aren't tried: they would only hang too, and each
/// would cost a wait of its own. A driver is a vendor's (the NVIDIA one serves
/// NVENC and NVDEC alike) or a decoder's own; another driver's checks are
/// unaffected.
struct Checks<'a> {
    tools: &'a Tools,
    waits: Waits,
    hung: Mutex<HashSet<&'static str>>,
}

/// The driver a vendor's encoders start.
fn driver(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::Nvidia => "nvidia",
        Vendor::Amd => "amd",
        Vendor::Intel => "intel",
        Vendor::Apple => "apple",
    }
}

/// The driver a decoder's device starts (CUDA is NVIDIA's, as NVENC is).
fn device_driver(device: &'static str) -> &'static str {
    match device {
        "cuda" => driver(Vendor::Nvidia),
        "videotoolbox" => driver(Vendor::Apple),
        other => other,
    }
}

impl Caps {
    pub fn detect(tools: &Tools) -> Self {
        Self::detect_within(tools, WAITS)
    }

    fn detect_within(tools: &Tools, waits: Waits) -> Self {
        let checks = &Checks { tools, waits, hung: Mutex::default() };
        let listed = |flag: &str| -> Option<String> {
            let list = |limit| checks.run(tools.ffmpeg().arg(flag), flag, limit, None);
            let out = list(waits.list).or_else(|| list(waits.list_again));
            out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        };
        let enc_list = listed("-encoders");
        let present: HashSet<&str> =
            enc_list.iter().flat_map(|list| list.lines()).filter_map(|l| l.split_whitespace().nth(1)).collect();
        let hwaccels = listed("-hwaccels").unwrap_or_default();

        let candidates: Vec<_> = HW_ENCODERS.iter().filter(|(n, ..)| present.contains(n)).collect();
        // One driver's encoders one after another: it is several NVIDIA
        // encoders starting at once that hangs. Drivers are independent.
        let hw_encoders: Vec<HwEncoder> = thread::scope(|s| {
            let handles: Vec<_> = [Vendor::Nvidia, Vendor::Amd, Vendor::Intel, Vendor::Apple]
                .into_iter()
                .map(|v| {
                    let mine: Vec<_> = candidates.iter().filter(|c| c.2 == v).copied().collect();
                    s.spawn(move || {
                        mine.into_iter()
                            .filter(|&&(name, _, vendor)| checks.smoke_encode(name, vendor))
                            .map(|&(name, family, vendor)| HwEncoder { name: name.into(), family, vendor })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });

        let decode = DECODERS
            .iter()
            .filter(|(accel, ..)| hwaccels.lines().any(|l| l.trim() == *accel))
            .find(|(_, device, _)| checks.smoke_device(device))
            .map(|(accel, _, fmt)| HwDecode { hwaccel: (*accel).into(), output_format: (*fmt).into() });

        // Not knowing is no "no": every build AutoScene takes has them, and an
        // export of one that hasn't says so itself.
        let sw_encoders = SW_ENCODERS
            .iter()
            .filter(|n| enc_list.is_none() || present.contains(*n))
            .map(|n| (*n).to_string())
            .collect();
        let nvenc_lossless = if hw_encoders.iter().any(|e| e.name == "hevc_nvenc") {
            NVENC_LOSSLESS.iter().filter(|&&fmt| checks.smoke_lossless(fmt)).map(|&fmt| fmt.to_string()).collect()
        } else {
            Vec::new()
        };
        let no_ten_bit = hw_encoders
            .iter()
            .filter(|e| e.family == CodecFamily::Hevc && !checks.smoke_ten_bit(&e.name, e.vendor))
            .map(|e| e.name.clone())
            .collect();
        let caps = Self { decode, hw_encoders, sw_encoders, nvenc_lossless, no_ten_bit };
        tracing::info!(?caps, "media capabilities");
        caps
    }

    pub fn has_sw(&self, name: &str) -> bool {
        self.sw_encoders.iter().any(|e| e == name)
    }

    /// Best working hardware encoder for a codec family (NVIDIA first: fastest, best quality).
    pub fn hw_encoder(&self, family: CodecFamily) -> Option<&HwEncoder> {
        self.hw_encoders.iter().find(|e| e.family == family)
    }

    /// [`Caps::hw_encoder`], for pictures of 10 bits when `ten_bit` says so.
    pub fn hw_encoder_for(&self, family: CodecFamily, ten_bit: bool) -> Option<&HwEncoder> {
        self.hw_encoders.iter().find(|e| e.family == family && !(ten_bit && self.no_ten_bit.contains(&e.name)))
    }
}

/// The layouts lossless HEVC NVENC is asked to take (as `as_export::lossless`
/// hands frames to it).
const NVENC_LOSSLESS: &[&str] = &["yuv420p", "p010le", "yuv444p", "yuv444p16le"];

impl Checks<'_> {
    /// Run `cmd`, the check `what`, for at most `limit`. `None` when it didn't
    /// end in time or couldn't start: the check then counts as failed. One
    /// that overran was ended with everything it started; when it starts a
    /// hardware `driver`, the later checks of that driver stand down.
    fn run(&self, cmd: &mut Command, what: &str, limit: Duration, driver: Option<&'static str>) -> Option<Output> {
        let hung = || self.hung.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(driver) = driver.filter(|d| hung().contains(d)) {
            tracing::warn!(
                check = what,
                driver,
                "not tried: an earlier check of this driver hung; counting it as not available"
            );
            return None;
        }
        match crate::proc::output_within(cmd, limit) {
            Ok(Some(out)) => Some(out),
            Ok(None) => {
                tracing::warn!(
                    check = what,
                    waited_secs = limit.as_secs_f64(),
                    "a media check didn't end in time and was stopped; counting it as not available"
                );
                if let Some(driver) = driver {
                    hung().insert(driver);
                }
                None
            }
            Err(e) => {
                tracing::debug!(error = %e, check = what, "a media check couldn't run");
                None
            }
        }
    }

    fn smoke_encode(&self, encoder: &str, vendor: Vendor) -> bool {
        let mut cmd = self.tools.ffmpeg();
        cmd.args(["-v", "error", "-f", "lavfi", "-i", "color=c=gray:s=320x240:r=24", "-frames:v", "3"])
            .args(["-pix_fmt", if encoder.contains("videotoolbox") { "nv12" } else { "yuv420p" }])
            .args(["-c:v", encoder, "-f", "null", "-"]);
        self.run(&mut cmd, encoder, self.waits.smoke, Some(driver(vendor))).is_some_and(|o| o.status.success())
    }

    fn smoke_ten_bit(&self, encoder: &str, vendor: Vendor) -> bool {
        let mut cmd = self.tools.ffmpeg();
        cmd.args(["-v", "error", "-f", "lavfi", "-i", "color=c=gray:s=320x240:r=24", "-frames:v", "3"])
            .args(["-pix_fmt", "p010le", "-c:v", encoder, "-f", "null", "-"]);
        self.run(&mut cmd, encoder, self.waits.smoke, Some(driver(vendor))).is_some_and(|o| o.status.success())
    }

    fn smoke_lossless(&self, pix_fmt: &str) -> bool {
        let mut cmd = self.tools.ffmpeg();
        cmd.args(["-v", "error", "-f", "lavfi", "-i", "color=c=gray:s=320x240:r=24", "-frames:v", "3"]).args([
            "-pix_fmt",
            pix_fmt,
            "-c:v",
            "hevc_nvenc",
            "-preset",
            "p4",
            "-tune",
            "lossless",
            "-f",
            "null",
            "-",
        ]);
        self.run(&mut cmd, "hevc_nvenc lossless", self.waits.smoke, Some(driver(Vendor::Nvidia)))
            .is_some_and(|o| o.status.success())
    }

    fn smoke_device(&self, device: &'static str) -> bool {
        let mut cmd = self.tools.ffmpeg();
        cmd.args(["-v", "error", "-init_hw_device", &format!("{device}=hw"), "-f", "lavfi", "-i", "nullsrc=s=64x64"])
            .args(["-frames:v", "1", "-f", "null", "-"]);
        self.run(&mut cmd, device, self.waits.smoke, Some(device_driver(device))).is_some_and(|o| o.status.success())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// The limits detection runs with: the lists are quick, a driver gets
    /// longer, and no check is without a bound.
    #[test]
    fn the_shipped_limits_are_the_ones_the_checks_use() {
        const { assert!(WAITS.list.as_secs() >= 5 && WAITS.list.as_secs() <= 15) };
        const { assert!(WAITS.smoke.as_secs() >= 15 && WAITS.smoke.as_secs() <= 30) };
        const { assert!(WAITS.list.as_millis() < WAITS.smoke.as_millis()) };
        const { assert!(WAITS.list.as_millis() < WAITS.list_again.as_millis()) };
    }

    /// Lists and driver runs have limits of their own: with an ffmpeg that
    /// hangs on everything, each list ends after its two tries (a `smoke` of
    /// half a minute would show). No hardware is available, and software
    /// encoding stays: exports would otherwise fail until a restart.
    #[test]
    fn detection_with_an_ffmpeg_that_hangs_leaves_software_encoding_only() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::hung(dir.path());
        let waits = Waits {
            list: Duration::from_millis(500),
            list_again: Duration::from_millis(700),
            smoke: Duration::from_secs(30),
        };
        let start = Instant::now();
        let caps = Caps::detect_within(&tools, waits);
        let took = start.elapsed();
        assert!(caps.decode.is_none() && caps.hw_encoders.is_empty(), "{caps:?}");
        assert!(caps.nvenc_lossless.is_empty());
        assert_eq!(caps.sw_encoders, SW_ENCODERS);
        assert!(caps.has_sw("libx264"));
        // Two lists, two tries each; far from the 60 s the stand-in sleeps.
        assert!(took >= Duration::from_millis(2400), "{took:?}");
        assert!(took < Duration::from_secs(12), "the lists waited {took:?}");
    }

    /// A list that overran once, as on a cold disk, says what the build has
    /// the second time: its hardware isn't lost for the session.
    #[test]
    fn a_list_that_overran_once_is_read_the_second_time() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::cold(dir.path());
        let waits = Waits {
            list: Duration::from_secs(10),
            list_again: Duration::from_secs(30),
            smoke: Duration::from_secs(30),
        };
        let caps = Caps::detect_within(&tools, waits);
        let names: Vec<_> = caps.hw_encoders.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["h264_nvenc", "hevc_nvenc", "h264_amf"], "{caps:?}");
        // The list was read, so what it leaves out isn't assumed.
        assert!(caps.sw_encoders.is_empty(), "{caps:?}");
    }

    /// The lists come at once and every driver run hangs: each driver
    /// (NVIDIA's NVENC and CUDA together, AMD's) is asked once, and the rest
    /// of its checks are not tried.
    #[test]
    fn a_driver_that_hangs_costs_one_wait_and_leaves_nothing_available() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::picky(dir.path(), "-frames:v");
        let waits =
            Waits { list: Duration::from_secs(30), list_again: Duration::from_secs(30), smoke: Duration::from_secs(4) };
        let caps = Caps::detect_within(&tools, waits);
        assert!(caps.decode.is_none() && caps.hw_encoders.is_empty() && caps.nvenc_lossless.is_empty(), "{caps:?}");
        // h264_nvenc and h264_amf; hevc_nvenc, CUDA and the lossless layouts
        // were spared.
        let hangs = std::fs::read_to_string(dir.path().join("hangs")).unwrap();
        assert_eq!(hangs.lines().count(), 2, "a driver that hung was tried again: {hangs:?}");
    }

    /// One driver hanging does not take the others' hardware with it.
    #[test]
    fn a_hung_amd_check_leaves_nvidia_encoding_and_decoding_available() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::picky(dir.path(), "h264_amf");
        let waits =
            Waits { list: Duration::from_secs(30), list_again: Duration::from_secs(30), smoke: Duration::from_secs(3) };
        let caps = Caps::detect_within(&tools, waits);
        let names: Vec<_> = caps.hw_encoders.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["h264_nvenc", "hevc_nvenc"], "{caps:?}");
        // A Mac is never asked for CUDA: its decoder is VideoToolbox's.
        let cuda = (!cfg!(target_os = "macos")).then_some("cuda");
        assert_eq!(caps.decode.as_ref().map(|d| d.hwaccel.as_str()), cuda);
        assert_eq!(caps.nvenc_lossless, NVENC_LOSSLESS);
    }

    /// A driver's checks run one after another, never side by side: the
    /// stand-in takes a lock for every run and notes when it finds it taken.
    #[cfg(unix)]
    #[test]
    fn one_driver_is_never_asked_twice_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let (lock, overlaps, runs) = (dir.path().join("lock"), dir.path().join("overlaps"), dir.path().join("runs"));
        let script = dir.path().join("ffmpeg");
        let lines = [
            "#!/bin/sh".to_string(),
            r#"case "$*" in"#.to_string(),
            r#"*-encoders*) printf ' V....D h264_nvenc x\n V....D hevc_nvenc x\n V....D av1_nvenc x\n'; exit 0;;"#
                .to_string(),
            r#"*-hwaccels*) printf 'Hardware acceleration methods:\ncuda\n'; exit 0;;"#.to_string(),
            "esac".to_string(),
            format!("echo run >> '{}'", runs.display()),
            format!("mkdir '{}' 2>/dev/null || echo overlap >> '{}'", lock.display(), overlaps.display()),
            "sleep 0.2".to_string(),
            format!("rmdir '{}' 2>/dev/null", lock.display()),
            "exit 0".to_string(),
            String::new(),
        ];
        crate::tools::write_script(&script, &lines.join("\n"));
        let tools = Tools::with_ffmpeg(script);
        let caps = Caps::detect_within(&tools, WAITS);
        assert_eq!(caps.hw_encoders.len(), 3, "{caps:?}");
        assert_eq!(caps.nvenc_lossless.len(), NVENC_LOSSLESS.len());
        // Three encoders, HEVC's 10 bits, the decoder (not on a Mac, where
        // this ffmpeg's CUDA isn't asked for) and the four lossless layouts.
        let checks = if cfg!(target_os = "macos") { 8 } else { 9 };
        assert_eq!(std::fs::read_to_string(&runs).unwrap().lines().count(), checks);
        assert!(!overlaps.exists(), "two NVIDIA checks ran at once");
    }

    /// One hung check spares the same driver's others their waits, and only those.
    #[test]
    fn after_a_check_hung_the_same_drivers_others_are_not_tried() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::hung(dir.path());
        let waits = Waits {
            list: Duration::from_millis(500),
            list_again: Duration::from_millis(500),
            smoke: Duration::from_millis(600),
        };
        let checks = Checks { tools: &tools, waits, hung: Mutex::default() };
        let start = Instant::now();
        assert!(!checks.smoke_device("cuda"));
        assert!(start.elapsed() >= Duration::from_millis(600));
        assert!(checks.hung.lock().unwrap().contains("nvidia"));
        let again = Instant::now();
        assert!(!checks.smoke_encode("hevc_nvenc", Vendor::Nvidia));
        assert!(!checks.smoke_lossless("yuv420p"));
        assert!(
            again.elapsed() < Duration::from_millis(300),
            "waited {:?} for a driver known to hang",
            again.elapsed()
        );
        // Another driver's check is still made (and hangs, here).
        let other = Instant::now();
        assert!(!checks.smoke_encode("h264_amf", Vendor::Amd));
        assert!(other.elapsed() >= Duration::from_millis(600), "AMD was not tried");
    }
}
