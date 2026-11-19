//! `autoscene-cloud-gateway <config.toml>`: AutoScene Cloud's gateway.

use std::path::PathBuf;

use as_cloud_gateway::config::Config;
use as_cloud_gateway::db::Db;
use as_cloud_gateway::dispatch::{self, Ffprobe};
use as_cloud_gateway::stripe::StripeApi;
use as_cloud_gateway::{Gateway, serve};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: autoscene-cloud-gateway <config.toml>");
        std::process::exit(2);
    };
    let cfg = match Config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&cfg.data_dir) {
        eprintln!("{}: {e}", cfg.data_dir.display());
        std::process::exit(1);
    }
    let db = match Db::open(&cfg.data_dir.join("gateway.db")) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let backend = dispatch::backend(&cfg.backend);
    let prober = Box::new(Ffprobe(cfg.ffprobe.clone()));
    let payments = Box::new(StripeApi { cfg: cfg.stripe.clone() });
    let gw = match Gateway::new(cfg, db, backend, prober, payments) {
        Ok(gw) => gw,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = serve(gw).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
