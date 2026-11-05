// Prevents an extra console window on Windows in release. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `AutoScene --engine …` is the analysis engine, which the app starts as a
    // process of its own (see `as_engine`).
    // `AutoScene --compile-plugin …` compiles one plugin's component, in a
    // process the app can end (see `as_plugin::wasm::Compiler`).
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(a) if a == "--engine" => std::process::exit(as_engine::main_from_args(args)),
        Some(a) if a == as_plugin::wasm::COMPILE_ARG => std::process::exit(as_plugin::wasm::compile_main(args)),
        _ => {}
    }
    autoscene_app::run();
}
