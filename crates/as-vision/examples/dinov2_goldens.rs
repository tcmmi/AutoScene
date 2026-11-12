//! Dev tool: writes the goldens of the scenes' picture embedder
//! (`as_vision::dinov2`): the vectors the processor gives its two made-up
//! pictures, as little-endian f32 (`dinov2_<k>.vec`).
//! `cargo run --release -p as-vision --example dinov2_goldens -- <ort_dir> <dinov2-small model.onnx> crates/as-vision/fixtures/scenes`
//! Run it again only when the model file changes (its hash in
//! `models/manifest.toml`).

use std::path::Path;

use as_infer::Runtime;
use as_vision::dinov2::{self, Dinov2};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (ort, model, out) = (Path::new(&a[0]), Path::new(&a[1]), Path::new(&a[2]));
    let rt = Runtime::load(ort, false).expect("the ONNX runtime");
    let mut net = Dinov2::open(&rt, model, 1).expect("the model");
    std::fs::create_dir_all(out).expect("the output folder");
    for k in 0..2 {
        let picture = dinov2::golden_picture(k);
        let e = &net.embed(&[&picture]).expect("an embedding")[0];
        // As the network gave it: the proof compares directions only.
        let bytes: Vec<u8> = e.v.iter().flat_map(|v| (v * e.norm).to_le_bytes()).collect();
        let path = out.join(format!("dinov2_{k}.vec"));
        std::fs::write(&path, bytes).expect("writing a golden");
        println!("{}: length {:.3} on {:?}", path.display(), e.norm, net.device());
    }
}
