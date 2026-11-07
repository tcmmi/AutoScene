//! Export presets: what editors actually ask for.

use as_core::{Msg, msg};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Container {
    Mov,
    Mp4,
}

impl Container {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Mov => "mov",
            Self::Mp4 => "mp4",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProResProfile {
    Proxy,
    Lt,
    Standard,
    Hq,
    P4444,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DnxProfile {
    /// 8-bit 4:2:2.
    Hq,
    /// 10-bit 4:2:2.
    Hqx,
}

/// Keyframe spacing for long-GOP codecs. Editors scrub short GOPs far better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Gop {
    /// About one keyframe per second, no B-frames — smooth scrubbing in NLEs.
    Editing,
    /// Every frame a keyframe (largest files, perfect for Twixtor/AE).
    AllIntra,
    /// Encoder defaults (smallest files).
    Delivery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EncoderPref {
    /// Hardware encoder when available, else software.
    Auto,
    Software,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "codec", rename_all = "snake_case")]
pub enum VideoCodec {
    Prores {
        profile: ProResProfile,
    },
    Dnxhr {
        profile: DnxProfile,
    },
    /// `quality`: 0 (lossless-ish) … 51 (worst), CRF-like. 16 is visually lossless.
    H264 {
        quality: u8,
        gop: Gop,
        encoder: EncoderPref,
    },
    Hevc {
        quality: u8,
        gop: Gop,
        encoder: EncoderPref,
        ten_bit: bool,
    },
    /// Bit-Perfect: every frame exactly as the source decodes to, without
    /// loss (see `as_export::lossless`).
    Lossless,
}

impl VideoCodec {
    pub fn is_intra(&self) -> bool {
        match self {
            Self::Prores { .. } | Self::Dnxhr { .. } => true,
            Self::H264 { gop, .. } | Self::Hevc { gop, .. } => *gop == Gop::AllIntra,
            Self::Lossless => false,
        }
    }

    /// Codecs whose output stays 8-bit SDR need HDR sources tone mapped.
    pub fn supports_hdr(&self) -> bool {
        match self {
            Self::Prores { .. } => true,
            Self::Dnxhr { profile } => *profile == DnxProfile::Hqx,
            Self::Hevc { ten_bit, .. } => *ten_bit,
            Self::H264 { .. } => false,
            Self::Lossless => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "codec", rename_all = "snake_case")]
pub enum AudioCodec {
    None,
    /// 24-bit PCM (MOV only) — what NLEs prefer.
    Pcm24,
    Aac {
        kbps: u32,
    },
    /// The source's samples as they are: PCM at its rate, in its channels
    /// and its sample format.
    Lossless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HdrMode {
    /// Keep HDR when the codec can carry it, otherwise tone map to SDR.
    Auto,
    /// Always tone map to SDR BT.709 (for SDR-only edits).
    ToneMap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreset {
    pub id: String,
    /// Its name in English. An export keeps its preset as JSON, and an older
    /// AutoScene reads these two as text.
    pub name: String,
    pub description: String,
    /// The name and description for the UI: none in an export made before
    /// they had keys, which says `name` and `description` as they are.
    #[serde(default)]
    pub name_msg: Option<Msg>,
    #[serde(default)]
    pub description_msg: Option<Msg>,
    pub container: Container,
    pub video: VideoCodec,
    pub audio: AudioCodec,
    /// Cap output height (keeps aspect). `None` = source resolution.
    pub max_height: Option<u32>,
    pub hdr: HdrMode,
    /// Mix surround sound down to stereo (phone editors don't handle 5.1).
    #[serde(default)]
    pub stereo: bool,
    pub builtin: bool,
    /// Take the black bars the picture was mastered with off the clips.
    #[serde(default)]
    pub remove_bars: bool,
    /// Bring every clip to the same loudness (see [`crate::loudness`]).
    #[serde(default)]
    pub match_loudness: bool,
}

/// The AAC bitrate of `channels` of sound for a preset's `kbps`, which is its
/// figure for stereo: surround gets two thirds of stereo's share a channel
/// (5.1 at 320: 640, and 320 alone would leave each channel a third of what
/// stereo has), mono half.
pub(crate) fn aac_kbps(kbps: u32, channels: u32) -> u32 {
    match channels {
        0 | 2 => kbps,
        1 => kbps / 2,
        n => kbps.max(kbps * n / 3),
    }
}

/// A built-in preset of these words, its English also as plain text. The
/// rest is each preset's own.
fn worded(name: Msg, description: Msg) -> ExportPreset {
    ExportPreset {
        id: String::new(),
        name: name.text.clone(),
        description: description.text.clone(),
        name_msg: Some(name),
        description_msg: Some(description),
        container: Container::Mov,
        video: VideoCodec::Lossless,
        audio: AudioCodec::None,
        max_height: None,
        hdr: HdrMode::Auto,
        stereo: false,
        builtin: true,
        remove_bars: false,
        match_loudness: false,
    }
}

pub fn builtin_presets() -> Vec<ExportPreset> {
    vec![
        ExportPreset {
            id: "prores-hq".into(),
            container: Container::Mov,
            video: VideoCodec::Prores { profile: ProResProfile::Hq },
            audio: AudioCodec::Pcm24,
            max_height: None,
            hdr: HdrMode::Auto,
            stereo: false,
            builtin: true,
            ..worded(
                msg!("export.preset.proresHq.name", "ProRes 422 HQ"),
                msg!(
                    "export.preset.proresHq.description",
                    "Edit-grade intra codec for After Effects, Premiere, Final Cut and Resolve. Large files."
                ),
            )
        },
        ExportPreset {
            id: "dnxhr-hq".into(),
            container: Container::Mov,
            video: VideoCodec::Dnxhr { profile: DnxProfile::Hq },
            audio: AudioCodec::Pcm24,
            max_height: None,
            hdr: HdrMode::ToneMap,
            stereo: false,
            builtin: true,
            ..worded(
                msg!("export.preset.dnxhrHq.name", "DNxHR HQ"),
                msg!(
                    "export.preset.dnxhrHq.description",
                    "Edit-grade intra codec, popular with Premiere and Resolve on Windows."
                ),
            )
        },
        ExportPreset {
            id: "h264-edit".into(),
            container: Container::Mp4,
            video: VideoCodec::H264 { quality: 16, gop: Gop::Editing, encoder: EncoderPref::Auto },
            audio: AudioCodec::Aac { kbps: 320 },
            max_height: None,
            hdr: HdrMode::ToneMap,
            stereo: false,
            builtin: true,
            ..worded(
                msg!("export.preset.h264Edit.name", "H.264 · Editing"),
                msg!(
                    "export.preset.h264Edit.description",
                    "Visually lossless H.264 with a short GOP for smooth scrubbing. Works everywhere."
                ),
            )
        },
        ExportPreset {
            id: "h264-mobile".into(),
            container: Container::Mp4,
            video: VideoCodec::H264 { quality: 18, gop: Gop::Editing, encoder: EncoderPref::Auto },
            audio: AudioCodec::Aac { kbps: 256 },
            max_height: Some(1080),
            hdr: HdrMode::ToneMap,
            stereo: true,
            builtin: true,
            ..worded(
                msg!("export.preset.h264Mobile.name", "H.264 · Mobile (CapCut, Alight Motion)"),
                msg!(
                    "export.preset.h264Mobile.description",
                    "1080p H.264 with stereo sound that imports cleanly into phone editors."
                ),
            )
        },
        ExportPreset {
            id: "h264-intra".into(),
            container: Container::Mp4,
            video: VideoCodec::H264 { quality: 14, gop: Gop::AllIntra, encoder: EncoderPref::Auto },
            audio: AudioCodec::Aac { kbps: 320 },
            max_height: None,
            hdr: HdrMode::ToneMap,
            stereo: false,
            builtin: true,
            ..worded(
                msg!("export.preset.h264Intra.name", "H.264 · All-Intra (Twixtor)"),
                msg!(
                    "export.preset.h264Intra.description",
                    "Every frame a keyframe — ideal for Twixtor and frame-by-frame work."
                ),
            )
        },
        ExportPreset {
            id: "hevc10".into(),
            container: Container::Mp4,
            video: VideoCodec::Hevc { quality: 18, gop: Gop::Editing, encoder: EncoderPref::Auto, ten_bit: true },
            audio: AudioCodec::Aac { kbps: 320 },
            max_height: None,
            hdr: HdrMode::Auto,
            stereo: false,
            builtin: true,
            ..worded(
                msg!("export.preset.hevc10.name", "HEVC 10-bit (keeps HDR)"),
                msg!("export.preset.hevc10.description", "Smaller files that preserve 10-bit colour and HDR."),
            )
        },
        ExportPreset {
            id: "bit-perfect".into(),
            container: Container::Mov,
            video: VideoCodec::Lossless,
            audio: AudioCodec::Lossless,
            max_height: None,
            hdr: HdrMode::Auto,
            stereo: false,
            builtin: true,
            ..worded(
                msg!("export.preset.bitPerfect.name", "Bit-Perfect (lossless)"),
                msg!(
                    "export.preset.bitPerfect.description",
                    "Every frame exactly as in the episode: the same pixels, colour and HDR, and its own sound. Stored without loss as HEVC, which After Effects and other editors open (FFV1 for the rare pictures HEVC can't hold). Very large files."
                ),
            )
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_builtin_presets_words_have_keys_and_keep_their_english() {
        let presets = builtin_presets();
        let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "ProRes 422 HQ",
                "DNxHR HQ",
                "H.264 · Editing",
                "H.264 · Mobile (CapCut, Alight Motion)",
                "H.264 · All-Intra (Twixtor)",
                "HEVC 10-bit (keeps HDR)",
                "Bit-Perfect (lossless)",
            ]
        );
        for p in &presets {
            let (name, description) = (p.name_msg.as_ref().unwrap(), p.description_msg.as_ref().unwrap());
            assert!(name.key.starts_with("export.preset.") && name.key.ends_with(".name"), "{}", p.id);
            assert!(description.key.ends_with(".description"), "{}", p.id);
            assert_eq!((name.text.as_str(), description.text.as_str()), (p.name.as_str(), p.description.as_str()));
            assert!(!p.id.is_empty() && p.builtin);
        }
    }

    #[test]
    fn an_export_kept_before_presets_had_keys_still_reads() {
        let mut kept = serde_json::to_value(builtin_presets().remove(0)).unwrap();
        let row = kept.as_object_mut().unwrap();
        row.remove("nameMsg");
        row.remove("descriptionMsg");
        let read: ExportPreset = serde_json::from_value(kept).unwrap();
        assert_eq!((read.name.as_str(), read.name_msg), ("ProRes 422 HQ", None));
    }
}
