//! Reusable ffmpeg filter snippets.

use crate::{HdrKind, MediaInfo};

/// HDR (PQ/HLG) → SDR BT.709 with Hable tone mapping. Identical on every
/// platform (zimg), so posters, proxies and exports look the same everywhere.
/// The HDR metadata goes too: an SDR picture claiming HDR light levels
/// confuses players and editors.
pub const TONEMAP_SDR: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=hable:desat=0,\
     zscale=t=bt709:m=bt709:r=tv,sidedata=mode=delete:type=MASTERING_DISPLAY_METADATA,\
     sidedata=mode=delete:type=CONTENT_LIGHT_LEVEL,sidedata=mode=delete:type=DYNAMIC_HDR_PLUS,\
     sidedata=mode=delete:type=DYNAMIC_HDR_VIVID,sidedata=mode=delete:type=DOVI_RPU_BUFFER,\
     sidedata=mode=delete:type=DOVI_METADATA";

/// [`TONEMAP_SDR`] for a picture that is kept: it stays at 16 bits for what
/// comes after it, so the last step down to 8 or 10 bits dithers. Straight
/// from `zscale` there is no dither, and a dark gradient comes out in bands
/// (13 steps across a test ramp, 959 this way, at the same speed); `zscale`'s
/// own dither is lost to any `scale` after it. The scan doesn't need it, and
/// its signals would change.
pub const TONEMAP_SDR_EXPORT: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=hable:desat=0,\
     zscale=t=bt709:m=bt709:r=tv,format=yuv444p16le,sidedata=mode=delete:type=MASTERING_DISPLAY_METADATA,\
     sidedata=mode=delete:type=CONTENT_LIGHT_LEVEL,sidedata=mode=delete:type=DYNAMIC_HDR_PLUS,\
     sidedata=mode=delete:type=DYNAMIC_HDR_VIVID,sidedata=mode=delete:type=DOVI_RPU_BUFFER,\
     sidedata=mode=delete:type=DOVI_METADATA";

/// Filters that turn decoded frames into what a viewer should see, without
/// changing the number of frames:
/// - deinterlace one-frame-per-frame (`bwdif send_frame`) for interlaced sources,
/// - tone map HDR to SDR when requested,
/// - convert anamorphic pixels to square,
/// - scale to `height` (never upscale) with even dimensions.
pub fn display_chain(info: &MediaInfo, height: Option<u32>, tonemap: bool) -> Vec<String> {
    let mut f: Vec<String> = deinterlace(info).into_iter().collect();
    f.extend(picture_chain(info, height, tonemap));
    f
}

/// [`display_chain`] without the deinterlacing: for a chain that
/// deinterlaces before it picks frames (a deinterlacer needs the frames
/// around the one it makes).
pub fn picture_chain(info: &MediaInfo, height: Option<u32>, tonemap: bool) -> Vec<String> {
    chain(info, height, tonemap.then_some(TONEMAP_SDR))
}

/// [`picture_chain`] for a clip that is kept: tone mapped with dither.
pub fn export_picture_chain(info: &MediaInfo, height: Option<u32>, tonemap: bool) -> Vec<String> {
    chain(info, height, tonemap.then_some(TONEMAP_SDR_EXPORT))
}

fn chain(info: &MediaInfo, height: Option<u32>, tonemap: Option<&str>) -> Vec<String> {
    let mut f = Vec::new();
    if let Some(tonemap) = tonemap.filter(|_| info.video.hdr != HdrKind::None) {
        f.push(tonemap.to_string());
    }
    let sar = info.video.sar;
    if sar.num != sar.den {
        f.push("scale=trunc(iw*sar/2)*2:ih,setsar=1".to_string());
    }
    if let Some(h) = height {
        f.push(scale_to_height(h));
    }
    f
}

/// Scale to `h` pixels tall (never upscale) with an even width.
pub fn scale_to_height(h: u32) -> String {
    format!("scale=-2:'min({h},ih)':flags=bicubic")
}

/// One-frame-per-frame deinterlacing for interlaced sources (the canonical
/// grid keeps every frame), `None` for progressive ones.
pub fn deinterlace(info: &MediaInfo) -> Option<String> {
    if !info.is_interlaced() {
        return None;
    }
    // Field order from the stream or idet; frames of unflagged sources
    // don't carry it, and bwdif would otherwise assume top field first.
    let parity = match info.video.field_order.as_deref() {
        Some("bb" | "tb") => "bff",
        _ => "tff",
    };
    Some(format!("bwdif=mode=send_frame:parity={parity}"))
}
