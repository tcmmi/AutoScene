//! Prints what AutoScene's analysis runs on here: the line the app logs at
//! start and, on Windows, both DXCore lists DirectML's default can take its
//! adapters from, with every adapter in them. One small program, so a computer
//! without an AutoScene build can be checked (design-i52 §5.1):
//!
//! ```text
//! cargo build -p as-infer --example computer --release
//! ```

use as_infer::computer::{self, Choice};

fn main() {
    let start = std::time::Instant::now();
    let machine = computer::machine();
    let read_in = start.elapsed();
    #[cfg(windows)]
    match computer::Lists::read() {
        Some(lists) => {
            println!("this computer: {}", lists.line());
            for (source, list) in [("GENERIC_ML", &lists.generic_ml), ("CORE_COMPUTE", &lists.core_compute)] {
                println!("{source}: {} adapters, sorted for high performance", list.len());
                for (i, adapter) in list.iter().enumerate() {
                    println!("  {i}: {adapter}");
                }
            }
        }
        None => println!("this computer: DXCore can't be read, so its graphics are of unknown kind"),
    }
    let auto = computer::choose(machine, &Choice::Auto);
    #[cfg(not(windows))]
    println!("this computer: {auto}");
    println!("on Automatic: {:?}; Fast Scan by default: {}", auto.class, auto.class.fast_by_default());
    println!("read in {read_in:?}");
}
