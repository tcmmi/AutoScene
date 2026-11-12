//! Dev tool: what the anime pass's two detectors find in every sample of an
//! episode, before anything is joined — the head detector's heads and the
//! person detector's people — to try rules for joining them
//! (`as_analysis::anime::union_heads`) without running the pass.
//! `cargo run --release -p as-analysis --example anime_dets -- <ort_dir> <ffmpeg_dir> <dir with
//! bb.index/bb.info.json/bb.path/bb.segments.tsv> <out.tsv>`
//! `HEAD=<onnx>` and `PERSON=<onnx>` are the models, `HEAD_INPUT` and
//! `PERSON_INPUT=<w>x<h>` their letterboxed inputs (defaults 640x384 and
//! 1280x736), as `anime_run` takes them. The samples are the pass's: every
//! `stride`-th frame inside a shot, decoded at the pass's size, in the
//! pass's batches. `CPU=1` runs the detectors on the processor (slow; its
//! boxes differ from a graphics card's in the last digits).
//! `UNTIL=<frame>` stops there.
//! Writes `<out.tsv>`: a `# size <w> <h>` line, then one line per box,
//! `frame kind x1 y1 x2 y2 score` in decoded pixels, `kind` `h` (a head) or
//! `p` (a person), each kind strongest first within its frame.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use as_analysis::DETECT_BATCH;
use as_analysis::anime::{self, Params};
use as_core::{CancelToken, FrameIndex, FrameRange};
use as_infer::Runtime;
use as_media::{MediaInfo, Source, Tools};
use as_vision::yolo::Yolo;

/// `<w>x<h>` of an env var, `fallback` when unset.
fn input(var: &str, fallback: (usize, usize)) -> (usize, usize) {
    std::env::var(var).ok().map_or(fallback, |v| {
        let (w, h) = v.split_once('x').unwrap_or_else(|| panic!("{var}=<w>x<h>"));
        (w.parse().expect(var), h.parse().expect(var))
    })
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (ort, ffmpeg, dir, out) = (&a[0], &a[1], Path::new(&a[2]), &a[3]);
    let index = FrameIndex::from_bytes(&std::fs::read(dir.join("bb.index")).unwrap()).unwrap();
    let info: MediaInfo = serde_json::from_str(&std::fs::read_to_string(dir.join("bb.info.json")).unwrap()).unwrap();
    let path = PathBuf::from(std::fs::read_to_string(dir.join("bb.path")).unwrap().trim());
    let shots: Vec<FrameRange> = std::fs::read_to_string(dir.join("bb.segments.tsv"))
        .unwrap()
        .lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split('\t').collect();
            (v[2] == "shot").then(|| FrameRange::new(v[0].parse().unwrap(), v[1].parse().unwrap()))
        })
        .collect();
    let tools = Tools::locate(Some(Path::new(ffmpeg))).unwrap();
    let cpu = std::env::var("CPU").is_ok_and(|v| v == "1");
    let rt = Runtime::load_with(Path::new(ort), &as_infer::Placement::from_gpu(!cpu)).unwrap();
    let params = Params::anime();
    let size = anime::frame_size(&info, &params);
    let model = |var: &str| PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("{var}=<model.onnx>")));
    let mut heads = Yolo::load(&rt, &model("HEAD"), size, input("HEAD_INPUT", (640, 384)), DETECT_BATCH).unwrap();
    let mut persons =
        Yolo::load(&rt, &model("PERSON"), size, input("PERSON_INPUT", (1280, 736)), DETECT_BATCH).unwrap();
    println!(
        "detectors at {size:?}, stride {}; heads on {:?}, persons on {:?}",
        params.stride,
        heads.device(),
        persons.device()
    );
    let mut file = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    writeln!(file, "# size {} {}", size.0, size.1).unwrap();
    let t0 = Instant::now();
    let (mut samples, mut found) = (0usize, [0usize; 2]);
    let mut queue: Vec<(u32, Vec<u8>)> = Vec::with_capacity(DETECT_BATCH);
    let mut flush = |queue: &mut Vec<(u32, Vec<u8>)>| {
        let frames: Vec<&[u8]> = queue.iter().map(|(_, rgb)| rgb.as_slice()).collect();
        let h = heads.detect_many(&frames, params.det_threshold).unwrap();
        let p = persons.detect_many(&frames, params.person_threshold).unwrap();
        for (((f, _), h), p) in queue.iter().zip(h).zip(p) {
            for (k, kind, dets) in [(0, 'h', h), (1, 'p', p)] {
                found[k] += dets.len();
                for d in dets {
                    let b = d.bbox;
                    writeln!(file, "{f}\t{kind}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.4}", b[0], b[1], b[2], b[3], d.score)
                        .unwrap();
                }
            }
        }
        samples += queue.len();
        queue.clear();
    };
    let in_shot = |f: u32| {
        let i = shots.partition_point(|s| s.end <= f);
        shots.get(i).is_some_and(|s| s.contains(f))
    };
    let src = Source { path: &path, info: &info, index: &index };
    let (w, h) = size;
    let until: u32 = std::env::var("UNTIL").map_or(u32::MAX, |v| v.parse().expect("UNTIL=<frame>"));
    as_media::frames::decode_samples(
        &tools,
        src,
        0,
        params.stride,
        (w as u32, h as u32),
        &CancelToken::new(),
        |f, rgb| {
            if f >= until {
                return Ok(false);
            }
            if in_shot(f) {
                queue.push((f, rgb.to_vec()));
                if queue.len() >= DETECT_BATCH {
                    flush(&mut queue);
                }
            }
            Ok(true)
        },
    )
    .unwrap();
    flush(&mut queue);
    println!("{samples} samples in {:.0}s: {} heads, {} people", t0.elapsed().as_secs_f64(), found[0], found[1]);
}
