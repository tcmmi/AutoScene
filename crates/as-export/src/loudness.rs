//! Evening out the sound of clips from different episodes and films.
//!
//! Each clip is measured as EBU R 128 measures a programme and turned up or
//! down by one fixed gain, so nothing changes inside a clip: its quiet and
//! loud moments keep their distance, and a loud clip is simply turned down.
//! The gain stops where the clip's peak would reach [`CEILING`], and a nearly
//! silent clip isn't blown up into hiss ([`MOST_BOOST`]). A clip of a few
//! seconds has too little for a reading, so it gets what the longer clips of
//! its episode got.

use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::Hash;

use as_core::{CancelToken, FrameRange};
use as_media::{MediaError, Source, Tools, run_ffmpeg_matching};

use crate::ExportResult;

/// What every clip is brought to (LUFS): the usual level of online video.
pub const TARGET: f64 = -16.0;
/// The highest true peak a gain may leave (dBTP).
const CEILING: f64 = -1.0;
/// The most a clip is turned up (dB).
const MOST_BOOST: f64 = 12.0;
/// Shorter clips (seconds) take their episode's gain.
const SHORTEST: f64 = 3.0;
/// Quieter than this (LUFS, or dB of peak) is silence.
const SILENCE: f64 = -60.0;

/// A clip's sound, measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    /// Integrated loudness (LUFS).
    pub lufs: f64,
    /// True peak (dBTP).
    pub peak: f64,
    pub seconds: f64,
}

/// `(loudness, peak)` from the lines of ffmpeg's `ebur128` summary.
fn parse(lines: &[String]) -> Option<(f64, f64)> {
    let value = |key: &str| {
        let line = lines.iter().map(|l| l.trim()).find(|l| l.starts_with(key))?;
        line[key.len()..].split_whitespace().next()?.parse::<f64>().ok()
    };
    Some((value("I:")?, value("Peak:")?))
}

/// Measure the sound of `range` of `src` (track `ordinal`), as `layout` will
/// hold it when it isn't the track's own: `merged`, a mono track at full
/// level on both sides, as parts of a file have it. `None`: there is none to
/// speak of.
pub fn measure(
    tools: &Tools,
    src: &Source,
    range: FrameRange,
    ordinal: u32,
    layout: Option<&str>,
    merged: bool,
    cancel: &CancelToken,
) -> ExportResult<Option<Measured>> {
    let (start, end) = (src.index.t(range.start), src.index.t(range.end));
    let seconds = end - start;
    let mono = src.info.audio.iter().find(|a| a.ordinal == ordinal).is_some_and(|a| a.channels == 1);
    let mix = match layout {
        Some(l) if merged && mono && l != "mono" => format!("pan=stereo|c0=c0|c1=c0,aformat=channel_layouts={l},"),
        Some(l) => format!("aformat=channel_layouts={l},"),
        None => String::new(),
    };
    let args: Vec<OsString> = vec![
        "-v".into(),
        "info".into(),
        "-ss".into(),
        // From the start of the file, which may be before its first frame.
        format!("{:.6}", (start - src.info.start_sec).max(0.0)).into(),
        "-t".into(),
        format!("{seconds:.6}").into(),
        "-i".into(),
        src.path.as_os_str().into(),
        "-map".into(),
        format!("0:a:{ordinal}").into(),
        "-af".into(),
        format!("{mix}ebur128=peak=true:framelog=quiet").into(),
        "-f".into(),
        "null".into(),
        "-".into(),
    ];
    match run_ffmpeg_matching(tools, args, cancel, |_| {}, &["i:", "peak:"]) {
        // Under half a second has no loudness yet, only a peak: `gains` knows.
        Ok(lines) => Ok(parse(&lines)
            .filter(|&(_, peak)| peak.is_finite() && peak > SILENCE)
            .map(|(lufs, peak)| Measured { lufs, peak, seconds })),
        Err(MediaError::Cancelled) => Err(MediaError::Cancelled.into()),
        // A clip that can't be read fails where it is cut, and says why.
        Err(e) => {
            tracing::warn!(detail = %e.detail(), "couldn't measure a clip's sound");
            Ok(None)
        }
    }
}

/// `wanted` dB, as far as a clip peaking at `peak` can take it.
fn capped(wanted: f64, peak: f64) -> f64 {
    wanted.min(MOST_BOOST).min(CEILING - peak)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

/// The gain (dB) of each clip, in order, from `(its episode, its sound)`.
pub fn gains<K: Eq + Hash + Copy>(clips: &[(K, Option<Measured>)]) -> Vec<f64> {
    let heard = |m: &Measured| m.lufs.is_finite() && m.lufs > SILENCE;
    let long = |m: &Measured| m.seconds >= SHORTEST && heard(m);
    let mut by_episode: HashMap<K, Vec<f64>> = HashMap::new();
    for (k, m) in clips {
        if let Some(m) = m.as_ref().filter(|m| long(m)) {
            by_episode.entry(*k).or_default().push(TARGET - m.lufs);
        }
    }
    let everyone = median(by_episode.values().flatten().copied().collect());
    clips
        .iter()
        .map(|(k, m)| match m {
            None => 0.0,
            Some(m) if long(m) => capped(TARGET - m.lufs, m.peak),
            Some(m) => {
                let episode = by_episode.get(k).and_then(|g| median(g.clone()));
                let own = heard(m).then_some(TARGET - m.lufs);
                episode.or(everyone).or(own).map_or(0.0, |gain| capped(gain, m.peak))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(lufs: f64, peak: f64, seconds: f64) -> Option<Measured> {
        Some(Measured { lufs, peak, seconds })
    }

    #[test]
    fn it_reads_ffmpegs_summary() {
        let lines = ["    I:         -23.4 LUFS".to_string(), "    Peak:       -3.1 dBFS".to_string()];
        assert_eq!(parse(&lines), Some((-23.4, -3.1)));
        assert_eq!(parse(&["    I:         nan LUFS".to_string()]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn a_quiet_clip_comes_up_and_a_loud_one_down() {
        let g = gains(&[(1, clip(-24.0, -12.0, 10.0)), (1, clip(-10.0, -1.0, 10.0))]);
        assert_eq!(g, [8.0, -6.0]);
    }

    #[test]
    fn a_gain_stops_at_the_peak_and_the_boost_limit() {
        assert_eq!(gains(&[(1, clip(-24.0, -3.0, 10.0))]), [2.0], "peak");
        assert_eq!(gains(&[(1, clip(-50.0, -30.0, 10.0))]), [12.0], "boost");
    }

    #[test]
    fn a_short_clip_takes_its_episodes_gain_then_everyones() {
        let g = gains(&[(1, clip(-20.0, -20.0, 30.0)), (1, clip(-40.0, -20.0, 1.0)), (2, clip(-40.0, -20.0, 1.0))]);
        assert_eq!(g, [4.0, 4.0, 4.0]);
        assert_eq!(gains(&[(1, clip(-20.0, -20.0, 1.0))]), [4.0], "nothing else to go by");
    }

    #[test]
    fn silence_stays_as_it_is() {
        assert_eq!(gains::<u8>(&[(1, None)]), [0.0]);
    }

    /// Under half a second a reading is its floor, -70: only the peak is true.
    #[test]
    fn a_clip_too_short_for_a_reading_takes_its_episodes_gain() {
        assert_eq!(gains(&[(1, clip(-20.0, -20.0, 30.0)), (1, clip(-70.0, -20.0, 0.3))]), [4.0, 4.0]);
        assert_eq!(gains(&[(1, clip(-70.0, -20.0, 0.3))]), [0.0], "nothing to go by");
    }
}
