//! Dev tool: the scenes pass of the app over a real episode
//! (`as_analysis::pictures::shot_pictures`), written as the file the scenes
//! bench reads with `EMB=` (`bench/scenes/README.md`): `ASEM`, the number of
//! rows and the vector length as u32, then per row the frame (u32) and its
//! unit vector (f32), little-endian.
//! `cargo run --release -p as-analysis --example scenes_run -- <ort_dir> <dinov2-small model.onnx> <ffmpeg_dir> <dir
//! with bb.index/bb.info.json/bb.path/bb.segments.tsv> <out.emb>`
//! `CPU=1` keeps the embedder off the graphics card, `NVDEC=1` decodes there.

use std::path::{Path, PathBuf};
use std::time::Instant;

use as_analysis::pictures::{self, shot_pictures};
use as_analysis::scenes::picture_frames;
use as_core::{CancelToken, FrameIndex, FrameRange};
use as_infer::Runtime;
use as_media::frames::Decoder;
use as_media::{MediaInfo, Source, Tools};
use as_vision::Dinov2;
use as_vision::dinov2::DIM;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (ort, model, ffmpeg, dir, out) =
        (Path::new(&a[0]), Path::new(&a[1]), &a[2], Path::new(&a[3]), Path::new(&a[4]));
    let gpu = std::env::var_os("CPU").is_none();
    let decoder = if std::env::var_os("NVDEC").is_some() { Decoder::Nvdec } else { Decoder::Cpu };

    let index = FrameIndex::from_bytes(&std::fs::read(dir.join("bb.index")).unwrap()).unwrap();
    let info: MediaInfo = serde_json::from_str(&std::fs::read_to_string(dir.join("bb.info.json")).unwrap()).unwrap();
    let path = PathBuf::from(std::fs::read_to_string(dir.join("bb.path")).unwrap().trim());
    let shots: Vec<FrameRange> = std::fs::read_to_string(dir.join("bb.segments.tsv"))
        .unwrap()
        .lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split('\t').collect();
            (v[2].trim() == "shot").then(|| FrameRange::new(v[0].parse().unwrap(), v[1].parse().unwrap()))
        })
        .collect();
    let tools = Tools::locate(Some(Path::new(ffmpeg))).unwrap();
    let rt = Runtime::load(ort, gpu).unwrap();
    let mut net = Dinov2::load(&rt, model, 8).unwrap();
    println!("{} shots, pictures of {:?}, the embedder on {:?}", shots.len(), pictures::SIZE, net.device());

    let wall = Instant::now();
    let mut rows: Vec<(u32, Vec<f32>)> = Vec::new();
    let src = Source { path: &path, info: &info, index: &index };
    shot_pictures(
        &tools,
        src,
        &mut net,
        &shots,
        decoder,
        &CancelToken::new(),
        |_| {},
        |chunk| {
            for shot in chunk {
                let range = shots[shots.partition_point(|s| s.start < shot.start)];
                let frames = picture_frames(range.start, range.end);
                assert_eq!(frames.len(), shot.vectors.len(), "one vector per picture of shot {}", shot.start);
                rows.extend(frames.into_iter().zip(shot.vectors));
            }
            Ok(())
        },
    )
    .unwrap();
    let secs = wall.elapsed().as_secs_f64();
    let hours = index.len() as f64 / index.nominal_fps.to_f64() / 3600.0;
    println!("{} pictures in {secs:.0} s ({:.0} s per hour of video)", rows.len(), secs / hours);

    let mut bytes = Vec::with_capacity(12 + rows.len() * (4 + DIM * 4));
    bytes.extend(b"ASEM");
    bytes.extend((rows.len() as u32).to_le_bytes());
    bytes.extend((DIM as u32).to_le_bytes());
    for (frame, v) in &rows {
        bytes.extend(frame.to_le_bytes());
        bytes.extend(v.iter().flat_map(|x| x.to_le_bytes()));
    }
    std::fs::write(out, bytes).unwrap();
    println!("{}", out.display());
}
