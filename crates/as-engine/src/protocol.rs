//! Engine wire protocol: one JSON object per line over the engine's stdin
//! (requests) and stdout (replies). Binary payloads travel as base64.

use std::path::PathBuf;

use as_core::Msg;
use as_infer::accel::AccelState;
use as_infer::hardware::Hardware;
use as_media::MediaInfo;
use as_media::frames::Decoder;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes that serialise as a base64 string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Blob(pub Vec<u8>);

impl Serialize for Blob {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map(Blob).map_err(serde::de::Error::custom)
    }
}

/// App → engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Measure the shot signals of a media from `resume` on.
    Shots(Box<ShotsJob>),
    /// Find, embed and track the faces of a media's shots from `resume` on.
    Faces(Box<FacesJob>),
    /// Embed a still image (a character portrait, say) with the anime
    /// embedder (`as_vision::ccip`) for matching against grouped
    /// characters; `Reply::Embedding` then `Done`.
    Embed(Box<EmbedJob>),
    /// Read faces again at the source's own resolution; `Reply::FaceRead`
    /// for each, in no order, then `Done`.
    Rescan(Box<RescanJob>),
    /// Stop job `id` as soon as possible (it then fails with `cancelled`).
    Cancel { id: u64 },
    /// Finish the current job's cancellation and exit.
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShotsJob {
    pub id: u64,
    pub path: PathBuf,
    pub info: MediaInfo,
    /// `FrameIndex::to_bytes`.
    pub index: Blob,
    /// First frame to measure; a multiple of `chunk`.
    pub resume: u32,
    /// Frames per checkpoint chunk; a multiple of TransNet's step.
    pub chunk: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FacesJob {
    pub id: u64,
    pub path: PathBuf,
    pub info: MediaInfo,
    /// `FrameIndex::to_bytes`.
    pub index: Blob,
    /// Shot ranges `(start, end)`, sorted; faces are looked for only there.
    pub shots: Vec<(u32, u32)>,
    /// First frame to look at: 0 or the end of a chunk handed out earlier.
    pub resume: u32,
    /// Frames per checkpoint chunk (chunks end at the next shot start).
    pub chunk: u32,
    /// Look for people too, tied to the faces (`FaceChunk::bodies`).
    #[serde(default)]
    pub bodies: bool,
    /// Faces found before (`as_analysis::tracks::encode_known`): only people
    /// are looked for, and tied to these.
    #[serde(default)]
    pub known: Option<Blob>,
    /// 3D animation: faces are searched as `FaceParams::animated` does.
    #[serde(default)]
    pub animated: bool,
    /// Fast Scan: faces are found as `as_analysis::fast` does (a few frames
    /// per shot) with YuNet and SFace, or on anime (`anime`) with its own
    /// models; nobody is looked for by their body.
    #[serde(default)]
    pub fast: bool,
    /// Anime footage: heads are found and embedded as `as_analysis::anime`
    /// does (anime head/person detectors, CCIP) — with `fast`, in the frames
    /// Fast Scan picks; `bodies`, `known` and `animated` don't apply.
    #[serde(default)]
    pub anime: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedJob {
    pub id: u64,
    /// The still image file to embed (any format ffmpeg reads).
    pub path: PathBuf,
}

/// A face to read again.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RescanFaceIn {
    pub id: u32,
    pub shot: (u32, u32),
    pub quality: f32,
    /// `as_analysis::tracks::dets_to_bytes`.
    pub dets: Blob,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RescanJob {
    pub id: u64,
    pub path: PathBuf,
    pub info: MediaInfo,
    /// `FrameIndex::to_bytes`.
    pub index: Blob,
    pub faces: Vec<RescanFaceIn>,
    /// The faces were found by Fast Scan (YuNet, SFace) ...
    #[serde(default)]
    pub fast: bool,
    /// ... or are anime heads (CCIP): read by what found them.
    #[serde(default)]
    pub anime: bool,
    /// Ask the other face model about the same crops: SFace for faces of the
    /// full pass, ArcFace for Fast Scan's. Anime has no other.
    #[serde(default)]
    pub second: bool,
}

/// A shot's sprite (`as_analysis::ShotSprite`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sprite {
    pub shot: u32,
    pub end: u32,
    pub cells: u32,
    pub jpeg: Blob,
}

impl Request {
    pub fn id(&self) -> Option<u64> {
        match self {
            Self::Shots(j) => Some(j.id),
            Self::Faces(j) => Some(j.id),
            Self::Embed(j) => Some(j.id),
            Self::Rescan(j) => Some(j.id),
            Self::Cancel { .. } | Self::Shutdown => None,
        }
    }
}

/// A model a job uses and the device it runs on (`as_infer::Device::label`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelDevice {
    pub model: String,
    pub device: String,
}

/// Engine → app.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Reply {
    /// The runtime is loaded; models run on `device` where they can (each
    /// falls back to the CPU on its own, which every job reports: `Devices`).
    /// `hardware` names the cards found and the one opened; `accel` says
    /// what became of NVIDIA's engine (`--accel`).
    Ready {
        device: String,
        #[serde(default)]
        hardware: Hardware,
        #[serde(default)]
        accel: AccelState,
    },
    /// Where job `id`'s models run, once they are loaded (before its first
    /// progress): on `preferred` each one it can take, on the CPU the rest.
    Devices {
        id: u64,
        preferred: String,
        models: Vec<ModelDevice>,
    },
    /// Where job `id` starts decoding, for a job that has a choice (Fast
    /// Scan), said before its models load: software, whatever the choice,
    /// where the file or this machine's check rule NVDEC out
    /// (`frames::decoder_for`). A decode that fails on the GPU part way goes
    /// on in software without another word.
    Decoding {
        id: u64,
        decoder: Decoder,
    },
    Progress {
        id: u64,
        fraction: f64,
    },
    /// Finished signals of frames `start..start + len` (`as_analysis::signals::encode`).
    ShotChunk {
        id: u64,
        start: u32,
        len: u32,
        signals: Blob,
    },
    /// Face tracks of frames `start..end` (`as_analysis::tracks::encode`),
    /// the sprites of its shots, and the people found there
    /// (`as_analysis::bodies::encode`; empty when none were looked for).
    FaceChunk {
        id: u64,
        start: u32,
        end: u32,
        tracks: Blob,
        #[serde(default)]
        sprites: Vec<Sprite>,
        #[serde(default)]
        bodies: Blob,
    },
    /// The vector an `Embed` job got from the still, unit-normalised (768
    /// floats); `Done` follows it.
    Embedding {
        id: u64,
        embedding: Vec<f32>,
    },
    /// A face read again (`Rescan`): `embedding` is little-endian f16
    /// (`as_analysis::tracks::embedding_to_bytes`), empty when no look read it.
    FaceRead {
        id: u64,
        track: u32,
        embedding: Blob,
        quality: f32,
        tried: u32,
        found: u32,
    },
    Done {
        id: u64,
    },
    /// A model of job `id` failed on NVIDIA's engine (`Failed` follows): the
    /// app counts it against the pack and runs the job again without it.
    AccelFailed {
        id: u64,
        detail: String,
    },
    Failed {
        id: u64,
        /// The English, for a reply that has no `msg`.
        message: String,
        /// The same words for the UI.
        #[serde(default)]
        msg: Option<Msg>,
        detail: String,
        cancelled: bool,
    },
    /// The engine can't work at all (runtime or ffmpeg missing) and exits.
    Fatal {
        message: String,
        #[serde(default)]
        msg: Option<Msg>,
        detail: String,
    },
}

impl Reply {
    /// A job's failure, in both forms.
    pub(crate) fn failed(id: u64, said: Msg, detail: String, cancelled: bool) -> Self {
        Self::Failed { id, message: said.text.clone(), msg: Some(said), detail, cancelled }
    }

    /// The engine's end, in both forms.
    pub(crate) fn fatal(said: Msg, detail: String) -> Self {
        Self::Fatal { message: said.text.clone(), msg: Some(said), detail }
    }
}

/// What a failure's reply says: its `msg`, or its English as plain words
/// when it has none.
pub(crate) fn said(message: String, msg: Option<Msg>) -> Msg {
    msg.unwrap_or_else(|| Msg::plain(message))
}

#[cfg(test)]
pub(crate) mod tests {
    use as_core::msg;

    use super::*;

    #[test]
    fn a_failure_reply_reads_with_and_without_its_key() {
        let reply = Reply::failed(5, msg!("infer.model", "An AI model couldn't be loaded."), "no file".into(), false);
        let line = serde_json::to_string(&reply).unwrap();
        let Reply::Failed { message, msg, .. } = serde_json::from_str(&line).unwrap() else { panic!("{line}") };
        assert_eq!(said(message, msg).key, "infer.model");
        // A reply of an engine from before keys.
        let old = r#"{"event":"failed","id":5,"message":"It failed.","detail":"x","cancelled":false}"#;
        let Reply::Failed { message, msg, .. } = serde_json::from_str(old).unwrap() else { panic!("{old}") };
        assert_eq!(said(message, msg), Msg::plain("It failed."));
        let old = r#"{"event":"fatal","message":"It can't work.","detail":"x"}"#;
        let Reply::Fatal { message, msg, .. } = serde_json::from_str(old).unwrap() else { panic!("{old}") };
        assert_eq!(said(message, msg), Msg::plain("It can't work."));
    }

    #[test]
    fn blobs_travel_as_base64() {
        let r = Reply::ShotChunk { id: 7, start: 2400, len: 2400, signals: Blob(vec![0, 1, 2, 255]) };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"event":"shot_chunk","id":7,"start":2400,"len":2400,"signals":"AAEC/w=="}"#);
        match serde_json::from_str::<Reply>(&line).unwrap() {
            Reply::ShotChunk { signals, .. } => assert_eq!(signals.0, [0, 1, 2, 255]),
            other => panic!("{other:?}"),
        }
        let c: Request = serde_json::from_str(r#"{"op":"cancel","id":3}"#).unwrap();
        assert!(matches!(c, Request::Cancel { id: 3 }));
    }

    #[test]
    fn ready_carries_the_hardware() {
        use as_infer::computer::Choice;
        use as_infer::hardware::{Kind, RawDevice, Using};
        let card = RawDevice {
            kind: Kind::Gpu,
            vendor: 0x10DE,
            device: 0x24C9,
            metadata: [("Description", "NVIDIA GeForce RTX 3060 Ti"), ("DxgiAdapterNumber", "0"), ("LUID", "67624")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .into(),
        };
        let hardware = Hardware::windows(&[card], Some(&[67624])).chosen(&Choice::Auto);
        let failed = AccelState::Failed { detail: "loading tensorrt_rtx_1_7.dll: not found".into() };
        let r = Reply::Ready { device: "GPU (DirectML)".into(), hardware: hardware.clone(), accel: failed.clone() };
        match serde_json::from_str::<Reply>(&serde_json::to_string(&r).unwrap()).unwrap() {
            Reply::Ready { hardware: back, accel, .. } => {
                assert_eq!((back.clone(), accel), (hardware, failed));
                assert_eq!((back.using, back.luid()), (Using::Gpu { index: 0 }, Some(67624)));
            }
            other => panic!("{other:?}"),
        }
        // A line from an engine that named no hardware, or knows no NVIDIA
        // engine, still reads.
        match serde_json::from_str::<Reply>(r#"{"event":"ready","device":"CPU"}"#).unwrap() {
            Reply::Ready { device, hardware, accel } => {
                assert_eq!((device.as_str(), hardware, accel), ("CPU", Hardware::default(), AccelState::Off))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nvidias_engine_reports_how_it_fared() {
        for (state, line) in [
            (AccelState::Off, r#"{"state":"off"}"#),
            (AccelState::Active, r#"{"state":"active"}"#),
            (AccelState::Unsupported, r#"{"state":"unsupported"}"#),
            (AccelState::Failed { detail: "x".into() }, r#"{"state":"failed","detail":"x"}"#),
        ] {
            assert_eq!(serde_json::to_string(&state).unwrap(), line);
            assert_eq!(serde_json::from_str::<AccelState>(line).unwrap(), state);
        }
        let r = Reply::AccelFailed { id: 5, detail: "inference: CUDA error".into() };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"event":"accel_failed","id":5,"detail":"inference: CUDA error"}"#);
        assert!(matches!(serde_json::from_str::<Reply>(&line).unwrap(), Reply::AccelFailed { id: 5, .. }));
    }

    #[test]
    fn devices_travel_by_model() {
        let r = Reply::Devices {
            id: 2,
            preferred: "GPU (DirectML)".into(),
            models: vec![ModelDevice { model: "transnetv2".into(), device: "CPU".into() }],
        };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(
            line,
            r#"{"event":"devices","id":2,"preferred":"GPU (DirectML)","models":[{"model":"transnetv2","device":"CPU"}]}"#
        );
        assert!(
            matches!(serde_json::from_str::<Reply>(&line).unwrap(), Reply::Devices { models, .. } if models.len() == 1)
        );
    }

    #[test]
    fn a_decoder_travels_with_its_device() {
        let r = Reply::Decoding { id: 3, decoder: Decoder::NvdecOn(1) };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"event":"decoding","id":3,"decoder":{"nvdec_on":1}}"#);
        assert!(matches!(
            serde_json::from_str::<Reply>(&line).unwrap(),
            Reply::Decoding { id: 3, decoder: Decoder::NvdecOn(1) }
        ));
    }

    #[test]
    fn people_are_optional_on_the_wire() {
        // A chunk from an engine that looks for no one.
        let chunk: Reply =
            serde_json::from_str(r#"{"event":"face_chunk","id":1,"start":0,"end":9,"tracks":"AQ=="}"#).unwrap();
        assert!(
            matches!(chunk, Reply::FaceChunk { ref bodies, ref sprites, .. } if bodies.0.is_empty() && sprites.is_empty())
        );
        let r = Reply::FaceChunk {
            id: 1,
            start: 0,
            end: 9,
            tracks: Blob(vec![1]),
            sprites: vec![],
            bodies: Blob(vec![1, 0]),
        };
        let back: Reply = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert!(matches!(back, Reply::FaceChunk { bodies, .. } if bodies.0 == [1, 0]));
    }

    pub(crate) fn info() -> MediaInfo {
        serde_json::from_value(serde_json::json!({
            "container": "matroska,webm", "durationSec": null, "startSec": 0.0, "sizeBytes": null, "bitRate": null,
            "video": {
                "index": 0, "codec": "h264", "profile": null, "pixFmt": "yuv420p", "bitDepth": 8,
                "width": 1920, "height": 1080, "sar": as_core::Rational::new(1, 1),
                "rFrameRate": as_core::Rational::new(24, 1), "avgFrameRate": null,
                "timeBase": as_core::Rational::new(1, 1000), "colorPrimaries": null,
                "colorTransfer": null, "colorSpace": null, "colorRange": null, "fieldOrder": null,
                "rotation": 0, "hdr": "none"
            },
            "audio": [], "subtitles": []
        }))
        .unwrap()
    }

    #[test]
    fn the_anime_flag_defaults_off_and_travels() {
        // An app that predates the flags sends none: every one defaults off.
        let plain = serde_json::json!({
            "op": "faces", "id": 4, "path": "/show/01.mkv",
            "info": serde_json::to_value(info()).unwrap(),
            "index": "AQI=", "shots": [[0, 100]], "resume": 0, "chunk": 100
        });
        match serde_json::from_value::<Request>(plain).unwrap() {
            Request::Faces(j) => {
                assert!(!j.anime && !j.fast && !j.animated && !j.bodies && j.known.is_none())
            }
            other => panic!("{other:?}"),
        }
        let job = FacesJob {
            id: 4,
            path: PathBuf::from("/show/01.mkv"),
            info: info(),
            index: Blob(vec![1, 2]),
            shots: vec![(0, 100)],
            resume: 0,
            chunk: 100,
            bodies: false,
            known: None,
            animated: false,
            fast: false,
            anime: true,
        };
        let line = serde_json::to_string(&Request::Faces(Box::new(job))).unwrap();
        assert!(line.contains(r#""anime":true"#), "{line}");
        match serde_json::from_str::<Request>(&line).unwrap() {
            Request::Faces(j) => assert!(j.anime && j.id == 4),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn embed_jobs_and_their_vectors_round_trip() {
        let r = Request::Embed(Box::new(EmbedJob { id: 9, path: PathBuf::from("portrait.jpg") }));
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"op":"embed","id":9,"path":"portrait.jpg"}"#);
        assert_eq!(r.id(), Some(9));
        match serde_json::from_str::<Request>(&line).unwrap() {
            Request::Embed(j) => assert_eq!(j.id, 9),
            other => panic!("{other:?}"),
        }
        let r = Reply::Embedding { id: 9, embedding: vec![0.5, -1.0] };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"event":"embedding","id":9,"embedding":[0.5,-1.0]}"#);
        match serde_json::from_str::<Reply>(&line).unwrap() {
            Reply::Embedding { id: 9, embedding } => assert_eq!(embedding, [0.5, -1.0]),
            other => panic!("{other:?}"),
        }
    }
}
