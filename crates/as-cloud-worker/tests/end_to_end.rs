//! The whole cloud on one machine: the gateway on a local port, the real
//! worker program started by it for each session, a stand-in engine (the
//! worker's `--fake-engine`), and the app's own client.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use as_cloud::admin;
use as_cloud::client::{Client, ClientError, file_sha256};
use as_cloud::{Depth, ErrorCode, Footage, RegisterRequest, ScanKind, SessionRequest, Stage, Switch};
use as_cloud_gateway::config::{self, Config};
use as_cloud_gateway::db::Db;
use as_cloud_gateway::dispatch::{Local, Prober};
use as_cloud_gateway::stripe::Payments;
use as_cloud_gateway::{Gateway, crypto, serve_on};
use serde_json::{Value, json};

const ADMIN: &str = "admin-token-for-the-tests-only";
const WEBHOOK: &str = "whsec_test_secret";

/// Every upload lasts two minutes.
struct TwoMinutes;
impl Prober for TwoMinutes {
    fn duration_ms(&self, _: &Path) -> Result<u64, String> {
        Ok(120_000)
    }
}

/// Two minutes too, but measuring takes a second: time to close meanwhile.
struct SlowTwoMinutes;
impl Prober for SlowTwoMinutes {
    fn duration_ms(&self, _: &Path) -> Result<u64, String> {
        std::thread::sleep(Duration::from_secs(1));
        Ok(120_000)
    }
}

struct NoStripe;
impl Payments for NoStripe {
    fn checkout(&self, _: i64, _: &str, cents: i64) -> Result<String, String> {
        Ok(format!("https://checkout.test/{cents}"))
    }
}

struct Cloud {
    base: String,
    admin: String,
    data: tempfile::TempDir,
}

fn start() -> Cloud {
    start_with(Box::new(TwoMinutes))
}

fn start_with(prober: Box<dyn Prober>) -> Cloud {
    start_opts(prober, false)
}

/// `verify`: the worker reads the file itself and refuses jobs that don't fit it.
/// The cloud is open: Stripe's test mode may sell here.
fn start_opts(prober: Box<dyn Prober>, verify: bool) -> Cloud {
    let cloud = start_cfg(prober, verify, None, |c| c.stripe.test_mode_ok = true);
    let (status, stats) = admin_post(&cloud, admin::STAGE, json!({"stage": "open"}));
    assert_eq!((status, stats["stage"].as_str()), (200, Some("open")));
    cloud
}

/// A gateway with `tweak` made to its settings, its database holding `stored` as
/// the launch stage (none: it starts hidden).
fn start_cfg(
    prober: Box<dyn Prober>,
    verify: bool,
    stored: Option<&'static str>,
    tweak: impl FnOnce(&mut Config) + Send + 'static,
) -> Cloud {
    let data = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let dir = data.path().to_path_buf();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let public = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let admin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let (pa, aa): (SocketAddr, SocketAddr) = (public.local_addr().unwrap(), admin.local_addr().unwrap());
            let worker = PathBuf::from(env!("CARGO_BIN_EXE_autoscene-cloud-worker"));
            let mut cfg = Config {
                listen: pa,
                admin_listen: aa,
                public_base: format!("http://{pa}"),
                data_dir: dir.clone(),
                admin_token: ADMIN.into(),
                pepper: "pepper-for-the-tests-only-0123456789".into(),
                trust_proxy: false,
                ffprobe: "ffprobe".into(),
                max_upload_bytes: 64 << 20,
                max_uploads_total: 256 << 20,
                upload_ttl_secs: 3600,
                app_idle_secs: 3600,
                worker_idle_secs: 10,
                worker_timeout_secs: 300,
                prices: Default::default(),
                guard: Default::default(),
                stripe: config::Stripe { webhook_secret: WEBHOOK.into(), ..Default::default() },
                backend: config::Backend::None,
            };
            tweak(&mut cfg);
            let backend = Box::new(Local {
                worker: worker.clone(),
                args: [
                    vec!["--engine".into(), worker.to_string_lossy().into_owned()],
                    if verify { vec![] } else { vec!["--no-verify".into()] },
                    vec![
                        "--work".into(),
                        dir.join("worker").to_string_lossy().into_owned(),
                        "--".into(),
                        "--fake-engine".into(),
                    ],
                ]
                .concat(),
            });
            let db = Db::open(&dir.join("gateway.db")).unwrap();
            if let Some(stage) = stored {
                db.set_setting("stage", stage).unwrap();
            }
            let gw = Gateway::new(cfg, db, backend, prober, Box::new(NoStripe)).unwrap();
            tx.send((pa, aa)).unwrap();
            serve_on(gw, public, admin, std::future::pending()).await.unwrap();
        });
    });
    let (pa, aa) = rx.recv().unwrap();
    Cloud { base: format!("http://{pa}"), admin: format!("http://{aa}"), data }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).build().into()
}

fn admin_post(cloud: &Cloud, p: &str, body: Value) -> (u16, Value) {
    let mut r = agent()
        .post(&format!("{}{p}", cloud.admin))
        .header("Authorization", &format!("Bearer {ADMIN}"))
        .header(admin::ACTOR, "tester")
        .header("Content-Type", "application/json")
        .send(body.to_string().as_bytes())
        .unwrap();
    let status = r.status().as_u16();
    (status, serde_json::from_slice(&r.body_mut().read_to_vec().unwrap()).unwrap_or(Value::Null))
}

fn admin_get(cloud: &Cloud, p: &str) -> Value {
    let mut r =
        agent().get(&format!("{}{p}", cloud.admin)).header("Authorization", &format!("Bearer {ADMIN}")).call().unwrap();
    assert_eq!(r.status().as_u16(), 200);
    serde_json::from_slice(&r.body_mut().read_to_vec().unwrap()).unwrap()
}

fn device(cloud: &Cloud, n: u8) -> Client {
    let mut c = Client::new(cloud.base.clone(), None);
    let hwid = format!("{:064x}", n);
    c.register(&RegisterRequest { hwid, platform: "test".into(), app_version: "1.0.6".into() }).unwrap();
    c
}

fn code(e: ClientError) -> ErrorCode {
    e.code().unwrap_or_else(|| panic!("not an answer of the gateway: {e}"))
}

fn webhook(cloud: &Cloud, body: &Value, secret: &str) -> u16 {
    let payload = body.to_string();
    let sig = crypto::stripe_sign(secret, payload.as_bytes(), as_cloud_gateway::now());
    agent()
        .post(&format!("{}/v1/stripe/webhook", cloud.base))
        .header("Stripe-Signature", &sig)
        .send(payload.as_bytes())
        .unwrap()
        .status()
        .as_u16()
}

const ANIME: ScanKind = ScanKind { footage: Footage::Anime, depth: Depth::Standard, people: false, scenes: false };

fn file(dir: &Path, bytes: usize) -> PathBuf {
    let p = dir.join(format!("episode-{bytes}.mkv"));
    let data: Vec<u8> = (0..bytes).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::write(&p, data).unwrap();
    p
}

/// Read replies until the job `id` is done.
fn until_done(c: &Client, session: &str, id: u64) -> Vec<Value> {
    let mut after = 0;
    let mut all = Vec::new();
    for _ in 0..60 {
        let r = c.events(session, after).unwrap();
        after = r.next;
        all.extend(r.lines.iter().map(|l| serde_json::from_str::<Value>(l).unwrap()));
        if all.iter().any(|v| v["event"] == "done" && v["id"] == id) {
            return all;
        }
        assert!(!r.ended, "the session ended: {:?}", r.error);
    }
    panic!("job {id} never finished: {all:?}");
}

#[test]
fn the_cloud_end_to_end() {
    let cloud = start();
    let app = device(&cloud, 1);
    let account = app.account().unwrap();
    assert_eq!((account.balance, account.beta_until), (0, None));
    assert!(account.open.contains(&Switch::Anime));

    // No money, no scan.
    let ep = file(cloud.data.path(), 3 << 20);
    let sha = file_sha256(&ep, |_| true).unwrap();
    let req = SessionRequest { kind: ANIME, size: 3 << 20, sha256: sha.clone(), duration_ms: 60_000 };
    assert_eq!(code(app.open_session(&req).unwrap_err()), ErrorCode::Balance);

    // A €1 code, once; wrong codes are counted and then refused outright.
    let (status, made) =
        admin_post(&cloud, admin::CODES, json!({"worth": {"kind": "credit", "cents": 100}, "count": 2}));
    assert_eq!(status, 200, "{made}");
    let codes: Vec<String> = serde_json::from_value(made["codes"].clone()).unwrap();
    assert_eq!(app.redeem(&codes[0].to_lowercase()).unwrap().added, 1_000_000);
    assert_eq!(code(app.redeem(&codes[0]).unwrap_err()), ErrorCode::CodeUsed);
    assert_eq!(admin_post(&cloud, admin::REVOKE, json!({"code": codes[1]})).1["revoked"], true);
    assert_eq!(code(app.redeem(&codes[1]).unwrap_err()), ErrorCode::CodeInvalid, "revoked");
    for _ in 0..3 {
        assert_eq!(code(app.redeem("0000-0000-0000-0000").unwrap_err()), ErrorCode::CodeInvalid);
    }
    assert_eq!(code(app.redeem("0000-0000-0000-0000").unwrap_err()), ErrorCode::RateLimited);

    // A paid checkout counts once, and only when Stripe signed it.
    let support = account.support_id.clone();
    assert_eq!(app.checkout(1000).unwrap(), "https://checkout.test/1000");
    assert_eq!(code(app.checkout(999).unwrap_err()), ErrorCode::BadRequest);
    // Stripe hands back the gateway's own account id: the first account is 1.
    let id = 1;
    let paid = json!({"id": "evt_1", "type": "checkout.session.completed", "data": {"object": {
        "id": "cs_test_1", "payment_status": "paid", "currency": "eur", "amount_total": 1000,
        "client_reference_id": id.to_string(), "payment_intent": "pi_1"}}});
    assert_eq!(webhook(&cloud, &paid, "whsec_wrong"), 401);
    assert_eq!(webhook(&cloud, &paid, WEBHOOK), 200);
    assert_eq!(webhook(&cloud, &paid, WEBHOOK), 200);
    let mut again = paid.clone();
    again["id"] = json!("evt_2");
    assert_eq!(webhook(&cloud, &again, WEBHOOK), 200);
    assert_eq!(app.account().unwrap().balance, 11_000_000, "€1 code + €10 paid, once");

    // A scan: upload, priced by the gateway's own measure, run by a worker.
    let s = app.open_session(&req).unwrap();
    assert_eq!(s.estimate, 10_000, "one claimed minute");
    assert_eq!(code(app.open_session(&req).unwrap_err()), ErrorCode::Busy, "one at a time");
    let st = app.upload(&s.id, &ep, |_, _| true).unwrap();
    assert_eq!(st.received, 3 << 20);
    let ready = app.ready(&s.id).unwrap();
    assert_eq!((ready.duration_ms, ready.price), (120_000, 20_000), "two measured minutes, whatever was claimed");
    // A Fast Scan job in a standard session is refused.
    let wrong = json!({"op": "faces", "id": 9, "path": "x", "anime": true, "fast": true});
    assert_eq!(code(app.send_jobs(&s.id, vec![wrong]).unwrap_err()), ErrorCode::BadRequest);
    app.send_jobs(&s.id, vec![json!({"op": "shots", "id": 1, "path": "C:/mine/ep.mkv"})]).unwrap();
    app.send_jobs(&s.id, vec![json!({"op": "faces", "id": 2, "path": "C:/mine/ep.mkv", "anime": true})]).unwrap();
    let replies = until_done(&app, &s.id, 2);
    assert!(replies.iter().any(|v| v["event"] == "face_chunk" && v["id"] == 2), "{replies:?}");
    assert!(!replies.iter().any(|v| v["event"] == "failed"), "the worker's copy was found: {replies:?}");
    let closed = app.close(&s.id).unwrap();
    assert_eq!(closed.charged, 20_000);
    assert_eq!(closed.account.balance, 11_000_000 - 20_000);

    // The upload is gone once closed.
    let uploads: Vec<_> = std::fs::read_dir(cloud.data.path().join("uploads")).unwrap().collect();
    assert!(uploads.is_empty(), "{uploads:?}");

    // A file that isn't the announced one is refused, and costs nothing.
    let other = SessionRequest { sha256: "0".repeat(64), ..req.clone() };
    let s = app.open_session(&other).unwrap();
    app.upload(&s.id, &ep, |_, _| true).unwrap();
    assert_eq!(code(app.ready(&s.id).unwrap_err()), ErrorCode::Checksum);
    assert_eq!(app.account().unwrap().balance, 11_000_000 - 20_000);

    // Paused for everyone: no session opens, the account still reads.
    let (status, stats) = admin_post(&cloud, admin::PAUSE, json!({"paused": true}));
    assert_eq!((status, stats["paused"].as_bool()), (200, Some(true)));
    assert!(app.account().unwrap().paused);
    assert_eq!(code(app.open_session(&req).unwrap_err()), ErrorCode::Paused);
    admin_post(&cloud, admin::PAUSE, json!({"paused": false}));

    // A kind switched off opens no session.
    let (status, _) = admin_post(&cloud, admin::SWITCHES, json!({"switch": "anime", "on": false}));
    assert_eq!(status, 200);
    assert_eq!(code(app.open_session(&req).unwrap_err()), ErrorCode::KindOff);
    admin_post(&cloud, admin::SWITCHES, json!({"switch": "anime", "on": true}));

    // A refund takes the money back; a dispute blocks the account.
    let refund = json!({"id": "evt_3", "type": "charge.refunded", "data": {"object": {"payment_intent": "pi_1", "amount_refunded": 1000}}});
    assert_eq!(webhook(&cloud, &refund, WEBHOOK), 200);
    assert_eq!(app.account().unwrap().balance, 1_000_000 - 20_000);

    // A lost computer: an admin moves the balance to the new one (the beta
    // pass moves too: db's tests).
    let new_pc = device(&cloud, 2);
    let to = new_pc.account().unwrap().support_id;
    let (status, moved) =
        admin_post(&cloud, admin::TRANSFER, json!({"from": support, "to": to, "note": "receipt cs_test_1"}));
    assert_eq!(status, 200, "{moved}");
    assert_eq!(moved["moved"], 980_000);
    assert_eq!(new_pc.account().unwrap().balance, 980_000);
    assert_eq!(app.account().unwrap().balance, 0);
    // The wrong codes above locked only the old account out for the hour.
    let (_, beta) = admin_post(
        &cloud,
        admin::CODES,
        json!({"worth": {"kind": "beta", "until": as_cloud_gateway::now() + 86_400}, "count": 1}),
    );
    let beta: Vec<String> = serde_json::from_value(beta["codes"].clone()).unwrap();
    assert_eq!(new_pc.redeem(&beta[0]).unwrap().added, 0);
    assert!(new_pc.account().unwrap().beta_until.is_some());

    // On a beta pass a scan costs nothing.
    let s = new_pc.open_session(&req).unwrap();
    assert_eq!(s.estimate, 0);
    new_pc.upload(&s.id, &ep, |_, _| true).unwrap();
    assert_eq!(new_pc.ready(&s.id).unwrap().price, 0);
    new_pc.send_jobs(&s.id, vec![json!({"op": "faces", "id": 1, "path": "x", "anime": true})]).unwrap();
    until_done(&new_pc, &s.id, 1);
    assert_eq!(new_pc.close(&s.id).unwrap().charged, 0);

    let dispute =
        json!({"id": "evt_4", "type": "charge.dispute.created", "data": {"object": {"payment_intent": "pi_1"}}});
    assert_eq!(webhook(&cloud, &dispute, WEBHOOK), 200);
    assert!(app.account().unwrap().blocked);

    // The bot's view: the month's money and the account's ledger.
    let stats = admin_get(&cloud, admin::STATS);
    assert_eq!(stats["paid_in_cents"], 1000);
    assert_eq!(stats["refunded_cents"], 1000);
    let view = admin_get(&cloud, &admin::account(&to));
    assert_eq!(view["ledger"][0]["kind"], "transfer_in");
    // Workers report how long they ran.
    std::thread::sleep(Duration::from_millis(500));
}

/// Read replies until one satisfies `want`.
fn until(c: &Client, session: &str, want: impl Fn(&Value) -> bool) {
    let mut after = 0;
    for _ in 0..60 {
        let r = c.events(session, after).unwrap();
        after = r.next;
        if r.lines.iter().any(|l| want(&serde_json::from_str(l).unwrap())) {
            return;
        }
        assert!(!r.ended, "the session ended: {:?}", r.error);
    }
    panic!("never came");
}

fn funded(cloud: &Cloud, n: u8) -> Client {
    let app = device(cloud, n);
    let (_, made) = admin_post(cloud, admin::CODES, json!({"worth": {"kind": "credit", "cents": 100}, "count": 1}));
    app.redeem(made["codes"][0].as_str().unwrap()).unwrap();
    app
}

#[test]
fn a_scan_stopped_early_pays_for_what_it_did_and_failures_are_not_free() {
    let cloud = start();
    let app = funded(&cloud, 1);
    let ep = file(cloud.data.path(), 1 << 20);
    let sha = file_sha256(&ep, |_| true).unwrap();
    let req = SessionRequest { kind: ANIME, size: 1 << 20, sha256: sha, duration_ms: 120_000 };
    let s = app.open_session(&req).unwrap();
    app.upload(&s.id, &ep, |_, _| true).unwrap();
    assert_eq!(app.ready(&s.id).unwrap().price, 20_000);
    // Jobs must be real ones: a cancel of nothing, a job without a file.
    assert_eq!(code(app.send_jobs(&s.id, vec![json!({"op": "cancel", "id": 99})]).unwrap_err()), ErrorCode::BadRequest);
    assert_eq!(code(app.send_jobs(&s.id, vec![json!({"op": "shots", "id": 7})]).unwrap_err()), ErrorCode::BadRequest);
    app.send_jobs(&s.id, vec![json!({"op": "faces", "id": 1, "path": "x", "anime": true, "fake_slow": true})]).unwrap();
    assert_eq!(
        code(app.send_jobs(&s.id, vec![json!({"op": "faces", "id": 1, "path": "x", "anime": true})]).unwrap_err()),
        ErrorCode::BadRequest,
        "an id once"
    );
    until(&app, &s.id, |v| v["event"] == "progress" && v["fraction"] == 0.5);
    let closed = app.close(&s.id).unwrap();
    assert_eq!(closed.charged, 10_000, "half the faces pass, half the price");
    assert_eq!(app.close(&s.id).map(|c| c.charged).unwrap_or(10_000), 10_000, "charged once");
    assert_eq!(closed.account.balance, 1_000_000 - 10_000);
}

#[test]
fn a_scenes_only_session_pays_its_share() {
    let cloud = start();
    let app = funded(&cloud, 1);
    let ep = file(cloud.data.path(), 1 << 20);
    let sha = file_sha256(&ep, |_| true).unwrap();
    let kind = ScanKind { scenes: true, ..ANIME };
    let s = app.open_session(&SessionRequest { kind, size: 1 << 20, sha256: sha, duration_ms: 120_000 }).unwrap();
    app.upload(&s.id, &ep, |_, _| true).unwrap();
    app.ready(&s.id).unwrap();
    app.send_jobs(&s.id, vec![json!({"op": "scenes", "id": 1, "path": "x"})]).unwrap();
    until_done(&app, &s.id, 1);
    assert_eq!(app.close(&s.id).unwrap().charged, 2_000);
}

#[test]
fn closing_while_measuring_ends_the_session_for_good() {
    let cloud = start_with(Box::new(SlowTwoMinutes));
    let app = funded(&cloud, 1);
    let ep = file(cloud.data.path(), 1 << 20);
    let sha = file_sha256(&ep, |_| true).unwrap();
    let req = SessionRequest { kind: ANIME, size: 1 << 20, sha256: sha, duration_ms: 120_000 };
    let s = app.open_session(&req).unwrap();
    app.upload(&s.id, &ep, |_, _| true).unwrap();
    let base = cloud.base.clone();
    let token = app.token().unwrap().to_string();
    let id = s.id.clone();
    let measuring = std::thread::spawn(move || Client::new(base, Some(token)).ready(&id));
    std::thread::sleep(Duration::from_millis(300));
    // A second ready while the first measures is refused: one worker only.
    assert_eq!(code(app.ready(&s.id).unwrap_err()), ErrorCode::BadRequest);
    assert_eq!(app.close(&s.id).unwrap().charged, 0);
    assert_eq!(code(measuring.join().unwrap().unwrap_err()), ErrorCode::Gone);
    // The account isn't stuck on the closed session.
    let again = app.open_session(&req).unwrap();
    assert_ne!(again.id, s.id);
    assert_eq!(app.account().unwrap().balance, 1_000_000 - again.estimate);
}

#[test]
fn a_refund_before_its_payment_waits_for_it() {
    let cloud = start();
    let app = device(&cloud, 1);
    let refund = json!({"id": "evt_r", "type": "charge.refunded", "data": {"object": {"payment_intent": "pi_9", "amount_refunded": 500}}});
    assert_ne!(webhook(&cloud, &refund, WEBHOOK), 200, "Stripe sends it again later");
    let paid = json!({"id": "evt_p", "type": "checkout.session.completed", "data": {"object": {
        "id": "cs_9", "payment_status": "paid", "currency": "eur", "amount_total": 1000,
        "client_reference_id": "1", "payment_intent": "pi_9"}}});
    assert_eq!(webhook(&cloud, &paid, WEBHOOK), 200);
    assert_eq!(webhook(&cloud, &refund, WEBHOOK), 200);
    assert_eq!(app.account().unwrap().balance, 5_000_000);
}

#[test]
fn the_worker_refuses_jobs_that_dont_fit_its_own_copy() {
    use base64::Engine as _;
    let Ok(tools) = as_media::tools::Tools::locate(None) else {
        eprintln!("no usable ffmpeg: skipping");
        return;
    };
    let cloud = start_opts(Box::new(TwoMinutes), true);
    let clip = cloud.data.path().join("clip.mkv");
    let made = std::process::Command::new(&tools.ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=duration=2:size=320x240:rate=24", "-c:v", "libx264"])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(made.success());
    let info = as_media::probe(&tools, &clip).unwrap();
    let index =
        as_media::build_frame_index(&tools, &clip, &info, cloud.data.path(), &as_core::CancelToken::new(), |_| {})
            .unwrap();
    let info = serde_json::to_value(&info).unwrap();
    let index = base64::engine::general_purpose::STANDARD.encode(index.to_bytes());

    let app = funded(&cloud, 1);
    let size = std::fs::metadata(&clip).unwrap().len();
    let sha = file_sha256(&clip, |_| true).unwrap();
    let s = app.open_session(&SessionRequest { kind: ANIME, size, sha256: sha, duration_ms: 2_000 }).unwrap();
    app.upload(&s.id, &clip, |_, _| true).unwrap();
    app.ready(&s.id).unwrap();
    // An index of the app's own making, shorter than the file's: refused.
    let forged = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
    app.send_jobs(
        &s.id,
        vec![json!({"op": "faces", "id": 1, "path": "x", "anime": true, "info": info, "index": forged})],
    )
    .unwrap();
    let mut after = 0;
    let refused = loop {
        let r = app.events(&s.id, after).unwrap();
        after = r.next;
        if let Some(v) = r.lines.iter().map(|l| serde_json::from_str::<Value>(l).unwrap()).find(|v| v["id"] == 1) {
            break v;
        }
        assert!(!r.ended, "{:?}", r.error);
    };
    assert_eq!(refused["event"], "failed");
    assert!(refused["detail"].as_str().unwrap().contains("frame index"), "{refused}");
    // The file's own: it runs.
    app.send_jobs(
        &s.id,
        vec![json!({"op": "faces", "id": 2, "path": "x", "anime": true, "info": info, "index": index})],
    )
    .unwrap();
    until_done(&app, &s.id, 2);
    assert_eq!(app.close(&s.id).unwrap().charged, 20_000);
}

#[test]
fn the_launch_stage_decides_what_the_cloud_takes() {
    // Stripe's test key, and no leave to sell with it: the cloud can't open.
    let test_key = |c: &mut Config| {
        c.stripe.secret_key = "sk_test_for_the_tests".into();
        c.stripe.test_mode_ok = false;
    };
    let cloud = start_cfg(Box::new(TwoMinutes), false, None, test_key);
    let nobody = Client::new(cloud.base.clone(), None);
    assert_eq!(nobody.info().unwrap().stage, Stage::Hidden, "hidden until the owner moves it");
    let reg = RegisterRequest { hwid: format!("{:064x}", 1), platform: "test".into(), app_version: "1.0.6".into() };
    assert_eq!(code(Client::new(cloud.base.clone(), None).register(&reg).unwrap_err()), ErrorCode::Closed);

    // The beta: codes and scans, no payments.
    let (status, stats) = admin_post(&cloud, admin::STAGE, json!({"stage": "beta"}));
    assert_eq!((status, stats["stage"].as_str(), stats["live_payments"].as_bool()), (200, Some("beta"), Some(false)));
    let app = funded(&cloud, 1);
    assert_eq!(app.account().unwrap().stage, Stage::Beta);
    assert_eq!(code(app.checkout(500).unwrap_err()), ErrorCode::Closed);
    let ep = file(cloud.data.path(), 1 << 20);
    let sha = file_sha256(&ep, |_| true).unwrap();
    let req = SessionRequest { kind: ANIME, size: 1 << 20, sha256: sha, duration_ms: 120_000 };
    // A whole scan runs and is paid from the balance an admin gave.
    let s = app.open_session(&req).unwrap();
    app.upload(&s.id, &ep, |_, _| true).unwrap();
    assert_eq!(app.ready(&s.id).unwrap().price, 20_000);
    app.send_jobs(&s.id, vec![json!({"op": "faces", "id": 1, "path": "x", "anime": true})]).unwrap();
    until_done(&app, &s.id, 1);
    assert_eq!(app.close(&s.id).unwrap().charged, 20_000);

    // Opening needs a key that takes real money: refused, the beta goes on.
    let (status, _) = admin_post(&cloud, admin::STAGE, json!({"stage": "open"}));
    assert_eq!(status, 400);
    assert_eq!(nobody.info().unwrap().stage, Stage::Beta);

    // Hidden while a file is uploaded: no GPU starts, nothing is charged.
    let s = app.open_session(&req).unwrap();
    app.upload(&s.id, &ep, |_, _| true).unwrap();
    assert_eq!(admin_post(&cloud, admin::STAGE, json!({"stage": "hidden"})).0, 200);
    assert_eq!(code(app.ready(&s.id).unwrap_err()), ErrorCode::Closed);
    let _ = app.close(&s.id);
    // The account still shows; nothing new is taken.
    let a = app.account().unwrap();
    assert_eq!((a.stage, a.balance), (Stage::Hidden, 1_000_000 - 20_000));
    assert_eq!(code(app.open_session(&req).unwrap_err()), ErrorCode::Closed);
    let (_, made) = admin_post(&cloud, admin::CODES, json!({"worth": {"kind": "credit", "cents": 100}, "count": 1}));
    assert_eq!(code(app.redeem(made["codes"][0].as_str().unwrap()).unwrap_err()), ErrorCode::Closed);
    assert_eq!(code(app.checkout(500).unwrap_err()), ErrorCode::Closed);

    // `open` stored while the key is a test one (swapped after opening): it acts as the beta.
    let swapped = start_cfg(Box::new(TwoMinutes), false, Some("open"), test_key);
    assert_eq!(Client::new(swapped.base.clone(), None).info().unwrap().stage, Stage::Beta);
    assert_eq!(admin_get(&swapped, admin::STATS)["stage"], "beta");
    assert_eq!(code(device(&swapped, 1).checkout(500).unwrap_err()), ErrorCode::Closed);

    // Open, where payments may be sold: the payment page too.
    let open = start();
    assert_eq!(Client::new(open.base.clone(), None).info().unwrap().stage, Stage::Open);
    assert_eq!(device(&open, 1).checkout(500).unwrap(), "https://checkout.test/500");
}
