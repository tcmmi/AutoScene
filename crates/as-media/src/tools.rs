//! Locating and launching ffmpeg/ffprobe.
//!
//! An app names the folder of the ffmpeg it ships with, and only that one is
//! used: a stale `AUTOSCENE_FFMPEG_DIR` or another ffmpeg on `PATH` never
//! stands in for the pinned build (when it's gone, the tools are missing and
//! a reinstall brings them back). Without one (tests, examples, dev builds
//! without fetched sidecars) they come from `AUTOSCENE_FFMPEG_DIR`, then
//! `PATH`. Every process is spawned without a console window (a GUI app
//! spawning console tools would otherwise flash windows on Windows).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use crate::caps::Caps;
use crate::{MediaError, MediaResult};

#[derive(Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub version: String,
    caps: OnceLock<Caps>,
}

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// The demuxers of the containers AutoScene imports (`VIDEO_EXTENSIONS`)
/// and writes (MP4, MOV and MKV clips, JPEG stills) and of the character
/// pictures AniList serves (JPEG, PNG, WebP, GIF), and the only ones
/// ffprobe uses. A "video" that is really a DASH manifest would otherwise
/// make it open whatever file the manifest names, a network share's
/// included (where Windows signs in with the user's credentials); none of
/// these reads anything but the file it's given (image2, only ever chosen
/// for an image's name, at most its numbered siblings). Every file is
/// probed before anything else reads it, so such a file is refused at
/// import.
pub const PROBE_FORMATS: &str = "matroska,webm,mov,mp4,m4a,3gp,3g2,mj2,avi,asf,mpegts,mpeg,mpegvideo,flv,ogg,mxf,image2,jpeg_pipe,png_pipe,webp_pipe,gif";

/// FFmpeg 7 is the oldest that works (`-stats_enc_pre {ptsi}`,
/// `-enc_time_base`); it came with libavcodec 61.
const MIN_MAJOR: u32 = 7;
const MIN_LIBAVCODEC: u32 = 61;

/// How long `ffmpeg -version` may take. It prints at once; a binary that
/// can't is damaged or stuck, and is ended with whatever it started.
const VERSION_WAIT: Duration = Duration::from_secs(10);

impl Tools {
    /// Find ffmpeg + ffprobe: in `bundled` alone when given (the app's
    /// resource folder), else in `AUTOSCENE_FFMPEG_DIR` or on `PATH`.
    pub fn locate(bundled: Option<&Path>) -> MediaResult<Self> {
        let (ffmpeg, ffprobe) = find(bundled, std::env::var_os("AUTOSCENE_FFMPEG_DIR"), std::env::var_os("PATH"))
            .ok_or(MediaError::ToolsMissing)?;
        Self::checked(ffmpeg, ffprobe, VERSION_WAIT)
    }

    /// An ffmpeg that doesn't print its version within `wait` is as good as
    /// missing: nothing else it does can be trusted, and startup waits on it.
    fn checked(ffmpeg: PathBuf, ffprobe: PathBuf, wait: Duration) -> MediaResult<Self> {
        let out = crate::proc::output_within(no_window(Command::new(&ffmpeg)).arg("-version"), wait)
            .ok()
            .flatten()
            .ok_or(MediaError::ToolsMissing)?;
        let version = supported_version(&String::from_utf8_lossy(&out.stdout))?;
        // Where it is says who the user is (their home folder): not at info,
        // which is what a shared log holds.
        tracing::debug!(ffmpeg = %ffmpeg.display(), "media tools found");
        tracing::info!(%version, "media tools ready");
        Ok(Self { ffmpeg, ffprobe, version, caps: OnceLock::new() })
    }

    /// Tools whose ffmpeg never answers, as one stuck in a GPU driver does:
    /// a program in `dir` that does nothing for a minute, whatever it is asked.
    #[cfg(test)]
    pub(crate) fn hung(dir: &Path) -> Self {
        #[cfg(windows)]
        let path = {
            let p = dir.join("hung-ffmpeg.cmd");
            // A loop, not `ping` or `timeout`: those are processes of their own,
            // and ending the script alone would leave them holding the pipes.
            std::fs::write(&p, "@echo off\r\n:again\r\ngoto again\r\n").unwrap();
            p
        };
        #[cfg(unix)]
        let path = {
            let p = dir.join("hung-ffmpeg");
            write_script(&p, "#!/bin/sh\nexec sleep 60\n");
            p
        };
        Self { ffmpeg: path.clone(), ffprobe: path, version: "stand-in".into(), caps: OnceLock::new() }
    }

    /// Tools with this ffmpeg (and no ffprobe).
    #[cfg(all(test, unix))]
    pub(crate) fn with_ffmpeg(ffmpeg: PathBuf) -> Self {
        Self { ffmpeg, ffprobe: PathBuf::new(), version: "stand-in".into(), caps: OnceLock::new() }
    }

    /// Tools that answer the lists (`-encoders`: NVENC and AMF H.264, HEVC
    /// NVENC; `-hwaccels`: cuda) and let every other ffmpeg run succeed at
    /// once, except one whose arguments contain `hang_on`: that one writes a
    /// line to `hangs` in `dir` and never ends. Its ffprobe describes a 64x64
    /// picture.
    #[cfg(test)]
    pub(crate) fn picky(dir: &Path, hang_on: &str) -> Self {
        const PROBE: &str = r#"{"format":{"format_name":"image2"},"streams":[{"index":0,"codec_type":"video","codec_name":"png","width":64,"height":64,"color_primaries":"bt709","color_transfer":"bt709","color_space":"bt709"}]}"#;
        let hangs = dir.join("hangs");
        let hangs = hangs.display();
        #[cfg(windows)]
        let (ffmpeg, ffprobe) = {
            let (ffmpeg, ffprobe) = (dir.join("picky-ffmpeg.cmd"), dir.join("picky-ffprobe.cmd"));
            let script = [
                "@echo off".to_string(),
                r#"echo %* | findstr /C:"-encoders" >NUL && goto enc"#.to_string(),
                r#"echo %* | findstr /C:"-hwaccels" >NUL && goto hw"#.to_string(),
                format!(r#"echo %* | findstr /C:"{hang_on}" >NUL && goto hang"#),
                "exit /b 0".to_string(),
                ":enc".to_string(),
                "echo  V....D h264_nvenc x".to_string(),
                "echo  V....D hevc_nvenc x".to_string(),
                "echo  V....D h264_amf x".to_string(),
                "exit /b 0".to_string(),
                ":hw".to_string(),
                "echo Hardware acceleration methods:".to_string(),
                "echo cuda".to_string(),
                "exit /b 0".to_string(),
                ":hang".to_string(),
                format!(r#"echo x >> "{hangs}""#),
                // A loop, not `ping`: that is a process of its own.
                ":stay".to_string(),
                "goto stay".to_string(),
                String::new(),
            ]
            .join("\r\n");
            std::fs::write(&ffmpeg, script).unwrap();
            std::fs::write(&ffprobe, format!("@echo off\r\necho {PROBE}\r\n")).unwrap();
            (ffmpeg, ffprobe)
        };
        #[cfg(unix)]
        let (ffmpeg, ffprobe) = {
            let (ffmpeg, ffprobe) = (dir.join("picky-ffmpeg"), dir.join("picky-ffprobe"));
            let script = [
                "#!/bin/sh".to_string(),
                r#"case "$*" in"#.to_string(),
                r#"*-encoders*) printf ' V....D h264_nvenc x\n V....D hevc_nvenc x\n V....D h264_amf x\n'; exit 0;;"#
                    .to_string(),
                r#"*-hwaccels*) printf 'Hardware acceleration methods:\ncuda\n'; exit 0;;"#.to_string(),
                format!("*{hang_on}*) echo x >> '{hangs}'; exec sleep 60;;"),
                "esac".to_string(),
                "exit 0".to_string(),
                String::new(),
            ]
            .join("\n");
            write_script(&ffmpeg, &script);
            write_script(&ffprobe, &format!("#!/bin/sh\nprintf '%s' '{PROBE}'\n"));
            (ffmpeg, ffprobe)
        };
        Self { ffmpeg, ffprobe, version: "stand-in".into(), caps: OnceLock::new() }
    }

    /// `ffmpeg -hide_banner -nostdin` with no console window.
    pub fn ffmpeg(&self) -> Command {
        let mut c = no_window(Command::new(&self.ffmpeg));
        c.args(["-hide_banner", "-nostdin"]);
        c
    }

    /// `ffprobe -v error` with no console window, reading only the
    /// containers of [`PROBE_FORMATS`].
    pub fn ffprobe(&self) -> Command {
        let mut c = no_window(Command::new(&self.ffprobe));
        c.args(["-hide_banner", "-v", "error", "-format_whitelist", PROBE_FORMATS]);
        c
    }

    /// Hardware capabilities, detected once (encoder smoke tests take ~1 s).
    pub fn caps(&self) -> &Caps {
        self.caps.get_or_init(|| Caps::detect(self))
    }

    /// Tools whose ffmpeg notes every run's arguments, a line each, in
    /// `asked` in `dir`, and ends at once without a frame. It lists cuda
    /// among its `-hwaccels`.
    #[cfg(test)]
    pub(crate) fn recording(dir: &Path) -> Self {
        let asked = dir.join("asked");
        let asked = asked.display();
        #[cfg(windows)]
        let ffmpeg = {
            let path = dir.join("recording-ffmpeg.cmd");
            let script = [
                "@echo off".to_string(),
                format!(r#"echo %* >> "{asked}""#),
                r#"echo %* | findstr /C:"-hwaccels" >NUL && echo cuda"#.to_string(),
                "exit /b 0".to_string(),
                String::new(),
            ]
            .join("\r\n");
            std::fs::write(&path, script).unwrap();
            path
        };
        #[cfg(unix)]
        let ffmpeg = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("recording-ffmpeg");
            let script = format!("#!/bin/sh\necho \"$*\" >> '{asked}'\ncase \"$*\" in *-hwaccels*) echo cuda;; esac\n");
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            // A process forked while the file was open for writing holds it
            // until it has run its program ("text file busy").
            std::thread::sleep(std::time::Duration::from_millis(150));
            path
        };
        Self { ffprobe: ffmpeg.clone(), ffmpeg, version: "stand-in".into(), caps: OnceLock::new() }
    }
}

/// Where ffmpeg and ffprobe are (see [`Tools::locate`]), given the
/// environment's `AUTOSCENE_FFMPEG_DIR` and `PATH`. Nothing is looked for
/// relative to the working directory: an empty `bundled` finds nothing, and
/// relative `PATH` entries are skipped.
fn find(bundled: Option<&Path>, env_dir: Option<OsString>, path: Option<OsString>) -> Option<(PathBuf, PathBuf)> {
    let both = |dir: &Path| {
        let (ff, fp) = (dir.join(format!("ffmpeg{EXE}")), dir.join(format!("ffprobe{EXE}")));
        (ff.is_file() && fp.is_file()).then_some((ff, fp))
    };
    if let Some(dir) = bundled {
        return both(&std::path::absolute(dir).ok()?);
    }
    if let Some(found) = env_dir.filter(|d| !d.is_empty()).and_then(|d| both(Path::new(&d))) {
        return Some(found);
    }
    let on_path = |name: &str| {
        let dirs = std::env::split_paths(path.as_ref()?).filter(|d| d.is_absolute());
        dirs.map(|d| d.join(format!("{name}{EXE}"))).find(|p| p.is_file())
    };
    Some((on_path("ffmpeg")?, on_path("ffprobe")?))
}

/// The version `ffmpeg -version` prints, when it's new enough. Judged by its
/// libavcodec, which every build prints whatever it calls itself (a git
/// build says `N-117000-g…`, some Windows builds a date); by the version
/// itself only when that line is missing. A build that tells neither is
/// refused: nothing says it isn't too old.
fn supported_version(out: &str) -> MediaResult<String> {
    let first = out.lines().next().unwrap_or_default();
    // "ffmpeg version 8.0.1-full_build-www.gyan.dev Copyright …" / "ffmpeg version n7.1 …"
    let version = first.split_whitespace().nth(2).unwrap_or("unknown").trim_start_matches('n').to_string();
    // "libavcodec     63.  1.102 / 63.  1.102"
    let libavcodec = out
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("libavcodec"))
        .and_then(|rest| rest.trim_start().split('.').next()?.trim().parse::<u32>().ok());
    // A date ("2021-05-05-git-…") isn't a version.
    let major = version.split(['.', '-']).next().filter(|m| m.len() < 4).and_then(|m| m.parse::<u32>().ok());
    let new_enough = match (libavcodec, major) {
        (Some(lavc), _) => lavc >= MIN_LIBAVCODEC,
        (None, Some(major)) => major >= MIN_MAJOR,
        (None, None) => {
            tracing::warn!(first, "can't tell which FFmpeg this is");
            false
        }
    };
    if new_enough { Ok(version) } else { Err(MediaError::ToolsTooOld(version)) }
}

/// Write an executable script for a test. Other tests start processes at the
/// same time, and one forked while this file was open for writing still holds
/// it until it has run its program; starting the script in that moment fails
/// ("text file busy"). The pause is longer than that.
#[cfg(all(test, unix))]
pub(crate) fn write_script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::thread::sleep(Duration::from_millis(150));
}

#[cfg(windows)]
fn no_window(mut c: Command) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
fn no_window(c: Command) -> Command {
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder with (empty) files named like ffmpeg and ffprobe.
    fn tools_dir(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for exe in ["ffmpeg", "ffprobe"] {
            std::fs::write(dir.join(format!("{exe}{EXE}")), b"").unwrap();
        }
        dir
    }

    #[test]
    fn a_bundled_folder_is_the_only_place_looked_in() {
        let tmp = tempfile::tempdir().unwrap();
        let (bundled, env, on_path) =
            (tools_dir(tmp.path(), "bundled"), tools_dir(tmp.path(), "env"), tools_dir(tmp.path(), "path"));
        let path = Some(std::env::join_paths([&on_path]).unwrap());

        let (ff, fp) = find(Some(&bundled), Some(env.clone().into()), path.clone()).unwrap();
        assert_eq!((ff.parent(), fp.parent()), (Some(bundled.as_path()), Some(bundled.as_path())));

        // Without its ffprobe the bundled build is missing, whatever else is around.
        std::fs::remove_file(bundled.join(format!("ffprobe{EXE}"))).unwrap();
        assert_eq!(find(Some(&bundled), Some(env.clone().into()), path.clone()), None);
        assert_eq!(find(Some(&tmp.path().join("gone")), Some(env.into()), path.clone()), None);
        // An empty folder is not the working directory.
        assert_eq!(find(Some(Path::new("")), None, path), None);
    }

    #[test]
    fn without_a_bundled_folder_the_environment_then_path_is_used() {
        let tmp = tempfile::tempdir().unwrap();
        let (env, on_path) = (tools_dir(tmp.path(), "env"), tools_dir(tmp.path(), "path"));
        let path = Some(std::env::join_paths([Path::new("relative"), &on_path]).unwrap());

        let (ff, _) = find(None, Some(env.clone().into()), path.clone()).unwrap();
        assert_eq!(ff.parent(), Some(env.as_path()));
        let (ff, fp) = find(None, Some(tmp.path().join("incomplete").into()), path.clone()).unwrap();
        assert_eq!((ff.parent(), fp.parent()), (Some(on_path.as_path()), Some(on_path.as_path())));
        let (ff, _) = find(None, Some(OsString::new()), path).unwrap();
        assert_eq!(ff.parent(), Some(on_path.as_path()));
        assert_eq!(find(None, None, None), None);
    }

    fn version_output(first: &str, libavcodec: Option<&str>) -> String {
        let mut out = format!("{first} Copyright (c) 2000-2026 the FFmpeg developers\nbuilt with gcc 16.2.0\n");
        out.push_str("configuration: --enable-gpl --enable-libavcodec-extra\nlibavutil      61.  1.102 / 61.  1.102\n");
        if let Some(v) = libavcodec {
            out.push_str(&format!("libavcodec     {v} / {v}\n"));
        }
        out
    }

    #[test]
    fn ffmpeg_7_or_newer_passes_by_its_libavcodec() {
        let ok = |first: &str, lavc: Option<&str>| supported_version(&version_output(first, lavc));
        // The bundled build.
        assert_eq!(
            ok("ffmpeg version n9.0.2-27-g7e93014c5b-20261013", Some("63.  1.102")).unwrap(),
            "9.0.2-27-g7e93014c5b-20261013"
        );
        // Git builds name no version; their libavcodec does.
        assert_eq!(ok("ffmpeg version N-117000-g1234abcd", Some("61. 19.100")).unwrap(), "N-117000-g1234abcd");
        assert!(matches!(ok("ffmpeg version N-110000-g1234abcd", Some("60.  3.100")), Err(MediaError::ToolsTooOld(_))));
        // A date-named FFmpeg 4 build isn't FFmpeg 2021.
        assert!(matches!(
            ok("ffmpeg version 2021-05-05-git-abcdef", Some("58. 134.100")),
            Err(MediaError::ToolsTooOld(_))
        ));
        assert!(matches!(ok("ffmpeg version 6.1.1", Some("60. 31.102")), Err(MediaError::ToolsTooOld(_))));
    }

    #[test]
    fn without_libavcodec_the_version_decides_and_unknown_is_refused() {
        let ok = |first: &str| supported_version(&version_output(first, None));
        assert_eq!(ok("ffmpeg version 8.0.1-full_build-www.gyan.dev").unwrap(), "8.0.1-full_build-www.gyan.dev");
        assert_eq!(ok("ffmpeg version n7.1").unwrap(), "7.1");
        assert!(matches!(ok("ffmpeg version 6.0"), Err(MediaError::ToolsTooOld(_))));
        assert!(matches!(ok("ffmpeg version N-117000-g1234abcd"), Err(MediaError::ToolsTooOld(_))));
        assert!(matches!(ok("ffmpeg version 2021-05-05-git-abcdef"), Err(MediaError::ToolsTooOld(_))));
        assert!(matches!(supported_version(""), Err(MediaError::ToolsTooOld(_))));
        assert!(matches!(supported_version("not ffmpeg at all"), Err(MediaError::ToolsTooOld(_))));
    }

    /// A downloaded "episode" that is really a DASH manifest: it names
    /// another file, which ffprobe would open (on a network share as
    /// readily as next to it). The probe refuses it instead.
    #[test]
    fn a_manifest_named_like_a_video_opens_nothing() {
        let tools = Tools::locate(None).expect("ffmpeg on PATH");
        let demuxers = crate::proc::output(no_window(Command::new(&tools.ffprobe)).args(["-hide_banner", "-demuxers"]))
            .expect("ffprobe -demuxers");
        if !String::from_utf8_lossy(&demuxers.stdout).lines().any(|l| l.split_whitespace().nth(1) == Some("dash")) {
            eprintln!("this ffmpeg reads no DASH (skipping)");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let segment = tmp.path().join("segment.mp4");
        let made = crate::proc::output(
            tools
                .ffmpeg()
                .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=duration=1:size=64x64:rate=10", "-c:v", "mpeg4"])
                .arg(&segment),
        )
        .unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        // What it points at is a real video, which it would play as its own.
        assert!(crate::probe(&tools, &segment).is_ok());

        let manifest = tmp.path().join("S01E01.mkv");
        std::fs::write(
            &manifest,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT1S" minBufferTime="PT1S"
     profiles="urn:mpeg:dash:profile:isoff-on-demand:2011">
  <Period><AdaptationSet mimeType="video/mp4">
    <Representation id="1" bandwidth="1000" width="64" height="64"><BaseURL>segment.mp4</BaseURL></Representation>
  </AdaptationSet></Period>
</MPD>
"#,
        )
        .unwrap();
        match crate::probe(&tools, &manifest) {
            Err(MediaError::Probe { message, .. }) => assert!(message.contains("whitelist"), "{message}"),
            other => panic!("the manifest was read: {other:?}"),
        }
    }

    /// The version check ends at its limit: a binary that doesn't answer is a
    /// missing one, not a startup that never ends.
    #[test]
    fn an_ffmpeg_that_never_prints_its_version_counts_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let hung = Tools::hung(dir.path());
        let start = std::time::Instant::now();
        let out = Tools::checked(hung.ffmpeg.clone(), hung.ffprobe.clone(), Duration::from_millis(500));
        assert!(matches!(out, Err(MediaError::ToolsMissing)), "{out:?}");
        let took = start.elapsed();
        assert!(took >= Duration::from_millis(500) && took < Duration::from_secs(10), "{took:?}");
    }
}
