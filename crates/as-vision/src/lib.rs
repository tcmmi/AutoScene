//! Vision models on top of [`as_infer`]: TransNetV2 shot boundaries, SCRFD
//! face detection and ArcFace identity embeddings — or, for Fast Scan,
//! a smaller SCRFD and SFace — and people: their detectors and an appearance
//! embedding for finding them again. For anime stills: deepghs's YOLO
//! detectors, the CCIP character embedding and Berkeley's.

pub mod arcface;
pub mod berkeley;
pub mod ccip;
pub mod golden;
pub mod image;
pub mod person;
pub mod reid;
pub mod scrfd;
pub mod sface;
pub mod transnet;
pub mod yolo;
pub mod yunet;

pub use arcface::{ArcFace, Embedding};
pub use berkeley::Berkeley;
pub use ccip::Ccip;
pub use person::{Fit, Norm, PersonBox, PpDet};
pub use reid::PersonReid;
pub use scrfd::{FaceBox, Heads, Scrfd};
pub use sface::SFace;
pub use transnet::{TransNet, TransNetStream};
pub use yolo::{Det, Yolo};
pub use yunet::YuNet;
