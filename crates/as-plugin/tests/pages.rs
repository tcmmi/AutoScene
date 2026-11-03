//! The asp scheme as a page's frame asks it: each plugin's files under
//! `ui/`, as its package lists them or its loaded folder holds them, the
//! kit, and the person's files a page was given, with the headers that
//! keep a page to its own session. Each test works in a folder of its own,
//! with a page-only plugin written there and a kit held in memory.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use as_plugin::pages::{
    self, AssetKit, KIT_FILES, MAX_FILE_TOKENS, MAX_SERVED_FILE, MAX_SESSIONS, MemoryKit, Mount, Pages, RANGE_PART,
    Served, base_url, page_csp,
};
use as_plugin::store::RECORD_VERSION;
use as_plugin::{
    Extra, FilesLock, Grants, Installed, LockedFile, PageSpec, PluginError, Record, RuntimeKind, Source, Trust,
    manifest, package,
};
use serde_json::json;

const ANCESTORS: [&str; 2] = ["http://tauri.localhost", "tauri://localhost"];
const PAGE: &[u8] = b"<!doctype html><link rel=stylesheet href=style.css><script src=app.js></script>";

/// A page-only plugin `id`'s files: its page, what the page loads, a file
/// of a type no page is served, and one outside `ui/`.
fn files(id: &str) -> Vec<(String, Vec<u8>)> {
    let manifest = json!({
        "manifestVersion": 2,
        "id": id,
        "version": "1.0.0",
        "name": "Hello Page",
        "description": "Shows a page.",
        "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.0.0", "protocol": 2},
        "runtime": {"type": "none"},
        "contributes": {"pages": [
            {"id": "main", "title": "Main", "entry": "ui/index.html", "rail": true},
            {"id": "deep", "title": "Deep", "entry": "ui/more/deep.html"}
        ]}
    });
    [
        ("plugin.json", serde_json::to_vec(&manifest).unwrap()),
        ("ui/index.html", PAGE.to_vec()),
        ("ui/more/deep.html", b"<p>deep</p>".to_vec()),
        ("ui/app.js", b"document.body.append('hi')".to_vec()),
        ("ui/style.css", b"body { color: red }".to_vec()),
        ("ui/data.bin", vec![0, 1, 2]),
        ("notes.txt", b"not a page's".to_vec()),
    ]
    .into_iter()
    .map(|(path, bytes)| (path.to_owned(), bytes))
    .collect()
}

/// Writes `files` into `dir`.
fn write(dir: &Path, files: &[(String, Vec<u8>)]) {
    for (path, bytes) in files {
        let file = package::file_in(dir, path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }
}

/// The plugin of `files` in `dir`, as its install left it, of `source`;
/// `lock_sha256` for a package.
fn installed(dir: &Path, files: &[(String, Vec<u8>)], source: Source, lock_sha256: Option<String>) -> Installed {
    let m = manifest::parse_for_any_app(&files[0].1).unwrap().manifest;
    Installed {
        record: Record {
            v: RECORD_VERSION,
            id: m.id.clone(),
            version: m.version.clone(),
            source,
            trust: Trust::Reviewed,
            author_key: None,
            permissions: Vec::new(),
            network_hosts: Vec::new(),
            enabled: true,
            after_export: Vec::new(),
            lock_sha256,
            installed_at: 0,
        },
        manifest: Some(m),
        warnings: Vec::new(),
        dir: dir.to_path_buf(),
        broken: None,
        extra: Extra::agreed(RuntimeKind::None),
    }
}

/// The plugin `id` installed from a package into `dir`: its files, and the
/// lock that lists all but `unlisted` of them.
fn package_in(dir: &Path, id: &str, unlisted: &[&str]) -> Installed {
    package_of(dir, files(id), unlisted)
}

/// The plugin of `files` installed from a package into `dir`, with the lock
/// that lists all but `unlisted` of them.
fn package_of(dir: &Path, files: Vec<(String, Vec<u8>)>, unlisted: &[&str]) -> Installed {
    write(dir, &files);
    let listed = files.iter().filter(|(path, _)| !unlisted.contains(&path.as_str()));
    let lock = FilesLock {
        lock_version: 1,
        files: listed
            .map(|(path, bytes)| LockedFile {
                path: path.clone(),
                size: bytes.len() as u64,
                sha256: package::sha256_hex(bytes),
                exec: false,
            })
            .collect(),
    };
    let lock = serde_json::to_vec_pretty(&lock).unwrap();
    fs::write(dir.join(package::LOCK), &lock).unwrap();
    let source = Source::Package { folder: "1.0.0+0123abcd".into() };
    installed(dir, &files, source, Some(package::sha256_hex(&lock)))
}

/// The plugin `id` loaded from its folder `dir`.
fn folder_in(dir: &Path, id: &str) -> Installed {
    let files = files(id);
    write(dir, &files);
    installed(dir, &files, Source::Folder { path: dir.display().to_string() }, None)
}

fn kit() -> MemoryKit {
    MemoryKit(HashMap::from([("sdk.js".into(), b"// sdk".to_vec()), ("theme.css".into(), b"/* theme */".to_vec())]))
}

fn pages() -> Pages {
    Pages::new(Arc::new(kit()), ANCESTORS.map(String::from).to_vec())
}

/// Opens the page `id` of `p`; gives its session, and the tokens of those
/// it closed.
fn open_closing(pages: &Pages, p: &Installed, id: &str) -> (Arc<Mount>, Vec<String>) {
    let page = p.manifest.as_ref().unwrap().page(id).unwrap().clone();
    pages.open(p, Grants::of(p).unwrap(), &page, None).unwrap()
}

/// Opens the page `id` of `p`.
fn open(pages: &Pages, p: &Installed, id: &str) -> Arc<Mount> {
    open_closing(pages, p, id).0
}

/// The path of the scheme's address `url`, as the handler sees it.
fn path(url: &str) -> &str {
    url.strip_prefix(base_url()).unwrap()
}

fn get(pages: &Pages, path: &str) -> Served {
    pages.serve("GET", path, None)
}

/// The status of a GET of `path` in the session of `m`.
fn status(pages: &Pages, m: &Mount, file: &str) -> u16 {
    get(pages, &format!("/{}/{file}", m.token)).status
}

/// Links `link` to the folder `target`: a symlink on Linux and macOS, a
/// junction on Windows, which needs no administrator (its symlinks do).
fn link_folder(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // mklink takes a "/" in a path for an option.
        let (link, target): (std::path::PathBuf, std::path::PathBuf) =
            (link.components().collect(), target.components().collect());
        let made =
            std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    }
}

/// Links `link` to the file `target`, where this computer lets a test:
/// Linux and macOS always, Windows with Developer Mode or as an
/// administrator. Whether it did.
fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = {
        std::os::unix::fs::symlink(target, link).unwrap();
        true
    };
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link).is_ok();
    made
}

#[test]
fn only_listed_files_under_ui_are_served() {
    let tmp = tempfile::tempdir().unwrap();
    let p = package_in(tmp.path(), "dev.autoscene.hello-page", &["ui/style.css"]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    assert_eq!(m.src(), format!("{}/{}/ui/index.html", base_url(), m.token));
    assert_eq!((m.token.len(), m.trust, m.name.as_str()), (32, Trust::Reviewed, "Hello Page"));

    let page = get(&pages, path(&m.src()));
    assert_eq!((page.status, page.body.as_slice()), (200, PAGE));
    assert_eq!(page.header("content-type"), Some("text/html; charset=utf-8"));
    assert_eq!(page.header("content-length"), Some(PAGE.len().to_string().as_str()));
    let script = get(&pages, &format!("/{}/ui/app.js", m.token));
    assert_eq!((script.status, script.header("Content-Type")), (200, Some("text/javascript; charset=utf-8")));
    assert_eq!(status(&pages, &m, "ui/more/deep.html"), 200);
    // HEAD answers as GET, without the body.
    let head = pages.serve("HEAD", path(&m.src()), None);
    assert_eq!((head.status, head.body.len()), (200, 0));
    assert_eq!(head.header("content-length"), Some(PAGE.len().to_string().as_str()));

    // On disk but not in the lock; outside ui/; the package's own files; a
    // type no page is served; a file that isn't there.
    for file in ["ui/style.css", "notes.txt", "plugin.json", "files.lock", "ui/data.bin", "ui/gone.js", "ui", "ui/"] {
        assert_eq!(status(&pages, &m, file), 404, "{file}");
    }
    // A query or a fragment is no part of the file.
    assert_eq!(status(&pages, &m, "ui/app.js?v=2#top"), 200);
    // A page whose file the lock doesn't list doesn't open.
    let unlisted = PageSpec { entry: "ui/style.html".into(), ..p.manifest.as_ref().unwrap().pages[0].clone() };
    let Err(PluginError::Invalid(said)) = pages.open(&p, Grants::of(&p).unwrap(), &unlisted, None) else { panic!() };
    assert_eq!(said, "The plugin package is missing its page ui/style.html.");
    // Nor one whose lock isn't the one its install recorded.
    let mut other = p.clone();
    other.record.lock_sha256 = Some("0".repeat(64));
    let page = other.manifest.as_ref().unwrap().pages[0].clone();
    let Err(PluginError::Invalid(said)) = pages.open(&other, Grants::of(&other).unwrap(), &page, None) else {
        panic!()
    };
    assert_eq!(said, "Its files were changed. Install it again.");
}

#[test]
fn a_changed_file_is_not_served() {
    let tmp = tempfile::tempdir().unwrap();
    let p = package_in(tmp.path(), "dev.autoscene.hello-page", &[]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    let app = tmp.path().join("ui/app.js");
    let bytes = fs::read(&app).unwrap();
    assert_eq!(status(&pages, &m, "ui/app.js"), 200);

    // Of its size, with other bytes; shorter; longer; gone.
    let mut same_size = bytes.clone();
    same_size[0] = b'X';
    for changed in [same_size, bytes[1..].to_vec(), [&bytes[..], b";fetch('x')"].concat()] {
        fs::write(&app, &changed).unwrap();
        assert_eq!(status(&pages, &m, "ui/app.js"), 404);
    }
    fs::remove_file(&app).unwrap();
    assert_eq!(status(&pages, &m, "ui/app.js"), 404);
    // As it was, it is served again.
    fs::write(&app, &bytes).unwrap();
    let served = get(&pages, &format!("/{}/ui/app.js", m.token));
    assert_eq!((served.status, served.body), (200, bytes));
}

#[test]
fn a_page_file_answers_the_one_range_it_is_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let mut files = files("dev.autoscene.hello-page");
    let len = 3 * RANGE_PART + 7;
    let video: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    files.push(("ui/intro.mp4".into(), video.clone()));
    let p = package_of(tmp.path(), files, &[]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    let intro = format!("/{}/ui/intro.mp4", m.token);
    let range = |r: &str| pages.serve("GET", &intro, Some(r));
    let part = |from: u64, to: u64| video[from as usize..to as usize].to_vec();

    // Whole without a range, which it says it takes.
    let whole = get(&pages, &intro);
    assert_eq!((whole.status, whole.body == video, whole.header("content-range")), (200, true, None));
    assert_eq!((whole.header("content-type"), whole.header("accept-ranges")), (Some("video/mp4"), Some("bytes")));
    // All a range asks, from the bytes that were checked: a page's file is
    // read whole for its lock, so it is never cut to RANGE_PART.
    let all = range("bytes=0-");
    assert_eq!((all.status, all.body == video), (206, true));
    assert_eq!(all.header("content-range"), Some(format!("bytes 0-{}/{len}", len - 1).as_str()));
    assert_eq!(all.header("content-length"), Some(len.to_string().as_str()));
    let probe = range("bytes=0-1");
    assert_eq!((probe.status, probe.header("content-range")), (206, Some("bytes 0-1/3145735")));
    assert_eq!(probe.body, part(0, 2));
    assert_eq!(range("bytes=-7").body, part(len - 7, len));
    assert_eq!(range("bytes=1048576-").body, part(RANGE_PART, len));
    // Several, or none in the file.
    for refused in ["bytes=0-1,5-9", "bytes=3145735-", "bytes=9-5", "lines=1-2"] {
        let served = range(refused);
        assert_eq!((served.status, served.body.len()), (416, 0), "{refused}");
        assert_eq!(served.header("content-range"), Some("bytes */3145735"), "{refused}");
    }
    // Changed on disk, no part of it is served.
    fs::write(tmp.path().join("ui/intro.mp4"), [&video[..len as usize - 1], b"x"].concat()).unwrap();
    assert_eq!(range("bytes=0-1").status, 404);

    // A loaded folder's file too.
    let dir = tmp.path().join("dev");
    let p = folder_in(&dir, "dev.autoscene.hello-page");
    let m = open(&pages, &p, "main");
    let served = pages.serve("GET", &format!("/{}/ui/app.js", m.token), Some("bytes=9-"));
    assert_eq!((served.status, served.body.as_slice()), (206, b"body.append('hi')".as_slice()));
}

#[test]
fn a_listed_file_is_served_up_to_32_mb() {
    let tmp = tempfile::tempdir().unwrap();
    let mut files = files("dev.autoscene.hello-page");
    let largest = MAX_SERVED_FILE as usize;
    files.push(("ui/largest.js".into(), vec![b' '; largest]));
    files.push(("ui/too-large.js".into(), vec![b' '; largest + 1]));
    let p = package_of(tmp.path(), files, &[]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    // Both are as the lock lists them; only the one of 32 MB is served.
    let served = get(&pages, &format!("/{}/ui/largest.js", m.token));
    assert_eq!((served.status, served.body.len()), (200, largest));
    assert_eq!(status(&pages, &m, "ui/too-large.js"), 404);
}

#[test]
fn every_response_has_the_page_csp_nosniff_and_acao() {
    let tmp = tempfile::tempdir().unwrap();
    let p = package_in(tmp.path(), "dev.autoscene.hello-page", &[]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    let clip = tmp.path().join("clip.mp4");
    fs::write(&clip, vec![7u8; 100]).unwrap();
    let file = pages.resolve(&m, clip).unwrap();
    let ancestors = ANCESTORS.map(String::from);
    let own = page_csp(base_url(), &m.token, &ancestors);

    let session = [
        ("GET", path(&m.src()).to_owned(), None, 200),
        ("GET", format!("/{}/ui/gone.html", m.token), None, 404),
        ("GET", format!("/{}/notes.txt", m.token), None, 404),
        ("POST", path(&m.src()).to_owned(), None, 405),
        ("GET", path(&file).to_owned(), Some("bytes=0-9"), 206),
        ("GET", path(&file).to_owned(), Some("bytes=500-"), 416),
        ("GET", format!("/{}/~f/{}", m.token, "f".repeat(32)), None, 404),
    ];
    for (method, at, range, want) in session {
        let served = pages.serve(method, &at, range);
        assert_eq!(served.status, want, "{method} {at}");
        assert_eq!(served.header("content-security-policy"), Some(own.as_str()), "{method} {at}");
    }
    // Outside a session, the CSP allows nothing at all.
    let none =
        format!("default-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors {}", ANCESTORS.join(" "));
    let elsewhere = [
        pages.serve("GET", "/_as/sdk.js", None),
        pages.serve("GET", "/_as/kit.css", None),
        pages.serve("GET", "/", None),
        pages.serve("GET", "/%zz", None),
        pages.serve("DELETE", "/x", None),
        pages::refusal(403, &ancestors),
        pages::refusal(503, &ancestors),
    ];
    let mut all: Vec<Served> = session_answers(&pages, &m, &file);
    for served in &elsewhere {
        assert_eq!(served.header("content-security-policy"), Some(none.as_str()), "{}", served.status);
    }
    all.extend(elsewhere);
    // An unknown session's address names that session's path, which serves nothing.
    let unknown = "0123456789abcdef0123456789abcdef";
    let served = get(&pages, &format!("/{unknown}/ui/index.html"));
    assert_eq!(served.header("content-security-policy"), Some(page_csp(base_url(), unknown, &ancestors).as_str()));
    all.push(served);

    for served in &all {
        let said = served.status;
        assert!(
            served.header("content-security-policy").is_some_and(|c| c.starts_with("default-src 'none'")),
            "{said}"
        );
        assert_eq!(served.header("x-content-type-options"), Some("nosniff"), "{said}");
        assert_eq!(served.header("access-control-allow-origin"), Some("*"), "{said}");
        assert_eq!(served.header("cross-origin-resource-policy"), Some("cross-origin"), "{said}");
        assert_eq!(served.header("cache-control"), Some("no-store"), "{said}");
        assert_eq!(served.header("referrer-policy"), Some("no-referrer"), "{said}");
        let permissions = served.header("permissions-policy").unwrap();
        for denied in [
            "camera=()",
            "microphone=()",
            "clipboard-read=()",
            "clipboard-write=()",
            "payment=()",
            "picture-in-picture=()",
        ] {
            assert!(permissions.contains(denied), "{said}: {denied}");
        }
        assert_eq!(served.header("content-length"), Some(served.body.len().to_string().as_str()), "{said}");
    }
    let refused = pages.serve("POST", path(&m.src()), None);
    assert_eq!((refused.header("allow"), refused.body.len()), (Some("GET, HEAD"), 0));
}

/// What the session `m` answers: its page, a file of it, a refused one.
fn session_answers(pages: &Pages, m: &Mount, file: &str) -> Vec<Served> {
    vec![
        get(pages, path(&m.src())),
        get(pages, &format!("/{}/ui/style.css", m.token)),
        get(pages, &format!("/{}/plugin.json", m.token)),
        pages.serve("GET", path(file), Some("bytes=10-")),
        pages.serve("GET", path(file), Some("bytes=10-1")),
    ]
}

#[test]
fn the_csp_names_the_session_path_never_self() {
    let token = "0123456789abcdef0123456789abcdef";
    let csp = page_csp("http://asp.localhost", token, &["http://tauri.localhost".into()]);
    assert_eq!(
        csp,
        "default-src 'none'; \
         script-src http://asp.localhost/0123456789abcdef0123456789abcdef/ http://asp.localhost/_as/; \
         style-src http://asp.localhost/0123456789abcdef0123456789abcdef/ http://asp.localhost/_as/ 'unsafe-inline'; \
         img-src http://asp.localhost/0123456789abcdef0123456789abcdef/ data: blob:; \
         font-src http://asp.localhost/0123456789abcdef0123456789abcdef/ http://asp.localhost/_as/ data:; \
         media-src http://asp.localhost/0123456789abcdef0123456789abcdef/ blob:; \
         connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; manifest-src 'none'; \
         object-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts allow-forms; \
         frame-ancestors http://tauri.localhost"
    );
    // The page's own answer sandboxes it as its frame does, never to its origin.
    assert!(csp.contains(&format!("; sandbox {}; ", pages::SANDBOX)), "{csp}");
    // 'self' would be every plugin's files; no script runs from text or eval.
    for never in ["'self'", "'unsafe-eval'", "'wasm-unsafe-eval'", "*", "http:", "https:"] {
        assert!(!csp.split([' ', ';']).any(|s| s == never), "{never}");
    }
    // Nothing frames a page when nothing is named.
    assert!(page_csp("asp://localhost", token, &[]).ends_with("frame-ancestors 'none'"));

    // Two sessions, of two plugins: each page's CSP names its own path, not the other's.
    let tmp = tempfile::tempdir().unwrap();
    let (a_dir, b_dir) = (tmp.path().join("a"), tmp.path().join("b"));
    let pages = pages();
    let a = open(&pages, &package_in(&a_dir, "dev.autoscene.page-a", &[]), "main");
    let b = open(&pages, &folder_in(&b_dir, "dev.autoscene.page-b"), "main");
    for (m, other) in [(&a, &b), (&b, &a)] {
        let csp = get(&pages, path(&m.src())).header("content-security-policy").unwrap().to_owned();
        assert!(csp.contains(&format!("script-src {}/{}/ ", base_url(), m.token)), "{csp}");
        assert!(!csp.contains(&other.token), "{csp}");
    }
    // A session serves only its own plugin's files.
    assert_eq!(get(&pages, &format!("/{}/../{}/ui/index.html", a.token, b.token)).status, 404);
}

#[test]
fn the_mock_csp_is_the_page_csp() {
    // The browser mock serves pages from an origin of their own, framed by
    // Vite's, with the CSP this file holds (app/vite.mock-asp.ts, whose own
    // test checks that it sends it): the page CSP, as Rust makes it.
    let golden = include_str!("../../../app/src/plugin-kit/page-csp.golden.txt");
    let csp = page_csp("http://localhost:5174", "0123456789abcdef0123456789abcdef", &["http://localhost:5173".into()]);
    assert_eq!(golden.trim_end(), csp, "write the golden file again from page_csp");
}

#[test]
fn paths_that_climb_or_alias_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let pages = pages();
    let package = open(&pages, &package_in(&tmp.path().join("package"), "dev.autoscene.package", &[]), "main");
    let folder = open(&pages, &folder_in(&tmp.path().join("folder"), "dev.autoscene.folder"), "main");
    fs::write(tmp.path().join("secret.html"), b"<p>the person's</p>").unwrap();
    for m in [&package, &folder] {
        assert_eq!(status(&pages, m, "ui/index.html"), 200);
        for file in [
            "ui/../plugin.json",
            "ui/../../secret.html",
            "ui/%2e%2e/plugin.json",
            "ui/%2E%2E/%2e%2e/secret.html",
            "ui%2f..%2fplugin.json",
            r"ui\index.html",
            "ui%5cindex.html",
            "ui/./index.html",
            "ui/%2e/index.html",
            "UI/index.html",
            "Ui/index.html",
            "ui//index.html",
            "ui/index.html.",
            "ui/index.html%00",
            "ui/INDEX~1.HTM",
            "ui/index.html::$DATA",
            "ui/%zz",
            "ui/%e9.html",
            "~f/../ui/index.html",
            "~f/%2e%2e/ui/index.html",
        ] {
            assert_eq!(status(&pages, m, file), 404, "{file}");
        }
    }
    // On a system that takes `Index.html` for `index.html`, a package's file
    // is still served only as its lock spells it.
    assert_eq!(status(&pages, &package, "ui/Index.html"), 404);
    // Nothing reaches outside the scheme's paths: no session, no kit file.
    for at in ["/../secret.html", "/_as/../secret.html", "/_as/%2e%2e/secret.html", "//ui/index.html", "ui/index.html"]
    {
        assert_eq!(get(&pages, at).status, 404, "{at}");
    }
}

#[test]
fn an_unknown_or_closed_session_is_404() {
    let tmp = tempfile::tempdir().unwrap();
    let (a_dir, b_dir) = (tmp.path().join("a"), tmp.path().join("b"));
    let (a, b) = (package_in(&a_dir, "dev.autoscene.page-a", &[]), folder_in(&b_dir, "dev.autoscene.page-b"));
    let pages = pages();
    let first = open(&pages, &a, "main");
    let second = open(&pages, &a, "deep");
    let other = open(&pages, &b, "main");
    assert_eq!((pages.open_count(&a.record.id), pages.open_count(&b.record.id)), (2, 1));
    assert_ne!(first.token, second.token);
    for unknown in ["0123456789abcdef0123456789abcdef", "main", "dev.autoscene.page-a", "", "_as"] {
        assert_eq!(get(&pages, &format!("/{unknown}/ui/index.html")).status, 404, "{unknown}");
    }

    pages.close(&first.token);
    assert_eq!(status(&pages, &first, "ui/index.html"), 404);
    assert!(pages.get(&first.token).is_none());
    // What it still had waiting hears it closed; the others' don't.
    assert!(first.closed.is_cancelled());
    assert!(!second.closed.is_cancelled());
    assert_eq!(status(&pages, &second, "ui/more/deep.html"), 200);
    // Closing one that is closed already is no error.
    pages.close(&first.token);

    assert_eq!(pages.close_plugin(&a.record.id), std::slice::from_ref(&second.token));
    assert_eq!(status(&pages, &second, "ui/more/deep.html"), 404);
    assert_eq!(pages.open_count(&a.record.id), 0);
    assert_eq!(status(&pages, &other, "ui/index.html"), 200);
    assert_eq!(pages.close_where(|m| m.trust != Trust::Reviewed), std::slice::from_ref(&other.token));
    assert_eq!(status(&pages, &other, "ui/index.html"), 404);
    assert!(second.closed.is_cancelled() && other.closed.is_cancelled());
}

#[test]
fn the_kit_serves_its_two_files_and_nothing_else() {
    let pages = pages();
    let sdk = get(&pages, "/_as/sdk.js");
    assert_eq!((sdk.status, sdk.body.as_slice()), (200, b"// sdk".as_slice()));
    assert_eq!(sdk.header("content-type"), Some("text/javascript; charset=utf-8"));
    let theme = get(&pages, "/_as/theme.css");
    assert_eq!((theme.status, theme.header("content-type")), (200, Some("text/css; charset=utf-8")));
    assert_eq!(KIT_FILES, ["sdk.js", "theme.css"]);
    for other in ["kit.css", "index.html", "sdk.js.map", "Sdk.js", "", "x/sdk.js", "../sdk.js"] {
        assert_eq!(get(&pages, &format!("/_as/{other}")).status, 404, "{other}");
    }
    // A kit without the file (a dev build before its kit was built).
    let empty = Pages::new(Arc::new(MemoryKit::default()), Vec::new());
    assert_eq!(get(&empty, "/_as/sdk.js").status, 404);

    // The app's asset resolver answers a path it doesn't have with the
    // app's own index.html: that is no kit file.
    let app_html = b"<!doctype html><script src=/assets/index.js></script>".to_vec();
    let falls_back = AssetKit(move |_: &str| Some((app_html.clone(), "text/html".to_owned())));
    let fallen = Pages::new(Arc::new(falls_back), Vec::new());
    for name in KIT_FILES {
        assert_eq!(get(&fallen, &format!("/_as/{name}")).status, 404, "{name}");
    }
    let built = AssetKit(|name: &str| {
        let kind = if name.ends_with(".js") { "text/javascript" } else { "text/css" };
        Some((format!("/* {name} */").into_bytes(), kind.to_owned()))
    });
    let built = Pages::new(Arc::new(built), Vec::new());
    assert_eq!(get(&built, "/_as/theme.css").body, b"/* theme.css */");
    // `tauri dev` reads any path it is asked for under dist/, also one that
    // climbs out: a kit that answers every name still gives only its two.
    for other in ["kit.css", "index.html", "sdk.js.map", "Sdk.js", "x/sdk.js", "../x.js", "..%2Fx.js", "%2e%2e/sdk.js"]
    {
        assert_eq!(get(&built, &format!("/_as/{other}")).status, 404, "{other}");
    }
    // As a HTML page whatever its case, or with a charset.
    let shouting =
        Pages::new(Arc::new(AssetKit(|_: &str| Some((b"x".to_vec(), "Text/HTML; charset=utf-8".into())))), vec![]);
    assert_eq!(get(&shouting, "/_as/sdk.js").status, 404);
}

#[test]
fn a_resolved_file_answers_ranges_in_1_mib_parts() {
    let tmp = tempfile::tempdir().unwrap();
    let p = package_in(&tmp.path().join("plugin"), "dev.autoscene.hello-page", &[]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    let len = 2 * RANGE_PART + RANGE_PART / 2;
    let bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    let clip = tmp.path().join("Jesse 001.mp4");
    fs::write(&clip, &bytes).unwrap();
    let url = pages.resolve(&m, clip.clone()).unwrap();
    assert!(url.starts_with(&format!("{}/{}/~f/", base_url(), m.token)), "{url}");
    let range = |r: Option<&str>| pages.serve("GET", path(&url), r);
    let part = |from: u64, to: u64| bytes[from as usize..to as usize].to_vec();
    let mib = RANGE_PART;

    // Asked for whole, a large file comes in its first part.
    let first = range(None);
    assert_eq!((first.status, first.body.len() as u64), (206, mib));
    assert_eq!(first.header("content-range"), Some(format!("bytes 0-{}/{len}", mib - 1).as_str()));
    assert_eq!((first.header("content-type"), first.header("accept-ranges")), (Some("video/mp4"), Some("bytes")));
    let open_end = range(Some("bytes=1048576-"));
    assert_eq!(open_end.header("content-range"), Some(format!("bytes {mib}-{}/{len}", 2 * mib - 1).as_str()));
    assert_eq!((open_end.status, open_end.body), (206, part(mib, 2 * mib)));
    let tail = range(Some("bytes=-100"));
    assert_eq!((tail.status, tail.body), (206, part(len - 100, len)));
    let some = range(Some("bytes=10-19"));
    assert_eq!(some.header("content-range"), Some("bytes 10-19/2621440"));
    assert_eq!((some.status, some.body), (206, part(10, 20)));
    let wide = range(Some("bytes=0-2621439"));
    assert_eq!(wide.body.len() as u64, mib);
    // Several ranges, or none in the file.
    for refused in ["bytes=0-1,5-9", "bytes=2621440-", "bytes=9-5", "bytes=-0", "lines=1-2"] {
        let served = range(Some(refused));
        assert_eq!((served.status, served.body.len()), (416, 0), "{refused}");
        assert_eq!(served.header("content-range"), Some("bytes */2621440"), "{refused}");
    }
    // A small one comes whole.
    let short = tmp.path().join("short.mp3");
    fs::write(&short, b"ID3 small").unwrap();
    let small = pages.serve("GET", path(&pages.resolve(&m, short).unwrap()), None);
    assert_eq!(
        (small.status, small.body.as_slice(), small.header("content-range")),
        (200, b"ID3 small".as_slice(), None)
    );
    // A picture asked for whole comes whole, as <img> never asks for a part;
    // asked for a part, it comes in one.
    let still = tmp.path().join("still.png");
    fs::write(&still, &bytes).unwrap();
    let picture = pages.resolve(&m, still.clone()).unwrap();
    let whole = pages.serve("GET", path(&picture), None);
    assert_eq!(
        (whole.status, whole.header("content-range"), whole.header("accept-ranges")),
        (200, None, Some("bytes"))
    );
    assert_eq!((whole.header("content-type"), whole.body == bytes), (Some("image/png"), true));
    assert_eq!(pages.serve("GET", path(&picture), Some("bytes=0-9")).body, part(0, 10));
    // Up to 32 MB: a larger one isn't given to a page, nor served once it grew so.
    let large = fs::File::create(tmp.path().join("large.png")).unwrap();
    large.set_len(MAX_SERVED_FILE + 1).unwrap();
    let said = pages.resolve(&m, tmp.path().join("large.png")).unwrap_err();
    assert!(matches!(&said, PluginError::Invalid(s) if s == "A page can show pictures of up to 32 MB."), "{said:?}");
    fs::File::options().write(true).open(&still).unwrap().set_len(MAX_SERVED_FILE + 1).unwrap();
    assert_eq!(pages.serve("GET", path(&picture), None).status, 404);
    assert_eq!(pages.serve("GET", path(&picture), Some("bytes=0-9")).status, 206);
    drop(large);

    // Only pictures, video and sound, of the person's files that are there.
    let notes = tmp.path().join("notes.txt");
    fs::write(&notes, b"x").unwrap();
    for refused in [notes, tmp.path().join("page.svg"), tmp.path().join("plugin").join("ui/index.html")] {
        let said = pages.resolve(&m, refused).unwrap_err();
        assert!(matches!(&said, PluginError::Denied(s) if s == "A page can show only pictures, video and sound."));
    }
    let said = pages.resolve(&m, tmp.path().join("gone.mp4")).unwrap_err();
    assert!(matches!(&said, PluginError::Invalid(s) if s == "That file isn't where it was."), "{said:?}");
    // A link is refused as one, where links to files can be made: its file
    // is there, and a link is never served.
    let linked = tmp.path().join("linked.mp4");
    if link_file(&clip, &linked) {
        let said = pages.resolve(&m, linked).unwrap_err();
        assert!(matches!(&said, PluginError::Denied(s) if s == "A page can't show a file through a link."), "{said:?}");
    }
    // Gone after it was resolved.
    fs::remove_file(&clip).unwrap();
    assert_eq!(range(None).status, 404);
    fs::write(&clip, &bytes).unwrap();
    assert_eq!(range(Some("bytes=0-0")).body, [0]);

    // A file token is its session's: another session, or a closed one, doesn't serve it.
    let other = open(&pages, &p, "main");
    let token = url.rsplit('/').next().unwrap();
    assert_eq!(get(&pages, &format!("/{}/~f/{token}", other.token)).status, 404);
    // Past MAX_FILE_TOKENS the first is forgotten.
    let tokens: Vec<String> = (0..MAX_FILE_TOKENS).map(|_| pages.resolve(&m, clip.clone()).unwrap()).collect();
    assert_eq!(range(None).status, 404);
    assert_eq!(pages.serve("GET", path(&tokens[0]), None).status, 206);
    pages.close(&m.token);
    assert_eq!(pages.serve("GET", path(&tokens[1]), None).status, 404);
}

#[test]
fn a_folder_plugin_is_served_as_its_files_are_and_never_through_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("dev");
    let p = folder_in(&dir, "dev.autoscene.hello-page");
    let pages = pages();
    let m = open(&pages, &p, "main");
    assert_eq!((m.trust, m.lock.is_none()), (Trust::Unreviewed, true), "a loaded folder is never reviewed");
    // As its files are now: changed, added, gone.
    fs::write(dir.join("ui/app.js"), b"changed()").unwrap();
    assert_eq!(get(&pages, &format!("/{}/ui/app.js", m.token)).body, b"changed()");
    fs::write(dir.join("ui/new.css"), b"p {}").unwrap();
    assert_eq!(status(&pages, &m, "ui/new.css"), 200);
    fs::remove_file(dir.join("ui/new.css")).unwrap();
    assert_eq!(status(&pages, &m, "ui/new.css"), 404);
    // Still only under ui/, of a type a page is served, and not too large.
    assert_eq!((status(&pages, &m, "notes.txt"), status(&pages, &m, "ui/data.bin")), (404, 404));
    let large = fs::File::create(dir.join("ui/large.js")).unwrap();
    large.set_len(MAX_SERVED_FILE + 1).unwrap();
    assert_eq!(status(&pages, &m, "ui/large.js"), 404);
    large.set_len(MAX_SERVED_FILE).unwrap();
    assert_eq!(status(&pages, &m, "ui/large.js"), 200);
    drop(large);
    // A folder of a file's name isn't one.
    fs::create_dir(dir.join("ui/folder.js")).unwrap();
    assert_eq!(status(&pages, &m, "ui/folder.js"), 404);

    // A link to a folder elsewhere, on the way to a file.
    let elsewhere = tmp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("page.html"), b"<p>the person's</p>").unwrap();
    link_folder(&elsewhere, &dir.join("ui/linked"));
    assert!(dir.join("ui/linked/page.html").is_file(), "the link leads there");
    assert_eq!(status(&pages, &m, "ui/linked/page.html"), 404);
    // Nor does a page open through one.
    let linked = PageSpec { entry: "ui/linked/page.html".into(), ..m.page.clone() };
    let Err(PluginError::Invalid(said)) = pages.open(&p, Grants::of(&p).unwrap(), &linked, None) else { panic!() };
    assert_eq!(said, "The plugin folder is missing its page ui/linked/page.html, or it is a link.");
    // ui itself a link.
    let moved = tmp.path().join("ui-elsewhere");
    fs::rename(dir.join("ui"), &moved).unwrap();
    link_folder(&moved, &dir.join("ui"));
    assert!(dir.join("ui/index.html").is_file());
    assert_eq!(status(&pages, &m, "ui/index.html"), 404);
    // A linked file, where links to files can be made.
    #[cfg(unix)]
    {
        fs::remove_file(dir.join("ui")).unwrap();
        fs::rename(&moved, dir.join("ui")).unwrap();
        assert_eq!(status(&pages, &m, "ui/index.html"), 200);
        std::os::unix::fs::symlink(elsewhere.join("page.html"), dir.join("ui/page.html")).unwrap();
        assert_eq!(status(&pages, &m, "ui/page.html"), 404);
    }
}

#[test]
fn post_and_other_methods_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let p = package_in(tmp.path(), "dev.autoscene.hello-page", &[]);
    let pages = pages();
    let m = open(&pages, &p, "main");
    for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "CONNECT", "get", "Head", ""] {
        for at in [path(&m.src()), "/_as/sdk.js"] {
            let served = pages.serve(method, at, None);
            assert_eq!((served.status, served.body.len()), (405, 0), "{method} {at}");
            assert_eq!(served.header("allow"), Some("GET, HEAD"));
        }
    }
    assert_eq!(get(&pages, path(&m.src())).status, 200);
}

#[test]
fn sessions_past_eight_drop_the_oldest() {
    let tmp = tempfile::tempdir().unwrap();
    let p = package_in(tmp.path(), "dev.autoscene.hello-page", &[]);
    let pages = pages();
    let opened: Vec<Arc<Mount>> = (0..MAX_SESSIONS).map(|_| open(&pages, &p, "main")).collect();
    assert_eq!(pages.open_count(&p.record.id), MAX_SESSIONS);
    let (newest, closed) = open_closing(&pages, &p, "deep");
    assert_eq!(pages.open_count(&p.record.id), MAX_SESSIONS);
    // The one opened first is closed, and said so; the others, and the new
    // one, serve.
    assert_eq!(closed, std::slice::from_ref(&opened[0].token));
    assert!(pages.get(&opened[0].token).is_none());
    assert!(opened[0].closed.is_cancelled() && !opened[1].closed.is_cancelled());
    assert_eq!(status(&pages, &opened[0], "ui/index.html"), 404);
    for m in &opened[1..] {
        assert_eq!(status(&pages, m, "ui/index.html"), 200);
    }
    assert_eq!(status(&pages, &newest, "ui/more/deep.html"), 200);
    let (after, closed) = open_closing(&pages, &p, "main");
    assert_eq!(closed, std::slice::from_ref(&opened[1].token));
    assert!(pages.get(&opened[1].token).is_none());
    assert!(pages.get(&after.token).is_some());
}
