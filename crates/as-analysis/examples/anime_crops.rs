//! Dev tool: the head crops the Best anime scan reads, of the tracks an
//! anime pass found — to embed them with another embedder outside the app
//! (`tools/train`) and score it on the pass's own tracks, and to train one.
//! `cargo run --release -p as-analysis --example anime_crops -- <ffmpeg_dir> <dir with
//! bb.index/bb.info.json/bb.path> <file.tracks> <out_prefix>`
//! The pass's samples are decoded again (every `stride`-th frame at the
//! pass's size, in software, as the pass decodes them) and each chosen head
//! is cropped as `as_vision::berkeley::crop` crops it: its box padded by a
//! quarter on each side, resized to 224×224.
//! `HEADS=<n>`: heads per track (4, the Best scan's), its clearest first, as
//! `as_analysis::anime` picks them for a track's identity (the pass keeps at
//! most 1,024 crops a shot and a track's head without one is passed over;
//! that cap isn't followed here, and no shot of the tuning episodes reaches
//! it: their offline vectors are the pass's). `APART=<frames>`:
//! only heads at least that many frames apart (0: any), for pairs of one
//! track that aren't one held drawing.
//! Writes
//! - `<out_prefix>.crops`: the crops one after another, RGB24, 150,528
//!   bytes each;
//! - `<out_prefix>.crops.tsv`: a `# tracks <n> size <w> <h> stride <s>`
//!   line, then per crop `crop track rank frame x1 y1 x2 y2 quality` (the
//!   track by its place in the file, the box as the track stores it);
//! - `<out_prefix>.pairs.tsv`: per two tracks seen in the same samples,
//!   `a b samples iou cover cosine` — in how many samples, the greatest
//!   overlap of their boxes there (over their union, and over the smaller
//!   box: a second box on one head), and how alike the pass's own
//!   embeddings are.

use std::collections::{BTreeMap, HashMap};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use as_analysis::anime::{self, Params};
use as_analysis::tracks::{self, FaceDet};
use as_core::{CancelToken, FrameIdx, FrameIndex};
use as_media::{MediaInfo, Source, Tools};
use as_vision::berkeley;

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().map_or(default, |v| v.parse().unwrap_or_else(|_| panic!("{name}=<number>")))
}

/// The overlap of two boxes: over their union, and over the smaller one.
fn overlap(a: [f32; 4], b: [f32; 4]) -> (f32, f32) {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let area = |x: [f32; 4]| (x[2] - x[0]).max(0.0) * (x[3] - x[1]).max(0.0);
    let (both, a, b) = (w * h, area(a), area(b));
    if both <= 0.0 {
        return (0.0, 0.0);
    }
    (both / (a + b - both), both / a.min(b))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (ffmpeg, dir, file, out) = (&a[0], Path::new(&a[1]), &a[2], &a[3]);
    let (heads, apart): (usize, u32) = (env("HEADS", anime::BEST_HEADS), env("APART", 0));
    let index = FrameIndex::from_bytes(&std::fs::read(dir.join("bb.index")).unwrap()).unwrap();
    let info: MediaInfo = serde_json::from_str(&std::fs::read_to_string(dir.join("bb.info.json")).unwrap()).unwrap();
    let path = PathBuf::from(std::fs::read_to_string(dir.join("bb.path")).unwrap().trim());
    let tools = Tools::locate(Some(Path::new(ffmpeg))).unwrap();
    let params = Params::anime();
    let (w, h) = anime::frame_size(&info, &params);
    let all = tracks::decode(&std::fs::read(file).expect("tracks file")).expect("readable tracks");

    // The heads to crop, by the sample they are in: the crop's number and
    // its box in the decoded frame's pixels.
    let mut wanted: HashMap<FrameIdx, Vec<(usize, [f32; 4])>> = HashMap::new();
    let mut rows: Vec<String> = Vec::new();
    for (i, t) in all.iter().enumerate() {
        let mut dets: Vec<&FaceDet> = t.dets.iter().collect();
        // As the Best scan picks a track's heads: the clearest, then the tallest.
        dets.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(b.height().total_cmp(&a.height())));
        let mut kept: Vec<&FaceDet> = Vec::new();
        for d in dets {
            if kept.len() == heads {
                break;
            }
            if apart == 0 || kept.iter().all(|k| k.frame.abs_diff(d.frame) >= apart) {
                kept.push(d);
            }
        }
        for (rank, d) in kept.into_iter().enumerate() {
            assert_eq!(d.frame % params.stride, 0, "a head between the pass's samples");
            let b = d.bbox;
            let px = [b[0] * w as f32, b[1] * h as f32, b[2] * w as f32, b[3] * h as f32];
            wanted.entry(d.frame).or_default().push((rows.len(), px));
            rows.push(format!(
                "{}\t{i}\t{rank}\t{}\t{}\t{}\t{}\t{}\t{}",
                rows.len(),
                d.frame,
                b[0],
                b[1],
                b[2],
                b[3],
                d.quality
            ));
        }
    }

    // Tracks seen together: they are two characters, unless two boxes on one head.
    let mut seen: BTreeMap<FrameIdx, Vec<(usize, [f32; 4])>> = BTreeMap::new();
    for (i, t) in all.iter().enumerate() {
        for d in &t.dets {
            seen.entry(d.frame).or_default().push((i, d.bbox));
        }
    }
    let mut pairs: BTreeMap<(usize, usize), (u32, f32, f32)> = BTreeMap::new();
    for boxes in seen.values() {
        for (k, &(x, bx)) in boxes.iter().enumerate() {
            for &(y, by) in &boxes[k + 1..] {
                if x == y {
                    continue;
                }
                let (iou, cover) = overlap(bx, by);
                let e = pairs.entry((x.min(y), x.max(y))).or_insert((0, 0.0, 0.0));
                *e = (e.0 + 1, e.1.max(iou), e.2.max(cover));
            }
        }
    }
    let mut text = String::from("a\tb\tsamples\tiou\tcover\tcosine\n");
    for (&(x, y), &(n, iou, cover)) in &pairs {
        let (ea, eb) = (&all[x].embedding, &all[y].embedding);
        let cosine: f32 = if ea.len() == eb.len() { ea.iter().zip(eb).map(|(p, q)| p * q).sum() } else { f32::NAN };
        text.push_str(&format!("{x}\t{y}\t{n}\t{iou:.4}\t{cover:.4}\t{cosine:.4}\n"));
    }
    std::fs::write(format!("{out}.pairs.tsv"), text).unwrap();

    let mut blob = std::fs::File::create(format!("{out}.crops")).unwrap();
    blob.set_len((rows.len() * berkeley::CROP_BYTES) as u64).unwrap();
    let src = Source { path: &path, info: &info, index: &index };
    let (t0, mut done) = (Instant::now(), 0usize);
    let last = wanted.keys().copied().max().unwrap_or(0);
    as_media::frames::decode_samples(
        &tools,
        src,
        0,
        params.stride,
        (w as u32, h as u32),
        &CancelToken::new(),
        |f, rgb| {
            for &(n, px) in wanted.get(&f).map_or(&[][..], |v| v) {
                blob.seek(SeekFrom::Start((n * berkeley::CROP_BYTES) as u64)).unwrap();
                blob.write_all(&berkeley::crop(rgb, w, h, px)).unwrap();
                done += 1;
            }
            Ok(f < last)
        },
    )
    .unwrap();
    assert_eq!(done, rows.len(), "every chosen head is in a decoded sample");
    let head = format!(
        "# tracks {} size {w} {h} stride {}\ncrop\ttrack\trank\tframe\tx1\ty1\tx2\ty2\tquality\n",
        all.len(),
        params.stride
    );
    std::fs::write(format!("{out}.crops.tsv"), head + &rows.join("\n") + "\n").unwrap();
    println!(
        "{} crops of {} tracks ({} pairs seen together) in {:.1} s",
        rows.len(),
        all.len(),
        pairs.len(),
        t0.elapsed().as_secs_f64()
    );
}
