//! Finding the black bars a picture was mastered with.
//!
//! A look at frames spread over the file: a line counts as bar when it is
//! dark and flat, so a dark scene with anything in it doesn't. What a single
//! frame says means little (a night sky reads as a bar, a fade as all bar),
//! so each side takes the median of the frames, and a side the frames
//! disagree on (a film that changes shape) takes the smallest bar: cutting
//! too little leaves a sliver, cutting too much loses picture.

use std::process::Stdio;
use std::time::Duration;

use as_core::CancelToken;
use serde::{Deserialize, Serialize};

use crate::{MediaError, MediaResult, Source, Tools};

/// Frames looked at.
const SAMPLES: u32 = 20;
/// Fewer usable frames than this say nothing.
const FEWEST: usize = 3;
/// A line darker than this on average (8 bits, video levels) may be bar.
const DARK: u32 = 32;
/// Video black: blacker than black counts as black.
const BLACK: u32 = 16;
/// How far a pixel of a bar strays from its line's average.
const FLAT: i32 = 16;
/// Frames in a row that may not be read before the file counts as unreadable.
const UNREAD_IN_A_ROW: u32 = 3;
/// How long one frame may take.
const FRAME_WAIT: Duration = Duration::from_secs(30);

/// Pixels of bar on each side of the upright picture, in even numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bars {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl Bars {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }

    /// `size` without the bars, in even numbers.
    pub fn inside(&self, (w, h): (u32, u32)) -> (u32, u32) {
        (w.saturating_sub(self.left + self.right) & !1, h.saturating_sub(self.top + self.bottom) & !1)
    }

    /// The filter that takes them off a picture of `size`; `None` without bars.
    pub fn filter(&self, size: (u32, u32)) -> Option<String> {
        let (w, h) = self.inside(size);
        (!self.is_none() && w > 0 && h > 0).then(|| format!("crop={w}:{h}:{}:{}", self.left, self.top))
    }
}

/// The picture's size as ffmpeg's filters get it: upright.
pub fn upright_size(info: &crate::MediaInfo) -> (u32, u32) {
    let v = &info.video;
    if matches!(v.rotation, 90 | 270) { (v.height, v.width) } else { (v.width, v.height) }
}

/// Is `line`, read pixel by pixel, a line of bar?
fn dark_and_flat(line: impl Iterator<Item = u8> + Clone) -> bool {
    let (mut sum, mut n) = (0u32, 0u32);
    for y in line.clone() {
        sum += u32::from(y).max(BLACK);
        n += 1;
    }
    if n == 0 {
        return false;
    }
    let mean = sum / n;
    mean < DARK && line.into_iter().all(|y| (i32::from(y).max(BLACK as i32) - mean as i32).abs() <= FLAT)
}

/// How many of `n` lines from one edge are bar. The outer 1% doesn't decide:
/// old masters have noise or a stray line there, in front of a real bar.
fn edge(n: u32, dark: impl Fn(u32) -> bool) -> u32 {
    let border = n / 100;
    let inward = (border..n).find(|&i| !dark(i)).unwrap_or(n);
    if inward > border { inward } else { (0..n).find(|&i| !dark(i)).unwrap_or(n) }
}

/// The bars of one frame's luma; `None` when they say nothing about the
/// picture, because a side is a quarter of it or more (a fade, the credits).
fn frame_bars(luma: &[u8], w: u32, h: u32) -> Option<Bars> {
    let (wu, hu) = (w as usize, h as usize);
    if luma.len() < wu * hu || w == 0 || h == 0 {
        return None;
    }
    let row = |y: u32| luma[y as usize * wu..(y as usize + 1) * wu].iter().copied();
    let column = |x: u32| luma[..wu * hu].iter().skip(x as usize).step_by(wu).copied();
    let bars = Bars {
        top: edge(h, |i| dark_and_flat(row(i))),
        bottom: edge(h, |i| dark_and_flat(row(h - 1 - i))),
        left: edge(w, |i| dark_and_flat(column(i))),
        right: edge(w, |i| dark_and_flat(column(w - 1 - i))),
    };
    (bars.top.max(bars.bottom) < h / 4 && bars.left.max(bars.right) < w / 4).then_some(bars)
}

/// One side's bar from what the frames say of it: the median, or the smallest
/// when frames disagree on it by more than a slip of the eye.
fn side(mut seen: Vec<u32>, length: u32) -> u32 {
    seen.sort_unstable();
    let median = seen[seen.len() / 2];
    let slip = 9.max(length / 120);
    let less = seen.iter().filter(|&&s| s + slip < median).count();
    // A dark scene only ever adds bar, so few frames with less are telling.
    let bar = if less >= 2.max(seen.len() / 10) { seen[0] } else { median };
    // Rounded up: a line of bar left is worse than a line of picture gone,
    // and 4:2:0 pictures are cut on even lines.
    bar + (bar & 1)
}

/// The bars frames agree on, for a picture of `w`×`h`.
fn agreed(frames: &[Bars], w: u32, h: u32) -> Bars {
    if frames.len() < FEWEST {
        return Bars::default();
    }
    let all = |f: fn(&Bars) -> u32| frames.iter().map(f).collect::<Vec<_>>();
    Bars {
        top: side(all(|b| b.top), h),
        bottom: side(all(|b| b.bottom), h),
        left: side(all(|b| b.left), w),
        right: side(all(|b| b.right), w),
    }
}

/// The luma of the frame at `seconds` into the file, at video levels (a
/// full-range file's black would read as darker than a bar).
fn luma_at(tools: &Tools, src: &Source, seconds: f64, (w, h): (u32, u32)) -> Option<Vec<u8>> {
    let mut cmd = tools.ffmpeg();
    cmd.args(["-v", "error", "-noaccurate_seek", "-ss", &format!("{seconds:.3}"), "-i"])
        .arg(src.path)
        .args(["-map", &format!("0:{}", src.info.video.index), "-frames:v", "1"])
        .args(["-vf", "scale=out_range=tv,format=yuv420p", "-f", "rawvideo", "-"])
        .stdin(Stdio::null());
    let out = crate::proc::output_within(&mut cmd, FRAME_WAIT).ok().flatten()?;
    let pixels = w as usize * h as usize;
    (out.status.success() && out.stdout.len() >= pixels).then(|| out.stdout[..pixels].to_vec())
}

/// The bars of `src`, none when its frames don't say. An error when too few
/// frames could be read to say anything: that isn't an answer to keep.
pub fn detect_bars(tools: &Tools, src: &Source, cancel: &CancelToken) -> MediaResult<Bars> {
    let (w, h) = upright_size(src.info);
    let frames = src.index.len();
    let (mut seen, mut read, mut unread) = (Vec::new(), 0, 0);
    for i in 1..=SAMPLES {
        if cancel.is_cancelled() {
            return Err(MediaError::Cancelled);
        }
        let f = (u64::from(frames) * u64::from(i) / u64::from(SAMPLES + 1)) as u32;
        let seconds = (src.index.t(f) - src.index.t0()).max(0.0);
        let Some(luma) = luma_at(tools, src, seconds, (w, h)) else {
            unread += 1;
            if unread == UNREAD_IN_A_ROW {
                break;
            }
            continue;
        };
        (read, unread) = (read + 1, 0);
        seen.extend(frame_bars(&luma, w, h));
    }
    if read < FEWEST {
        return Err(MediaError::Ffmpeg("too few frames could be read to look for black bars".into()));
    }
    Ok(agreed(&seen, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey-noise picture of `w`×`h` with bars of `top`, `bottom`, `left`, `right`.
    fn frame(w: u32, h: u32, (top, bottom, left, right): (u32, u32, u32, u32)) -> Vec<u8> {
        let mut x: u32 = 12345;
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for c in 0..w {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                let bar = y < top || y >= h - bottom || c < left || c >= w - right;
                out.push(if bar { 16 } else { 60 + (x >> 24) as u8 / 2 });
            }
        }
        out
    }

    #[test]
    fn a_letterbox_and_pillars_are_found() {
        let bars = frame_bars(&frame(640, 360, (40, 42, 0, 0)), 640, 360).unwrap();
        assert_eq!(bars, Bars { top: 40, bottom: 42, left: 0, right: 0 });
        let bars = frame_bars(&frame(640, 360, (0, 0, 80, 80)), 640, 360).unwrap();
        assert_eq!(bars, Bars { top: 0, bottom: 0, left: 80, right: 80 });
    }

    #[test]
    fn a_picture_without_bars_has_none() {
        assert_eq!(frame_bars(&frame(640, 360, (0, 0, 0, 0)), 640, 360), Some(Bars::default()));
    }

    #[test]
    fn a_black_frame_says_nothing() {
        assert_eq!(frame_bars(&vec![16; 640 * 360], 640, 360), None);
        assert_eq!(frame_bars(&frame(640, 360, (100, 100, 0, 0)), 640, 360), None, "a quarter or more");
    }

    #[test]
    fn a_dark_line_with_something_on_it_is_no_bar() {
        let mut f = frame(640, 360, (0, 0, 0, 0));
        for y in 0..20 {
            f[y * 640..(y + 1) * 640].fill(20);
            f[y * 640 + 300] = 200; // a star
        }
        assert_eq!(frame_bars(&f, 640, 360).unwrap().top, 0);
    }

    #[test]
    fn noise_at_the_very_edge_doesnt_hide_the_bar_behind_it() {
        let mut f = frame(1280, 720, (60, 0, 0, 0));
        f[..1280].fill(120); // the first line, in the outer 1%
        assert_eq!(frame_bars(&f, 1280, 720).unwrap().top, 60);
    }

    #[test]
    fn a_thin_bar_inside_the_outer_percent_counts() {
        assert_eq!(frame_bars(&frame(1280, 720, (2, 0, 0, 0)), 1280, 720).unwrap().top, 2);
    }

    fn letterbox(top: u32) -> Bars {
        Bars { top, bottom: top, left: 0, right: 0 }
    }

    #[test]
    fn the_median_ignores_dark_scenes() {
        let mut frames = vec![letterbox(138); 17];
        frames.extend([letterbox(170), letterbox(200), letterbox(160)]);
        assert_eq!(agreed(&frames, 1920, 1080), letterbox(138));
    }

    #[test]
    fn a_film_that_changes_shape_keeps_its_tallest_picture() {
        let mut frames = vec![letterbox(138); 15];
        frames.extend(vec![letterbox(0); 5]);
        assert_eq!(agreed(&frames, 1920, 1080), letterbox(0));
    }

    #[test]
    fn two_open_frames_are_enough_to_keep_the_picture() {
        let mut frames = vec![letterbox(138); 18];
        frames.extend(vec![letterbox(0); 2]);
        assert_eq!(agreed(&frames, 1920, 1080), letterbox(0));
        let one_off = [vec![letterbox(138); 19], vec![letterbox(0)]].concat();
        assert_eq!(agreed(&one_off, 1920, 1080), letterbox(138));
    }

    #[test]
    fn too_few_frames_say_nothing_and_odd_bars_round_up() {
        assert_eq!(agreed(&[letterbox(138); 2], 1920, 1080), Bars::default());
        assert_eq!(agreed(&[letterbox(137); 3], 1920, 1080), letterbox(138));
    }

    #[test]
    fn the_filter_keeps_even_sizes_and_nothing_means_no_filter() {
        assert_eq!(letterbox(138).filter((1920, 1080)).as_deref(), Some("crop=1920:804:0:138"));
        assert_eq!(Bars::default().filter((1920, 1080)), None);
        assert_eq!(Bars { left: 2, ..Bars::default() }.inside((1921, 1080)), (1918, 1080));
    }
}
