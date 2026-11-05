//! Scenepacks: a character's shots, filtered, ranked and picked by hand.
//! This is the one place that decides what a pack holds — for the grid, the
//! export plan and the export alike — so what is exported is what was shown.
//!
//! A shot is a character's by a face of theirs, or — when people are
//! followed by their bodies — by a body of theirs big and clear enough to
//! make a clip ([`as_analysis::bodies::Showing::prominent`]): a shot
//! without their face, badged so, behind the shots with it.
//!
//! Picking shots changes only their [`PackChoice`]s, never which shots a
//! pack has or their order: the grid takes the choices alone after a pick
//! ([`Project::pack_choices`]), while the pack's `shape` is the one it shows.
//!
//! Plugins' shot filters ([`PluginFilters`], from the app) leave out the
//! shots their plugin said no to ([`crate::verdicts`]), as AutoScene's own
//! filters do, before the limit. A shot none has judged yet stays in, and
//! counts as unchecked; a pick by hand wins over a verdict too. Only shots
//! the pack's own filters take are judged: the ones a check sends.
//!
//! A plugin's pack action saves the pack as one file before anything is
//! rendered: it gets what an export of the pack would cut
//! ([`Project::pack_cut`]), as the grid showed it, or the export's refusal.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use as_analysis::bodies::{dets_from_bytes, face_height_of};
use as_api::{
    CharacterShot, ClipRequest, ExportPlan, ExportRequest, MediaItem, MediaStatus, PackChoice, PackChoices, PackClip,
    PackExportRequest, PackFilter, PackFilterState, PackKey, PackNaming, PackOrder, PackPreview, PackReason, PackSpec,
    PackState, PackTotals, PresetSize, ShotGroup, ShotSize,
};
use as_core::{CharacterId, FrameIdx, FrameIndex, FrameRange, MediaId, Rational};
use as_db::{PackBodyRow, PackRow};
use as_export::preset::VideoCodec;
use as_export::{EstimateClip, ExportPreset, MergeSource, estimate_bytes, lossless_merge_blocker};
use as_media::Caps;

use crate::bodies::{showing_of, spans_of};
use crate::cast::track_id;
use crate::naming::{NameParts, check_template, clip_timecode, render_file_name, render_name, safe_component};
use crate::project::{apart_sentence, episode_name, lossless_note};
use crate::verdicts::{Judgement, PluginFilters};
use crate::{Project, ProjectError, ProjectResult};

// What makes a good clip, for "best first" and the limit. The weights add up
// to 1, so a score is 0..1.
/// How clearly the face shows who it is.
const W_QUALITY: f32 = 0.35;
/// How big the face is.
const W_SIZE: f32 = 0.30;
/// How much of the shot the character is on screen.
const W_COVERAGE: f32 = 0.20;
/// How well the shot's length cuts into an edit.
const W_LENGTH: f32 = 0.10;
/// Being alone in the shot.
const W_SOLO: f32 = 0.05;
/// A face this tall (share of the frame) or taller counts as full size.
const FULL_SIZE: f32 = 0.45;
/// Shot lengths that cut well into an edit, in seconds.
const GOOD_LENGTH: (f64, f64) = (1.0, 6.0);
/// Names a plan shows of a pack's files (the export dialog lists the first few).
const PLAN_NAMES: usize = 3;

/// How well a shot of `seconds` cuts into an edit: 1 inside the good range,
/// falling off outside it.
fn length_fit(seconds: f64) -> f32 {
    let (lo, hi) = GOOD_LENGTH;
    (if seconds < lo {
        seconds / lo
    } else if seconds > hi {
        hi / seconds
    } else {
        1.0
    }) as f32
}

/// A pack, made from the analysis as it stands.
struct Made {
    pack: PackPreview,
    /// The episodes it was made from.
    sources: HashMap<MediaId, Source>,
    /// For each of its plugins' filters (in [`PackPreview::filters`]'
    /// order): the shots the pack's own filters take that it hasn't judged
    /// on its basis now, in (media, start) order; none for one that's off.
    lacking: Vec<Vec<(MediaId, FrameIdx)>>,
}

/// What a pack action's run saves of a character's pack: the clips an
/// export of it would cut, named as its files (or chapters) would be, in the
/// pack's order, and the episodes they are cut from.
pub struct PackCut {
    pub character: CharacterId,
    /// The character's name.
    pub name: String,
    /// In the project's order.
    pub episodes: Vec<CutEpisode>,
    pub clips: Vec<ClipRequest>,
}

/// An episode a pack's clips are cut from: its file, frame rate and frames.
pub struct CutEpisode {
    pub media: MediaId,
    pub path: String,
    pub fps: Option<Rational>,
    pub frames: Option<u32>,
}

/// What a shot filter's run judges: one episode's shots of a character,
/// those the pack takes by its own filters that the filter hasn't judged.
pub struct FilterCheck {
    pub character: CharacterId,
    /// The character's name.
    pub name: String,
    pub media: MediaId,
    /// The episode's file, frame rate and frames.
    pub path: String,
    pub fps: Option<Rational>,
    pub frames: Option<u32>,
    pub shots: Vec<CheckShot>,
}

/// A shot a filter's run judges.
pub struct CheckShot {
    pub start: FrameIdx,
    pub end: FrameIdx,
    /// See [`CharacterShot::face_height`].
    pub face_height: f32,
    pub faceless: bool,
    /// The character's clearest face in it: its frame and its box (x1, y1,
    /// x2, y2 as shares of the frame); none without their face.
    pub face: Option<(FrameIdx, [f32; 4])>,
}

/// What the plugins' filters said, for one pack: each filter's verdicts
/// (none for one that is off), and those that apply (the spec's, on), in
/// the spec's order.
struct Judged<'a> {
    filters: &'a PluginFilters,
    all: Vec<Option<Judgement>>,
    on: Vec<usize>,
}

/// An episode a pack can cut from, with its frame grid.
struct Source {
    media: MediaItem,
    /// `None` while it can't be cut from (its file is missing, or it isn't indexed).
    index: Option<Arc<FrameIndex>>,
}

impl Project {
    /// A character's pack settings as last used (the defaults the first time).
    pub fn pack_state(&self, c: CharacterId) -> ProjectResult<PackState> {
        let Some((spec, naming)) = self.db().pack_state(c)? else { return Ok(PackState::default()) };
        Ok(PackState { spec: with_defaults(&spec), naming: naming.map(|n| with_defaults(&n)).unwrap_or_default() })
    }

    /// Keep a character's pack settings.
    pub fn save_pack_state(&self, c: CharacterId, state: &PackState) -> ProjectResult<()> {
        let spec = serde_json::to_string(&state.spec).expect("a spec serialises");
        let naming = serde_json::to_string(&state.naming).expect("naming serialises");
        if !self.db().set_pack_state(c, &spec, Some(&naming))? {
            return Err(gone());
        }
        Ok(())
    }

    /// Pick (`Some(true)`), drop (`Some(false)`) or hand back to the filters
    /// (`None`) these shots of a character's pack. A pick is kept on the
    /// frame its shot starts at; an older one inside the shot (made before it
    /// was found again) goes.
    pub fn set_pack_overrides(&self, c: CharacterId, shots: &[PackKey], include: Option<bool>) -> ProjectResult<()> {
        let keys: Vec<(MediaId, FrameIdx)> = shots.iter().map(|k| (k.media_id, k.shot)).collect();
        if !self.db().set_pack_overrides(c, &keys, include)? {
            return Err(gone());
        }
        Ok(())
    }

    /// Every shot of the character, the ones `spec` (with the plugins'
    /// `filters`, and the hand picks) take marked, in the pack's order.
    pub fn pack_preview(&self, c: CharacterId, spec: &PackSpec, filters: &PluginFilters) -> ProjectResult<PackPreview> {
        Ok(self.make_pack(c, spec, filters)?.pack)
    }

    /// The pack `spec` makes as picks leave it: each clip's choice, in the
    /// pack's order. The rest is its preview's while the `shape` is the same.
    pub fn pack_choices(&self, c: CharacterId, spec: &PackSpec, filters: &PluginFilters) -> ProjectResult<PackChoices> {
        let PackPreview { clips, totals, stamp, shape, .. } = self.make_pack(c, spec, filters)?.pack;
        Ok(PackChoices { choices: clips.iter().map(choice_of).collect(), totals, stamp, shape })
    }

    /// The name `c` goes by, as a check of their shots says it.
    pub fn character_label(&self, c: CharacterId) -> ProjectResult<String> {
        self.db().character_label(c)?.ok_or_else(gone)
    }

    /// The episode a check of the plugin filter `filter` for `c`'s pack
    /// `spec` goes on with after `after` (from the first when none): the
    /// first, in project order, with shots the pack's own filters take that
    /// `filter` hasn't judged on its basis now.
    pub fn filter_next(
        &self,
        c: CharacterId,
        spec: &PackSpec,
        filters: &PluginFilters,
        filter: &str,
        after: Option<MediaId>,
    ) -> ProjectResult<Option<MediaId>> {
        let Some((i, _)) = filters.find(filter) else { return Ok(None) };
        let made = self.make_pack(c, spec, filters)?;
        let order: HashMap<MediaId, u32> = made.sources.values().map(|s| (s.media.id, s.media.position)).collect();
        let from = after.and_then(|m| order.get(&m)).copied();
        Ok(made.lacking[i]
            .iter()
            .filter_map(|(m, _)| Some((*order.get(m)?, *m)))
            .filter(|(at, _)| from.is_none_or(|from| *at > from))
            .min()
            .map(|(_, m)| m))
    }

    /// What a check of the plugin filter `filter` judges in `media` for
    /// `c`'s pack `spec`: the shots of it the pack's own filters take that
    /// `filter` hasn't judged on its basis now, with the episode and the
    /// character. None when there are none (or the filter is off).
    pub fn filter_shots(
        &self,
        c: CharacterId,
        spec: &PackSpec,
        filters: &PluginFilters,
        filter: &str,
        media: MediaId,
    ) -> ProjectResult<Option<FilterCheck>> {
        let Some((i, _)) = filters.find(filter) else { return Ok(None) };
        let mut made = self.make_pack(c, spec, filters)?;
        let wanted: HashSet<FrameIdx> = made.lacking[i].iter().filter(|(m, _)| *m == media).map(|(_, s)| *s).collect();
        let Some(source) = made.sources.remove(&media).filter(|_| !wanted.is_empty()) else { return Ok(None) };
        let mut clips: Vec<&PackClip> =
            made.pack.clips.iter().filter(|x| x.shot.media_id == media && wanted.contains(&x.shot.start)).collect();
        clips.sort_by_key(|x| x.shot.start);
        // The character's clearest face in each shot: the clearest moment of
        // their clearest track there.
        let tracks: Vec<i64> = clips.iter().filter(|x| !x.shot.faceless).map(|x| i64::from(x.shot.track_id)).collect();
        let boxes: HashMap<i64, (FrameIdx, [f32; 4])> = self
            .db()
            .track_dets(&tracks)?
            .into_iter()
            .filter_map(|(id, _, dets)| {
                let dets = as_analysis::tracks::dets_from_bytes(&dets)?;
                let d = as_analysis::tracks::clearest(&dets)?;
                Some((id, (d.frame, d.bbox)))
            })
            .collect();
        let shots = clips
            .iter()
            .map(|x| CheckShot {
                start: x.shot.start,
                end: x.shot.end,
                face_height: x.shot.face_height,
                faceless: x.shot.faceless,
                face: if x.shot.faceless { None } else { boxes.get(&i64::from(x.shot.track_id)).copied() },
            })
            .collect();
        let name = self.character_label(c)?;
        let MediaItem { path, fps, frames, .. } = source.media;
        Ok(Some(FilterCheck { character: c, name, media, path, fps, frames, shots }))
    }

    /// The first names the pack's files (or chapters) get, in its order; the
    /// one file's name or the character's folder; the pack's size per preset
    /// with the encoders of `caps`.
    pub fn pack_plan(
        &self,
        c: CharacterId,
        spec: &PackSpec,
        filters: &PluginFilters,
        naming: &PackNaming,
        presets: &[ExportPreset],
        caps: &Caps,
    ) -> ProjectResult<ExportPlan> {
        // The file's name first: the dialog shows its problem under it.
        let label = self.db().character_label(c)?.ok_or_else(gone)?;
        let file = self.pack_file_name(&label, naming)?;
        let made = self.make_pack(c, spec, filters)?;
        let clips = self.named_clips(c, &made, naming)?;
        let mut sized = Vec::with_capacity(clips.len());
        let mut joined = Vec::with_capacity(clips.len());
        for clip in &clips {
            let Some(Source { media, index: Some(index) }) = made.sources.get(&clip.media_id) else { continue };
            let Some(info) = &media.info else { continue };
            sized.push(EstimateClip {
                info,
                audio_ordinal: media.audio_ordinal,
                frames: u64::from(clip.end - clip.start),
                seconds: index.t(clip.end) - index.t(clip.start),
                fps: index.nominal_fps.to_f64(),
            });
            joined.push(MergeSource {
                info,
                index,
                range: FrameRange::new(clip.start, clip.end),
                audio_ordinal: media.audio_ordinal,
            });
        }
        // Bit-Perfect joins only episodes alike; others make a file per clip.
        let apart = |p: &ExportPreset| {
            (file.is_some() && p.video == VideoCodec::Lossless)
                .then(|| lossless_merge_blocker(&joined).map(|why| apart_sentence(&why)))
                .flatten()
        };
        let sizes = presets
            .iter()
            .map(|p| {
                let apart = apart(p);
                let merged = file.is_some() && apart.is_none();
                let (ext, note) = lossless_note(p, sized.iter().map(|c| c.info));
                PresetSize { preset_id: p.id.clone(), bytes: estimate_bytes(p, caps, &sized, merged), apart, ext, note }
            })
            .collect();
        // Named for a file per clip (Bit-Perfect may make one of a file).
        let folder = naming.folder.then(|| safe_component(&label, " ", 80));
        Ok(ExportPlan {
            names: clips.into_iter().filter_map(|c| c.name).take(PLAN_NAMES).collect(),
            sizes,
            folder,
            file,
        })
    }

    /// The export of a character's pack, made again from the analysis: what
    /// the grid showed (`req.stamp`), or an error when the pack changed since.
    /// Returns the request and the folder it goes to (inside `default_dir`
    /// unless the request names another; a file per clip goes into the
    /// character's own folder if asked, one file for all of them doesn't).
    /// The settings are kept for next time.
    pub fn pack_export_request(
        &self,
        c: CharacterId,
        req: &PackExportRequest,
        filters: &PluginFilters,
        default_dir: &Path,
    ) -> ProjectResult<(ExportRequest, PathBuf)> {
        let made = self.shown_pack(c, &req.spec, filters, &req.naming, &req.stamp)?;
        let label = self.db().character_label(c)?.ok_or_else(gone)?;
        let mut merge = self.pack_file_name(&label, &req.naming)?;
        let clips = self.named_clips(c, &made, &req.naming)?;
        // A Bit-Perfect file of episodes that differ: a file per clip, as
        // the dialog said (in the character's folder, if asked for one).
        let preset = as_export::builtin_presets().into_iter().find(|p| p.id == req.preset_id);
        if let Some(preset) = preset.filter(|_| merge.is_some())
            && self.apart_reason(&preset, &clips)?.is_some()
        {
            merge = None;
        }
        let base = req
            .out_dir
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map_or(default_dir.to_path_buf(), PathBuf::from);
        let out_dir =
            if req.naming.folder && merge.is_none() { base.join(safe_component(&label, " ", 80)) } else { base };
        self.save_pack_state(c, &PackState { spec: req.spec.clone(), naming: req.naming.clone() })?;
        let request = ExportRequest {
            preset_id: req.preset_id.clone(),
            out_dir: Some(out_dir.display().to_string()),
            clips,
            label: Some(format!("{label} · scenepack")),
            merge,
        };
        Ok((request, out_dir))
    }

    /// What a pack action saves of a character's pack: the clips an export
    /// of what the grid showed (`stamp`) would cut, named by `naming`, and
    /// their episodes; refused as that export would be, when the pack
    /// changed since or takes nothing.
    pub fn pack_cut(
        &self,
        c: CharacterId,
        spec: &PackSpec,
        filters: &PluginFilters,
        naming: &PackNaming,
        stamp: &str,
    ) -> ProjectResult<PackCut> {
        let made = self.shown_pack(c, spec, filters, naming, stamp)?;
        let name = self.character_label(c)?;
        let clips = self.named_clips(c, &made, naming)?;
        let mut episodes: Vec<&MediaItem> = Vec::new();
        for clip in &clips {
            if let Some(s) = made.sources.get(&clip.media_id)
                && !episodes.iter().any(|m| m.id == s.media.id)
            {
                episodes.push(&s.media);
            }
        }
        episodes.sort_by_key(|m| m.position);
        let episodes = episodes
            .into_iter()
            .map(|m| CutEpisode { media: m.id, path: m.path.clone(), fps: m.fps, frames: m.frames })
            .collect();
        Ok(PackCut { character: c, name, episodes, clips })
    }

    /// The pack `spec` makes (with the plugins' `filters`) as the grid
    /// showed it (`stamp`), to be cut with clips named by `naming`: refused
    /// when the pack changed since, when it takes nothing, or when `naming`
    /// can't name its clips.
    fn shown_pack(
        &self,
        c: CharacterId,
        spec: &PackSpec,
        filters: &PluginFilters,
        naming: &PackNaming,
        stamp: &str,
    ) -> ProjectResult<Made> {
        check_template(&naming.template).map_err(|e| ProjectError::Invalid(e.to_string()))?;
        let made = self.make_pack(c, spec, filters)?;
        if made.pack.stamp != stamp {
            return Err(ProjectError::Invalid("The pack changed meanwhile. Check it and export again.".into()));
        }
        if made.pack.totals.clips == 0 {
            return Err(ProjectError::Invalid("There's nothing in the pack to export.".into()));
        }
        Ok(made)
    }

    /// The one file's name when `naming` joins the pack into one.
    fn pack_file_name(&self, label: &str, naming: &PackNaming) -> ProjectResult<Option<String>> {
        if !naming.merge {
            return Ok(None);
        }
        let name = render_file_name(&naming.file, &self.title()?, label);
        name.map(Some).map_err(|e| ProjectError::Invalid(e.to_string()))
    }

    /// The pack's clips (the ones it takes, in its order), named by `naming`.
    fn named_clips(&self, c: CharacterId, made: &Made, naming: &PackNaming) -> ProjectResult<Vec<ClipRequest>> {
        check_template(&naming.template).map_err(|e| ProjectError::Invalid(e.to_string()))?;
        let title = self.title()?;
        let label = self.db().character_label(c)?.ok_or_else(gone)?;
        let taken: Vec<&PackClip> = made.pack.clips.iter().filter(|c| c.included).collect();
        let mut used = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(taken.len());
        for (i, clip) in taken.iter().enumerate() {
            // Only clips of episodes with a frame grid are taken.
            let Some(source @ Source { index: Some(index), .. }) = made.sources.get(&clip.shot.media_id) else {
                continue;
            };
            let fps = index.nominal_fps;
            let (tc, episode) = (clip_timecode(clip.start, fps), episode_name(&source.media));
            let parts = NameParts {
                series: &title,
                character: Some(&label),
                episode: &episode,
                index: (i + 1, taken.len()),
                tc: &tc,
                frame: clip.start,
            };
            let base = render_name(&naming.template, &parts).map_err(|e| ProjectError::Invalid(e.to_string()))?;
            // Two clips can't share a name (a template without {tc} or {index}).
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base}_{n}");
                n += 1;
            }
            out.push(ClipRequest { media_id: clip.shot.media_id, start: clip.start, end: clip.end, name: Some(name) });
        }
        Ok(out)
    }

    fn make_pack(&self, c: CharacterId, spec: &PackSpec, filters: &PluginFilters) -> ProjectResult<Made> {
        // Only a filter that isn't off is read: the pack ignores the others.
        let mut all = Vec::with_capacity(filters.0.len());
        for f in &filters.0 {
            all.push(match f.state {
                PackFilterState::Off | PackFilterState::Gone => None,
                _ => Some(self.judgement(&f.key, c, &f.basis)?),
            });
        }
        let mut on: Vec<usize> = Vec::new();
        for key in &spec.plugin_filters {
            if let Some((i, _)) = filters.find(key).filter(|(i, _)| all[*i].is_some() && !on.contains(i)) {
                on.push(i);
            }
        }
        let judged = Judged { filters, all, on };
        let rows = self.db().pack_rows(c)?;
        let bodies = self.shown_bodies(|db| db.pack_bodies(c))?;
        let overrides = self.db().pack_overrides(c)?;
        let cover = self.cover_thumb(c)?;
        // Every episode, in project order, with its frame grid when it can be cut from.
        let mut sources = HashMap::new();
        let mut order = HashMap::new();
        for (i, media) in self.media_list_uncounted()?.into_iter().enumerate() {
            let index = match media.status {
                MediaStatus::Ready => self.frame_index(media.id).ok(),
                _ => None,
            };
            order.insert(media.id, i);
            sources.insert(media.id, Source { media, index });
        }
        let shots = shots_of(c, &rows, &bodies);
        let shot = |s: &Shot| self.character_shot(s, cover.as_deref());
        let (pack, lacking) = make(spec, &shots, &sources, &order, &overrides, &judged, shot);
        Ok(Made { pack, sources, lacking })
    }

    /// The small picture of a character's own face (for shots without it).
    fn cover_thumb(&self, c: CharacterId) -> ProjectResult<Option<String>> {
        Ok(self.db().character_cover(c)?.map(|(t, m)| Project::path_string(&self.thumb_path(m, t))))
    }

    /// A shot of a character as the cast and the pack show it: its picture
    /// and the character's face in it — `cover`, their own face, where it
    /// doesn't show.
    fn character_shot(&self, s: &Shot, cover: Option<&str>) -> CharacterShot {
        let face = match s.bodies.is_empty() {
            false => cover.unwrap_or_default().to_string(),
            true => Project::path_string(&self.thumb_path(s.media, s.track)),
        };
        CharacterShot {
            media_id: s.media,
            start: s.start,
            end: s.end,
            sprite: s.sprite.map(|(end, _)| Project::path_string(&self.sprite_path(s.media, s.start, end))),
            cells: s.sprite.map_or(0, |x| x.1),
            track_id: track_id(s.track),
            face,
            face_height: s.face_h,
            faceless: !s.bodies.is_empty(),
            bodies: s.bodies.iter().map(|&b| track_id(b)).collect(),
        }
    }

    /// Every shot a character is seen in, in episode order: by a face of
    /// theirs, or by a body of theirs that makes a clip.
    pub fn character_shots(&self, c: CharacterId) -> ProjectResult<Vec<CharacterShot>> {
        Ok(self.shots_and_media(c)?.0)
    }

    /// A character's shots as the cast shows them: in episode order, in
    /// groups of shots close together in time (see `group_lengths`).
    /// Nothing is kept: a new analysis or an edit in the cast groups them
    /// again.
    pub fn character_shot_groups(&self, c: CharacterId) -> ProjectResult<Vec<ShotGroup>> {
        let (shots, media) = self.shots_and_media(c)?;
        let fps = media.into_iter().filter_map(|m| Some((m.id, m.fps?))).collect();
        Ok(groups_of(shots, &fps, &self.db().fades()?))
    }

    /// [`Self::character_shots`], and the project's media, which put them
    /// in episode order.
    fn shots_and_media(&self, c: CharacterId) -> ProjectResult<(Vec<CharacterShot>, Vec<MediaItem>)> {
        let shots = shots_of(c, &self.db().pack_rows(c)?, &self.shown_bodies(|db| db.pack_bodies(c))?);
        let media = self.media_list_uncounted()?;
        let order: HashMap<MediaId, usize> = media.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
        let cover = self.cover_thumb(c)?;
        let mut out: Vec<(usize, CharacterShot)> = shots
            .iter()
            .filter_map(|s| Some((*order.get(&s.media)?, self.character_shot(s, cover.as_deref()))))
            .collect();
        out.sort_by_key(|(i, s)| (*i, s.start));
        Ok((out.into_iter().map(|(_, s)| s).collect(), media))
    }
}

/// How far apart, in seconds, two shots of a character may be and still
/// share a group: from the end of one to the start of the next. 6 s
/// bridges someone else's reverse shot of normal length, as dialogue cuts
/// back and forth. Measured on a Breaking Bad episode whose scenes were
/// labelled by eye (#59): with 6 s, 9 % of its four main characters' shots
/// sit in a group made mostly of another scene; with 10 s, 14 %.
const GROUP_GAP: i64 = 6;

/// The sizes of the groups one episode's shots of a character make, in
/// order. `shots` are in time order, `fades` (the episode's fades and
/// black, see [`as_db::ProjectDb::fades`]) in frame order. A shot joins
/// the group before it when it starts at most [`GROUP_GAP`] (rounded up to
/// whole frames) after the shot before it ends, and no fade or black lies
/// in that gap or touches it: a dissolve or a flash doesn't part them.
/// Without a frame rate every shot is a group of its own.
fn group_lengths(shots: &[(FrameIdx, FrameIdx)], fades: &[FrameRange], fps: Option<Rational>) -> Vec<usize> {
    let gap = fps
        .filter(|f| f.num > 0 && f.den > 0)
        .map(|f| GROUP_GAP.saturating_mul(f.num).saturating_add(f.den - 1) / f.den);
    let mut out: Vec<usize> = Vec::new();
    for (i, &(start, _)) in shots.iter().enumerate() {
        let joins = i > 0
            && gap.is_some_and(|gap| {
                let end = shots[i - 1].1;
                i64::from(start.saturating_sub(end)) <= gap && !fade_between(fades, end, start)
            });
        match out.last_mut() {
            Some(n) if joins => *n += 1,
            _ => out.push(1),
        }
    }
    out
}

/// A fade or black (`fades`, in frame order, apart from each other) in the
/// gap `[end, start)` between two shots, or touching it at either end.
fn fade_between(fades: &[FrameRange], end: FrameIdx, start: FrameIdx) -> bool {
    // Apart and in order, they end in order too: of those that end where
    // the gap starts or later, the first starts earliest.
    let first = fades.partition_point(|f| f.end < end);
    fades.get(first).is_some_and(|f| f.start <= start)
}

/// `shots` (in episode order, as [`Project::character_shots`] lists them)
/// in groups: [`group_lengths`] per episode, with its frame rate (`fps`)
/// and its fades. Shots of two episodes never share a group.
fn groups_of(
    shots: Vec<CharacterShot>,
    fps: &HashMap<MediaId, Rational>,
    fades: &HashMap<MediaId, Vec<FrameRange>>,
) -> Vec<ShotGroup> {
    let lengths: Vec<usize> = shots
        .chunk_by(|a, b| a.media_id == b.media_id)
        .flat_map(|run| {
            let media = run[0].media_id;
            let spans: Vec<(FrameIdx, FrameIdx)> = run.iter().map(|s| (s.start, s.end)).collect();
            group_lengths(&spans, fades.get(&media).map_or(&[], Vec::as_slice), fps.get(&media).copied())
        })
        .collect();
    let mut shots = shots.into_iter();
    lengths.into_iter().map(|n| ShotGroup { shots: shots.by_ref().take(n).collect() }).collect()
}

/// Settings saved by another version: what is there, the defaults for the rest.
fn with_defaults<T: Default + serde::Serialize + serde::de::DeserializeOwned>(json: &str) -> T {
    let mut merged = serde_json::to_value(T::default()).expect("defaults serialise");
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::Object(stored)) => {
            if let serde_json::Value::Object(fields) = &mut merged {
                fields.extend(stored);
            }
        }
        _ => tracing::warn!("unreadable pack settings; using the defaults"),
    }
    serde_json::from_value(merged).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "pack settings of an unknown shape; using the defaults");
        T::default()
    })
}

fn gone() -> ProjectError {
    ProjectError::NotFound("This character isn't in the cast any more.".into())
}

/// A shot of the character, worked out from their faces and bodies.
pub(crate) struct Shot {
    media: MediaId,
    start: FrameIdx,
    end: FrameIdx,
    tiny: bool,
    sprite: Option<(FrameIdx, u32)>,
    /// The character's clearest face in it (0: none shows).
    track: i64,
    quality: f32,
    /// The biggest the character's face is in it (as big as their body
    /// suggests, without one).
    face_h: f32,
    /// Frames the character is on screen: their faces and bodies.
    on_screen: u32,
    /// Other (shown) characters on screen in it.
    others: u32,
    /// Their face doesn't show: these bodies of theirs do (one makes a clip).
    bodies: Vec<i64>,
}

/// The shots of `c` from the faces (`rows`, `c`'s clearest first in each
/// shot) and the bodies (`bodies`, `c`'s tallest first) of the shots they
/// are seen in: by a face of theirs, or by a body of theirs that makes a
/// clip of its own.
pub(crate) fn shots_of(c: CharacterId, rows: &[PackRow], bodies: &[PackBodyRow]) -> Vec<Shot> {
    #[derive(Default)]
    struct Parts<'a> {
        faces: Vec<&'a PackRow>,
        bodies: Vec<&'a PackBodyRow>,
    }
    let mut by: BTreeMap<(MediaId, FrameIdx), Parts> = BTreeMap::new();
    for r in rows {
        by.entry((r.media_id, r.shot)).or_default().faces.push(r);
    }
    for b in bodies {
        by.entry((b.body.media_id, b.body.shot)).or_default().bodies.push(b);
    }
    let mut out = Vec::with_capacity(by.len());
    for ((media, start), parts) in by {
        let (end, tiny, sprite) = match (parts.faces.first(), parts.bodies.first()) {
            (Some(r), _) => (r.shot_end, r.tiny, r.sprite),
            (None, Some(b)) => (b.shot_end, b.tiny, b.sprite),
            (None, None) => continue,
        };
        let faces: Vec<&PackRow> = parts.faces.iter().copied().filter(|r| r.character_id == c).collect();
        let mine: Vec<&PackBodyRow> = parts.bodies.iter().copied().filter(|b| b.body.character_id == c).collect();
        let (track, quality, face_h, bodies) = match faces.first() {
            // The first row of a shot is the character's clearest face there.
            Some(f) => (f.track, f.quality, faces.iter().map(|r| r.face_h).fold(0.0, f32::max), Vec::new()),
            None => {
                let Some(b) = mine.iter().find(|b| showing_of(&b.body).prominent()) else { continue };
                let dets = dets_from_bytes(&b.body.dets).unwrap_or_default();
                (0, 0.0, face_height_of(&dets), mine.iter().map(|b| b.body.id).collect())
            }
        };
        let mut spans: Vec<(FrameIdx, FrameIdx)> = faces.iter().map(|r| (r.start, r.end)).collect();
        spans.extend(mine.iter().flat_map(|b| spans_of(&b.body.dets, b.body.end)));
        spans.sort_unstable();
        let (mut covered, mut reach) = (0, start);
        for &(a, b) in &spans {
            let (a, b) = (a.max(reach).max(start), b.min(end));
            if b > a {
                covered += b - a;
                reach = b;
            }
        }
        let mut others: Vec<CharacterId> = parts
            .faces
            .iter()
            .filter(|r| r.character_id != c && !r.hidden)
            .map(|r| r.character_id)
            .chain(
                parts.bodies.iter().filter(|b| b.body.character_id != c && !b.body.hidden).map(|b| b.body.character_id),
            )
            .collect();
        others.sort_unstable();
        others.dedup();
        out.push(Shot {
            media,
            start,
            end,
            tiny,
            sprite,
            track,
            quality,
            face_h,
            on_screen: covered,
            others: others.len() as u32,
            bodies,
        });
    }
    out
}

/// Tenths of a second `frames` take at `fps`, rounded: in whole numbers, so
/// shots as long are as long whatever their frames' timestamps round to.
fn tenths(frames: u32, fps: Rational) -> i64 {
    if fps.num <= 0 || fps.den <= 0 {
        return 0;
    }
    (i64::from(frames) * 10 * fps.den + fps.num / 2) / fps.num
}

/// A clip's choice: whether it is in, and why.
fn choice_of(c: &PackClip) -> PackChoice {
    PackChoice { included: c.included, by_rules: c.by_rules, reason: c.reason, manual: c.manual, filter: c.filter }
}

/// The pack itself — its clips, totals, stamp and shape: pure, so every rule
/// is tested without a database. `shots` are in (media, start) order, as
/// [`shots_of`] makes them; `shot` makes a card's shot. Also, for each of
/// the plugins' filters, the shots it has yet to judge (see [`Made`]).
fn make(
    spec: &PackSpec,
    shots: &[Shot],
    sources: &HashMap<MediaId, Source>,
    order: &HashMap<MediaId, usize>,
    overrides: &[(MediaId, FrameIdx, bool)],
    judged: &Judged,
    shot: impl Fn(&Shot) -> CharacterShot,
) -> (PackPreview, Vec<Vec<(MediaId, FrameIdx)>>) {
    debug_assert!(shots.is_sorted_by_key(|s| (s.media, s.start)));
    // Picks by hand: on the shot that starts there, or — when shots were
    // found again since — on the shot that now holds that frame (the last
    // one starting at or before it).
    let mut manual: HashMap<(MediaId, FrameIdx), bool> = HashMap::new();
    for &(m, f, include) in overrides {
        let at = shots.partition_point(|s| (s.media, s.start) <= (m, f)).checked_sub(1).map(|i| &shots[i]);
        if let Some(s) = at.filter(|s| s.media == m && f < s.end) {
            // An exact match wins over a moved one.
            if s.start == f || !manual.contains_key(&(m, s.start)) {
                manual.insert((m, s.start), include);
            }
        }
    }

    struct Cand {
        clip: PackClip,
        rule: Option<PackReason>,
        seconds_with_handles: f64,
        /// Tenths of a second the character is on screen in the shot.
        on_screen: i64,
    }
    let mut cands: Vec<Cand> = Vec::with_capacity(shots.len());
    // Per plugin filter: the shots it judged on its basis now, and those it could.
    let mut checked = vec![(0u32, 0u32); judged.all.len()];
    let mut lacking = vec![Vec::new(); judged.all.len()];
    let mut unchecked = 0;
    for s in shots {
        let source = sources.get(&s.media);
        let index = source.and_then(|x| x.index.as_ref());
        let seconds = index.map_or(0.0, |i| i.t(s.end.min(i.len())) - i.t(s.start.min(i.len())));
        let size = ShotSize::of(s.face_h);
        let coverage = if s.end > s.start { s.on_screen as f32 / (s.end - s.start) as f32 } else { 0.0 };
        let score = W_QUALITY * s.quality.clamp(0.0, 1.0)
            + W_SIZE * (s.face_h / FULL_SIZE).min(1.0)
            + W_COVERAGE * coverage.min(1.0)
            + W_LENGTH * length_fit(seconds)
            + W_SOLO * if s.others == 0 { 1.0 } else { 0.0 };
        let rule = if index.is_none() {
            Some(PackReason::Missing)
        } else if !spec.media.is_empty() && !spec.media.contains(&s.media) {
            Some(PackReason::Episode)
        } else if spec.skip_tiny && s.tiny {
            Some(PackReason::Tiny)
        } else if spec.skip_faceless && !s.bodies.is_empty() {
            Some(PackReason::Faceless)
        } else if spec.min_sec.is_some_and(|min| seconds < min) {
            Some(PackReason::Short)
        } else if spec.max_sec.is_some_and(|max| seconds > max) {
            Some(PackReason::Long)
        } else if !spec.sizes.is_empty() && !spec.sizes.contains(&size) {
            Some(PackReason::Size)
        } else if spec.solo && s.others > 0 {
            Some(PackReason::NotSolo)
        } else {
            None
        };
        // The plugins' filters judge what AutoScene's own take; the first
        // filter turned on that says no names the shot's reason.
        let mut filter = None;
        if rule.is_none() {
            let mut unjudged = false;
            for (i, j) in judged.all.iter().enumerate() {
                let Some(j) = j else { continue };
                checked[i].1 += 1;
                if j.current(s.media, s.start) {
                    checked[i].0 += 1;
                } else {
                    lacking[i].push((s.media, s.start));
                    unjudged |= judged.on.contains(&i);
                }
            }
            unchecked += u32::from(unjudged);
            filter =
                judged.on.iter().copied().find(|&i| judged.all[i].as_ref().is_some_and(|j| j.drops(s.media, s.start)));
        }
        let rule = rule.or(filter.map(|_| PackReason::Plugin));
        let (start, end) = match index {
            Some(i) => (s.start.saturating_sub(spec.handles), (s.end + spec.handles).min(i.len())),
            None => (s.start, s.end),
        };
        let seconds_with_handles = index.map_or(0.0, |i| i.t(end) - i.t(start));
        let clip = PackClip {
            shot: shot(s),
            start,
            end,
            size,
            quality: s.quality,
            coverage,
            others: s.others,
            score,
            included: false,
            by_rules: rule.is_none(),
            reason: rule,
            manual: manual.get(&(s.media, s.start)).copied(),
            filter: filter.map(|i| i as u32),
        };
        let on_screen = index.map_or(0, |i| tenths(s.on_screen.min(s.end - s.start), i.nominal_fps));
        cands.push(Cand { clip, rule, seconds_with_handles, on_screen });
    }

    let episode =
        |c: &PackClip| (order.get(&c.shot.media_id).copied().unwrap_or(usize::MAX), c.shot.media_id, c.shot.start);
    // Which of two shots makes the better clip, for "best first" and the
    // limit alike. Shots without the character's face come behind every
    // shot with it; then the higher score. The best shots all score 1
    // (clear, big, on screen all along, alone): of those, the one they are
    // on screen longer in (to a tenth of a second), then the one their face
    // is bigger in, then the earlier one.
    let better = |a: &Cand, b: &Cand| -> Ordering {
        a.clip
            .shot
            .faceless
            .cmp(&b.clip.shot.faceless)
            .then(b.clip.score.total_cmp(&a.clip.score))
            .then(b.on_screen.cmp(&a.on_screen))
            .then(b.clip.shot.face_height.total_cmp(&a.clip.shot.face_height))
            .then_with(|| episode(&a.clip).cmp(&episode(&b.clip)))
    };
    // The limit keeps the best of what the filters take; picks by hand
    // stay on top of it and don't take a place. A missing file can't be
    // picked back in: there is nothing to cut from.
    if let Some(limit) = spec.limit {
        let limit = limit as usize;
        let mut kept: Vec<usize> =
            (0..cands.len()).filter(|&i| cands[i].rule.is_none() && cands[i].clip.manual.is_none()).collect();
        kept.sort_by(|&a, &b| better(&cands[a], &cands[b]));
        // A pick by hand takes no place; the filters alone would take it
        // if it made the limit without the pick.
        let picked_past: Vec<usize> = (0..cands.len())
            .filter(|&i| cands[i].rule.is_none() && cands[i].clip.manual.is_some())
            .filter(|&i| kept.partition_point(|&k| better(&cands[k], &cands[i]).is_lt()) >= limit)
            .collect();
        for i in kept.into_iter().skip(limit).chain(picked_past) {
            cands[i].clip.by_rules = false;
            cands[i].rule = Some(PackReason::Limit);
        }
    }
    for cand in &mut cands {
        let missing = cand.rule == Some(PackReason::Missing);
        let clip = &mut cand.clip;
        clip.included = !missing && clip.manual.unwrap_or(cand.rule.is_none());
        clip.reason = match clip.manual {
            _ if missing => Some(PackReason::Missing),
            Some(false) => Some(PackReason::Removed),
            Some(true) => None,
            None => cand.rule,
        };
    }

    match spec.order {
        PackOrder::Episodes => cands.sort_by_key(|x| episode(&x.clip)),
        PackOrder::Best => cands.sort_by(better),
    }

    let mut totals = PackTotals { clips: 0, frames: 0.0, seconds: 0.0 };
    let (mut stamp, mut shape) = (Fnv::new(), Fnv::new());
    // A clip's filter is a place in this list.
    for f in &judged.filters.0 {
        shape.text(&f.key);
    }
    for cand in &cands {
        shape.clip(&cand.clip);
        if cand.clip.included {
            totals.clips += 1;
            totals.frames += f64::from(cand.clip.end - cand.clip.start);
            totals.seconds += cand.seconds_with_handles;
            stamp.write(cand.clip.shot.media_id.to_string().as_bytes());
            stamp.write(&cand.clip.start.to_le_bytes());
            stamp.write(&cand.clip.end.to_le_bytes());
        }
    }
    let clips = cands.into_iter().map(|x| x.clip).collect();
    let filters = judged
        .filters
        .0
        .iter()
        .zip(checked)
        .map(|(f, (checked, total))| PackFilter {
            key: f.key.clone(),
            title: f.title.clone(),
            description: f.description.clone(),
            reason: f.reason.clone(),
            plugin: f.plugin.clone(),
            checked,
            total,
            state: f.state,
        })
        .collect();
    let pack = PackPreview { clips, totals, stamp: stamp.hex(), shape: shape.hex(), filters, unchecked };
    (pack, lacking)
}

/// FNV-1a: a short fingerprint of what a pack takes (its stamp), or of its
/// clips apart from their choices (its shape).
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn text(&mut self, s: &str) {
        self.write(&(s.len() as u64).to_le_bytes());
        self.write(s.as_bytes());
    }
    /// Everything a card shows of a clip but its [`PackChoice`].
    fn clip(&mut self, c: &PackClip) {
        let s = &c.shot;
        self.text(&s.media_id.to_string());
        for n in [s.start, s.end, s.cells, s.track_id, c.start, c.end, c.others, c.size as u32] {
            self.write(&n.to_le_bytes());
        }
        for x in [s.face_height, c.quality, c.coverage, c.score] {
            self.write(&x.to_bits().to_le_bytes());
        }
        self.write(&[u8::from(s.faceless), u8::from(s.sprite.is_some())]);
        self.text(s.sprite.as_deref().unwrap_or_default());
        self.text(&s.face);
        self.write(&(s.bodies.len() as u64).to_le_bytes());
        for b in &s.bodies {
            self.write(&b.to_le_bytes());
        }
    }
    fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

#[cfg(test)]
mod bench;

#[cfg(test)]
mod tests {
    use as_analysis::FaceDet;
    use as_analysis::tracks::{self, FaceTrack};
    use as_api::{CastEdit, Segment, SegmentKind, TrackFilter};

    use super::*;
    use crate::analysis::tests::project_with_media;
    use crate::cast::tests::seen;
    use crate::verdicts::PluginFilter;

    const FRAMES: u32 = 2400;

    /// `t` with a face `height` tall (share of the frame) in every detection.
    fn sized(mut t: FaceTrack, height: f32) -> FaceTrack {
        t.dets = t
            .dets
            .into_iter()
            .map(|d| FaceDet { bbox: [d.bbox[0], 0.3, d.bbox[0] + height * 0.6, 0.3 + height], ..d })
            .collect();
        t
    }

    struct Pack {
        dir: tempfile::TempDir,
        project: Arc<Project>,
        media: MediaId,
        walter: CharacterId,
        jesse: CharacterId,
    }

    /// The faces of one 24 fps episode (see [`pack`]); `split` is where the
    /// last shot starts.
    fn faces(split: FrameIdx) -> [FaceTrack; 5] {
        [
            sized(seen(1, 0, 0, 48, 0.1), 0.3),
            // Jesse: the only face 0.2 tall.
            sized(seen(2, 0, 0, 48, 0.6), 0.2),
            sized(seen(1, 48, 48, 50, 0.1), 0.3),
            sized(seen(1, 50, 50, split, 0.1), 0.5),
            sized(seen(1, split, 290, 350, 0.1), 0.05),
        ]
    }

    fn shots(split: FrameIdx) -> Vec<Segment> {
        let shot = |start, end| Segment { start, end, kind: SegmentKind::Shot, tiny: end - start < 3 };
        vec![shot(0, 48), shot(48, 50), shot(50, split), shot(split, 410)]
    }

    /// One 24 fps episode of 100 s: shots 0–48 (2 s, Walter with Jesse),
    /// 48–50 (a flash cut, Walter), 50–290 (10 s, Walter big all along) and
    /// 290–410 (5 s, Walter small for half of it).
    fn pack() -> Pack {
        let (dir, project, media) = project_with_media(FRAMES);
        project.db().store_segments(media, &shots(290)).unwrap();
        project.store_face_chunk(media, 0, FRAMES, &tracks::encode(&faces(290)), &[], &[]).unwrap();
        project.finish_faces(media).unwrap();
        let tracks = project.cast_tracks(TrackFilter::Unassigned).unwrap();
        let is_jesse = |h: f32| (h - 0.2).abs() < 0.01;
        let pick = |jesse: bool| -> Vec<u32> {
            tracks.iter().filter(|t| is_jesse(t.face_height) == jesse).map(|t| t.id).collect()
        };
        assert_eq!((pick(false).len(), pick(true).len()), (4, 1));
        let new = |tracks, name: &str| {
            project.cast_edit(&CastEdit::NewCharacter { tracks, name: Some(name.into()) }).unwrap().unwrap()
        };
        let walter = new(pick(false), "Walter");
        let jesse = new(pick(true), "Jesse");
        Pack { dir, project, media, walter, jesse }
    }

    fn preview(p: &Pack, spec: &PackSpec) -> PackPreview {
        p.project.pack_preview(p.walter, spec, &PluginFilters::NONE).unwrap()
    }

    fn starts(v: &PackPreview, included: bool) -> Vec<FrameIdx> {
        v.clips.iter().filter(|c| c.included == included).map(|c| c.shot.start).collect()
    }

    fn clip(v: &PackPreview, start: FrameIdx) -> &PackClip {
        v.clips.iter().find(|c| c.shot.start == start).unwrap()
    }

    fn with(f: impl Fn(&mut PackSpec)) -> PackSpec {
        let mut s = PackSpec::default();
        f(&mut s);
        s
    }

    #[test]
    fn every_shot_of_the_character_in_episode_order() {
        let p = pack();
        let v = preview(&p, &PackSpec::default());
        assert_eq!(v.clips.iter().map(|c| c.shot.start).collect::<Vec<_>>(), [0, 48, 50, 290]);
        // Flash cuts stay out by default; every other shot is in.
        assert_eq!(starts(&v, true), [0, 50, 290]);
        assert_eq!(clip(&v, 48).reason, Some(PackReason::Tiny));
        // What each shot is like.
        let (a, b, c) = (clip(&v, 0), clip(&v, 50), clip(&v, 290));
        assert_eq!((a.size, b.size, c.size), (ShotSize::CloseUp, ShotSize::ExtremeCloseUp, ShotSize::Wide));
        assert_eq!((a.others, b.others), (1, 0), "Jesse is with him in shot 0");
        assert!((b.coverage - 1.0).abs() < 1e-6 && (c.coverage - 0.5).abs() < 1e-6, "{} {}", b.coverage, c.coverage);
        assert!(b.score > a.score && b.score > c.score, "big, alone, on screen all along: the best");
        // Totals: 48 + 240 + 120 frames at 24 fps.
        assert_eq!((v.totals.clips, v.totals.frames), (3, 408.0));
        assert!((v.totals.seconds - 17.0).abs() < 1e-9, "{}", v.totals.seconds);
        // Cards show the shot and the character's face.
        assert_eq!((b.shot.start, b.shot.end, b.start, b.end), (50, 290, 50, 290));
        assert!(b.shot.face.ends_with(".jpg"));
    }

    #[test]
    fn filters_leave_shots_out_and_say_why() {
        let p = pack();
        let v = preview(&p, &with(|s| s.min_sec = Some(3.0)));
        assert_eq!((starts(&v, true), clip(&v, 0).reason), (vec![50, 290], Some(PackReason::Short)));
        let v = preview(&p, &with(|s| s.max_sec = Some(6.0)));
        assert_eq!((starts(&v, true), clip(&v, 50).reason), (vec![0, 290], Some(PackReason::Long)));
        let v = preview(&p, &with(|s| s.sizes = vec![ShotSize::CloseUp, ShotSize::ExtremeCloseUp]));
        assert_eq!((starts(&v, true), clip(&v, 290).reason), (vec![0, 50], Some(PackReason::Size)));
        let v = preview(&p, &with(|s| s.solo = true));
        assert_eq!((starts(&v, true), clip(&v, 0).reason), (vec![50, 290], Some(PackReason::NotSolo)));
        let v = preview(&p, &with(|s| s.skip_tiny = false));
        assert_eq!(starts(&v, true), [0, 48, 50, 290]);
        let v = preview(&p, &with(|s| s.media = vec![MediaId::new()]));
        assert!(starts(&v, true).is_empty() && clip(&v, 0).reason == Some(PackReason::Episode));
        // A hidden character doesn't count as someone else in the shot.
        p.project.cast_edit(&CastEdit::SetHidden { character_id: p.jesse, hidden: true }).unwrap();
        assert_eq!(starts(&preview(&p, &with(|s| s.solo = true)), true), [0, 50, 290]);
    }

    #[test]
    fn handles_stay_inside_the_episode() {
        let p = pack();
        let v = preview(&p, &with(|s| (s.handles, s.skip_tiny) = (30, false)));
        let ranges: Vec<_> = v.clips.iter().map(|c| (c.start, c.end)).collect();
        assert_eq!(ranges, [(0, 78), (18, 80), (20, 320), (260, 440)]);
        // Handles longer than what is left end at the episode's last frame.
        let v = preview(&p, &with(|s| s.handles = 2100));
        assert_eq!((clip(&v, 0).start, clip(&v, 290).end), (0, FRAMES));
        // The filters look at the shot itself, not its handles.
        let v = preview(&p, &with(|s| (s.handles, s.min_sec) = (30, Some(3.0))));
        assert_eq!(starts(&v, true), [50, 290]);
    }

    #[test]
    fn best_first_and_a_limit_that_keeps_hand_picks() {
        let p = pack();
        let v = preview(&p, &with(|s| s.order = PackOrder::Best));
        assert_eq!(v.clips[0].shot.start, 50, "the best first");
        assert!(v.clips.windows(2).all(|w| w[0].score >= w[1].score));
        // At most one: the best. The flash cut picked by hand stays on top of it.
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 48 }], Some(true)).unwrap();
        let v = preview(&p, &with(|s| s.limit = Some(1)));
        assert_eq!(starts(&v, true), [48, 50]);
        assert_eq!((clip(&v, 0).reason, clip(&v, 290).reason), (Some(PackReason::Limit), Some(PackReason::Limit)));
        assert_eq!((clip(&v, 48).manual, clip(&v, 48).by_rules), (Some(true), false));
        // Dropped by hand: out, and says so; handing it back to the filters undoes it.
        let key = PackKey { media_id: p.media, shot: 50 };
        p.project.set_pack_overrides(p.walter, &[key], Some(false)).unwrap();
        let v = preview(&p, &PackSpec::default());
        assert_eq!((clip(&v, 50).included, clip(&v, 50).reason), (false, Some(PackReason::Removed)));
        p.project.set_pack_overrides(p.walter, &[key], None).unwrap();
        assert!(clip(&preview(&p, &PackSpec::default()), 50).included);
    }

    #[test]
    fn under_a_limit_a_pick_by_hand_knows_what_the_limit_would_say() {
        let p = pack();
        let limit = with(|s| s.limit = Some(1));
        // The shot at 0 is past the limit; picked, it is in, and the
        // filters alone would still leave it out.
        let key = PackKey { media_id: p.media, shot: 0 };
        p.project.set_pack_overrides(p.walter, &[key], Some(true)).unwrap();
        let v = preview(&p, &limit);
        assert_eq!((clip(&v, 0).included, clip(&v, 0).by_rules, clip(&v, 0).reason), (true, false, None));
        // Taken out again: back to the filters, past the limit — no pick left.
        p.project.set_pack_overrides(p.walter, &[key], None).unwrap();
        let v = preview(&p, &limit);
        assert_eq!((clip(&v, 0).manual, clip(&v, 0).reason), (None, Some(PackReason::Limit)));
        // The best shot picked by hand would make the limit anyway.
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], Some(true)).unwrap();
        assert!(clip(&preview(&p, &limit), 50).by_rules);
    }

    #[test]
    fn a_missing_file_says_so_even_for_a_pick() {
        let p = pack();
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], Some(false)).unwrap();
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 0 }], Some(true)).unwrap();
        std::fs::remove_file(p.project.media(p.media).unwrap().path).unwrap();
        let v = preview(&p, &PackSpec::default());
        assert!(v.clips.iter().all(|c| !c.included && c.reason == Some(PackReason::Missing)), "{:?}", v.clips);
    }

    #[test]
    fn hand_picks_follow_a_shot_found_again() {
        let p = pack();
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 290 }], Some(false)).unwrap();
        // Shots found again: the last one now starts at 280; a new face pass.
        p.project.db().store_segments(p.media, &shots(280)).unwrap();
        p.project.reset_faces(p.media).unwrap();
        p.project.store_face_chunk(p.media, 0, FRAMES, &tracks::encode(&faces(280)), &[], &[]).unwrap();
        p.project.finish_faces(p.media).unwrap();
        let v = preview(&p, &PackSpec::default());
        assert_eq!(clip(&v, 280).reason, Some(PackReason::Removed), "the pick moved with its shot");
        // And it can be undone where the shot is now.
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 280 }], None).unwrap();
        let v = preview(&p, &PackSpec::default());
        assert_eq!((clip(&v, 280).manual, clip(&v, 280).included), (None, true));
        assert!(p.project.db().pack_overrides(p.walter).unwrap().is_empty());
    }

    #[test]
    fn an_export_takes_exactly_what_the_pack_showed() {
        let p = pack();
        let spec = with(|s| s.handles = 2);
        let naming = PackNaming { merge: false, ..PackNaming::default() };
        let v = preview(&p, &spec);
        let out = p.dir.path().join("Exports");
        let req = |spec: &PackSpec, stamp: &str| PackExportRequest {
            spec: spec.clone(),
            naming: naming.clone(),
            preset_id: "h264-edit".into(),
            out_dir: None,
            stamp: stamp.into(),
        };
        let (request, dir) =
            p.project.pack_export_request(p.walter, &req(&spec, &v.stamp), &PluginFilters::NONE, &out).unwrap();
        let shown: Vec<_> = v.clips.iter().filter(|c| c.included).map(|c| (c.shot.media_id, c.start, c.end)).collect();
        let exported: Vec<_> = request.clips.iter().map(|c| (c.media_id, c.start, c.end)).collect();
        assert_eq!(exported, shown);
        assert_eq!(dir, out.join("Walter"), "in the character's own folder");
        assert_eq!(request.label.as_deref(), Some("Walter · scenepack"));
        assert_eq!(request.merge, None, "a file per clip");
        assert_eq!(
            request.clips.iter().map(|c| c.name.clone().unwrap()).collect::<Vec<_>>(),
            ["Show_Walter_E01_00-00-00-00", "Show_Walter_E01_00-00-02-00", "Show_Walter_E01_00-00-12-00"]
        );
        // The settings are kept for next time.
        assert_eq!(p.project.pack_state(p.walter).unwrap().spec, spec);
        // A pack that changed since it was shown isn't exported.
        let other = PackSpec { solo: true, ..spec.clone() };
        assert!(p.project.pack_export_request(p.walter, &req(&other, &v.stamp), &PluginFilters::NONE, &out).is_err());
        // Nor is a template that can't name files.
        let mut bad = req(&spec, &v.stamp);
        bad.naming.template = "{series} {nme}".into();
        assert!(p.project.pack_export_request(p.walter, &bad, &PluginFilters::NONE, &out).is_err());

        // Queued as asked: a clip each, so named.
        let preset = as_export::builtin_presets().into_iter().find(|x| x.id == "h264-edit").unwrap();
        let queued = p.project.create_export(&request, preset.clone(), &dir).unwrap();
        let items: Vec<_> = queued.items.iter().map(|i| (i.start, i.end, Some(i.name.clone()))).collect();
        assert_eq!(items, request.clips.iter().map(|c| (c.start, c.end, c.name.clone())).collect::<Vec<_>>());
        // A clip past the episode's end is refused, wherever it is.
        let mut past = request.clone();
        past.clips[2].end = FRAMES + 1;
        assert!(p.project.create_export(&past, preset, &dir).is_err());

        // A pick since the pack was shown: refused too, until the grid has
        // the pack as the pick left it.
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], Some(false)).unwrap();
        assert!(p.project.pack_export_request(p.walter, &req(&spec, &v.stamp), &PluginFilters::NONE, &out).is_err());
        let now = p.project.pack_choices(p.walter, &spec, &PluginFilters::NONE).unwrap();
        let (request, _) =
            p.project.pack_export_request(p.walter, &req(&spec, &now.stamp), &PluginFilters::NONE, &out).unwrap();
        assert_eq!(request.clips.len(), 2);
    }

    #[test]
    fn a_pick_changes_the_choices_alone() {
        let p = pack();
        // Under a limit of two: shots 50 and 0 are in, 290 is past it.
        let spec = with(|s| s.limit = Some(2));
        let before = preview(&p, &spec);
        assert_eq!((starts(&before, true), clip(&before, 290).reason), (vec![0, 50], Some(PackReason::Limit)));
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], Some(false)).unwrap();
        let after = preview(&p, &spec);
        let choices = p.project.pack_choices(p.walter, &spec, &PluginFilters::NONE).unwrap();
        // The same shots in the same order: their choices are all that changed
        // — the dropped shot's, and the next one's the limit lets in.
        assert_eq!((after.shape.as_str(), choices.shape.as_str()), (before.shape.as_str(), before.shape.as_str()));
        assert_eq!(choices.choices, after.clips.iter().map(choice_of).collect::<Vec<_>>());
        assert_eq!((starts(&after, true), clip(&after, 50).reason), (vec![0, 290], Some(PackReason::Removed)));
        assert_eq!((choices.stamp.as_str(), choices.totals.clips), (after.stamp.as_str(), 2));
        assert_ne!(after.stamp, before.stamp);
        // Best first under a limit: the shot the limit lets in now moves nowhere.
        let best = with(|s| (s.limit, s.order) = (Some(1), PackOrder::Best));
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], None).unwrap();
        let ranked = preview(&p, &best);
        assert_eq!((starts(&ranked, true), clip(&ranked, 0).reason), (vec![50], Some(PackReason::Limit)));
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], Some(false)).unwrap();
        let picked = preview(&p, &best);
        assert_eq!((picked.shape.as_str(), starts(&picked, true)), (ranked.shape.as_str(), vec![0]));
        let choices = p.project.pack_choices(p.walter, &best, &PluginFilters::NONE).unwrap().choices;
        assert_eq!(choices, picked.clips.iter().map(choice_of).collect::<Vec<_>>());
        // More than picks — other handles, someone hidden in the cast — is another shape.
        assert_ne!(preview(&p, &with(|s| (s.limit, s.handles) = (Some(2), 2))).shape, before.shape);
        p.project.cast_edit(&CastEdit::SetHidden { character_id: p.jesse, hidden: true }).unwrap();
        assert_ne!(p.project.pack_choices(p.walter, &spec, &PluginFilters::NONE).unwrap().shape, before.shape);
    }

    /// One 24 fps episode where every shot of Walter scores 1 (a clear face
    /// at least 0.45 tall, alone, on screen all along, 1–6 s long): 0–96
    /// (4 s, face 0.5 tall), 97–193 (4 s, 0.7 — 3.999999999999999 s by its
    /// frames' timestamps), 193–241 (2 s, 0.9) and 241–265 (1 s, 0.5).
    fn perfect() -> (tempfile::TempDir, Arc<Project>, CharacterId) {
        let (dir, project, media) = project_with_media(FRAMES);
        let shots = [(0, 96, 0.5), (97, 193, 0.7), (193, 241, 0.9), (241, 265, 0.5)];
        let segments: Vec<Segment> =
            shots.iter().map(|&(start, end, _)| Segment { start, end, kind: SegmentKind::Shot, tiny: false }).collect();
        project.db().store_segments(media, &segments).unwrap();
        let faces: Vec<FaceTrack> = shots
            .iter()
            .map(|&(start, end, height)| FaceTrack { quality: 1.0, ..sized(seen(1, start, start, end, 0.1), height) })
            .collect();
        project.store_face_chunk(media, 0, FRAMES, &tracks::encode(&faces), &[], &[]).unwrap();
        project.finish_faces(media).unwrap();
        let tracks = project.cast_tracks(TrackFilter::Unassigned).unwrap().iter().map(|t| t.id).collect();
        let edit = CastEdit::NewCharacter { tracks, name: Some("Walter".into()) };
        let walter = project.cast_edit(&edit).unwrap().unwrap();
        (dir, project, walter)
    }

    #[test]
    fn of_shots_that_score_alike_the_longest_then_the_closest_is_best() {
        let (_dir, project, walter) = perfect();
        let v = project.pack_preview(walter, &with(|s| s.order = PackOrder::Best), &PluginFilters::NONE).unwrap();
        assert!(v.clips.iter().all(|c| c.score == v.clips[0].score), "every shot scores the same");
        // On screen longest first; of two as long, the one the face is
        // bigger in; the shortest last — not simply the earliest first.
        assert_eq!(v.clips.iter().map(|c| c.shot.start).collect::<Vec<_>>(), [97, 0, 193, 241]);
        // A limit keeps the same ones, in whatever order the grid shows them.
        let v = project.pack_preview(walter, &with(|s| s.limit = Some(2)), &PluginFilters::NONE).unwrap();
        assert_eq!((starts(&v, true), clip(&v, 193).reason), (vec![0, 97], Some(PackReason::Limit)));
    }

    #[test]
    fn a_pack_in_one_file_has_a_chapter_per_shot() {
        let p = pack();
        let v = preview(&p, &PackSpec::default());
        let out = p.dir.path().join("Exports");
        let mut req = PackExportRequest {
            spec: PackSpec::default(),
            naming: PackNaming::default(),
            preset_id: "h264-edit".into(),
            out_dir: None,
            stamp: v.stamp.clone(),
        };
        assert!(req.naming.merge && req.naming.folder, "one file by default; the folder waits for a file per clip");
        let (request, dir) = p.project.pack_export_request(p.walter, &req, &PluginFilters::NONE, &out).unwrap();
        assert_eq!(request.merge.as_deref(), Some("Show_Walter_scenepack"));
        assert_eq!(dir, out, "one file needs no folder of its own");
        // The clips' names are the file's chapters.
        assert_eq!(
            request.clips.iter().map(|c| c.name.clone().unwrap()).collect::<Vec<_>>(),
            ["Show_Walter_E01_00-00-00-00", "Show_Walter_E01_00-00-02-02", "Show_Walter_E01_00-00-12-02"]
        );
        let plan = p
            .project
            .pack_plan(
                p.walter,
                &req.spec,
                &PluginFilters::NONE,
                &req.naming,
                &as_export::builtin_presets(),
                &Caps::default(),
            )
            .unwrap();
        assert_eq!((plan.file.as_deref(), plan.folder.as_deref()), (Some("Show_Walter_scenepack"), Some("Walter")));
        assert_eq!(plan.names.len(), 3);
        // A file name can't use what only names a shot.
        req.naming.file = "{series} {tc}".into();
        let err = p.project.pack_export_request(p.walter, &req, &PluginFilters::NONE, &out).unwrap_err();
        assert!(err.to_string().contains("names a shot"), "{err}");
        // With the shots' names broken too, the file's problem comes first.
        req.naming.template = "{series} {nme}".into();
        let err = p
            .project
            .pack_plan(p.walter, &req.spec, &PluginFilters::NONE, &req.naming, &[], &Caps::default())
            .unwrap_err();
        assert!(err.to_string().contains("names a shot"), "{err}");
        // Settings saved before there was a choice load with one file.
        let naming: PackNaming = with_defaults(r#"{"template":"{character} {index}","folder":false}"#);
        assert_eq!(
            naming,
            PackNaming { template: "{character} {index}".into(), folder: false, ..PackNaming::default() }
        );
        assert!(naming.merge);
    }

    #[test]
    fn settings_of_another_version_load_with_the_defaults_filled_in() {
        let spec: PackSpec = with_defaults(r#"{"solo":true,"pickedColour":"red"}"#);
        assert_eq!(spec, PackSpec { solo: true, ..PackSpec::default() });
        let naming: PackNaming = with_defaults("not json");
        assert_eq!(naming, PackNaming::default());
        // A field of the wrong shape can't be read: all defaults.
        let spec: PackSpec = with_defaults(r#"{"limit":"ten"}"#);
        assert_eq!(spec, PackSpec::default());
    }

    const SPEAKS: &str = "dev.a.speech/speaks";

    /// The filter "Speaks" of the plugin "Speech Filter", its verdicts
    /// judged on `basis` now, doing `state`.
    fn speaks(basis: &str, state: PackFilterState) -> PluginFilters {
        PluginFilters(vec![PluginFilter {
            key: SPEAKS.into(),
            title: "Speaks".into(),
            description: Some("Keeps the shots where they talk.".into()),
            reason: "No speech".into(),
            plugin: "Speech Filter".into(),
            basis: basis.into(),
            state,
        }])
    }

    /// The pack's spec with "Speaks" on.
    fn on() -> PackSpec {
        with(|s| s.plugin_filters = vec![SPEAKS.into()])
    }

    fn judge(p: &Pack, c: CharacterId, basis: &str, verdicts: &[(FrameIdx, bool)]) {
        assert!(p.project.keep_verdicts(SPEAKS, c, p.media, basis, verdicts).unwrap());
    }

    fn filtered(p: &Pack, spec: &PackSpec, filters: &PluginFilters) -> PackPreview {
        p.project.pack_preview(p.walter, spec, filters).unwrap()
    }

    #[test]
    fn a_drop_verdict_leaves_a_shot_out_with_its_filter() {
        let p = pack();
        let filters = speaks("b1", PackFilterState::Ready);
        judge(&p, p.walter, "b1", &[(50, false), (0, true), (290, true)]);
        let v = filtered(&p, &on(), &filters);
        assert_eq!(starts(&v, true), [0, 290]);
        let c = clip(&v, 50);
        assert_eq!((c.reason, c.filter, c.by_rules, c.manual), (Some(PackReason::Plugin), Some(0), false, None));
        assert_eq!((clip(&v, 0).filter, clip(&v, 48).reason), (None, Some(PackReason::Tiny)));
        assert_eq!(
            v.filters,
            [PackFilter {
                key: SPEAKS.into(),
                title: "Speaks".into(),
                description: Some("Keeps the shots where they talk.".into()),
                reason: "No speech".into(),
                plugin: "Speech Filter".into(),
                checked: 3,
                total: 3,
                state: PackFilterState::Ready,
            }]
        );
        assert_eq!(v.unchecked, 0);
        // Its choice names the filter too.
        let choices = p.project.pack_choices(p.walter, &on(), &filters).unwrap();
        let at = v.clips.iter().position(|c| c.shot.start == 50).unwrap();
        assert_eq!(choices.choices[at].filter, Some(0));

        // The chip off: the shot is in, and the filter still says what it judged.
        let off = filtered(&p, &PackSpec::default(), &filters);
        assert_eq!((starts(&off, true), off.filters[0].checked), (vec![0, 50, 290], 3));
        assert_ne!(off.stamp, v.stamp);
        // A limit keeps the best of what the filters take, the plugin's too.
        let v = filtered(&p, &with(|s| (s.plugin_filters, s.limit) = (vec![SPEAKS.into()], Some(1))), &filters);
        assert_eq!((starts(&v, true), clip(&v, 50).reason), (vec![0], Some(PackReason::Plugin)));

        // An export takes what was shown; without the verdicts, the pack changed.
        let out = p.dir.path().join("Exports");
        let req = PackExportRequest {
            spec: on(),
            naming: PackNaming { merge: false, ..PackNaming::default() },
            preset_id: "h264-edit".into(),
            out_dir: None,
            stamp: filtered(&p, &on(), &filters).stamp,
        };
        let (request, _) = p.project.pack_export_request(p.walter, &req, &filters, &out).unwrap();
        assert_eq!(request.clips.iter().map(|c| c.start).collect::<Vec<_>>(), [0, 290]);
        assert!(p.project.pack_export_request(p.walter, &req, &PluginFilters::NONE, &out).is_err());
    }

    #[test]
    fn a_hand_pick_beats_a_verdict() {
        let p = pack();
        let filters = speaks("b1", PackFilterState::Ready);
        judge(&p, p.walter, "b1", &[(50, false)]);
        let key = PackKey { media_id: p.media, shot: 50 };
        p.project.set_pack_overrides(p.walter, &[key], Some(true)).unwrap();
        let c = clip(&filtered(&p, &on(), &filters), 50).clone();
        assert_eq!((c.included, c.reason, c.by_rules, c.manual, c.filter), (true, None, false, Some(true), Some(0)));
        // Handed back to the filters: the verdict counts again.
        p.project.set_pack_overrides(p.walter, &[key], None).unwrap();
        let c = clip(&filtered(&p, &on(), &filters), 50).clone();
        assert_eq!((c.included, c.reason, c.manual), (false, Some(PackReason::Plugin), None));
        // A keep doesn't undo a drop by hand.
        judge(&p, p.walter, "b1", &[(0, true)]);
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 0 }], Some(false)).unwrap();
        assert_eq!(clip(&filtered(&p, &on(), &filters), 0).reason, Some(PackReason::Removed));
    }

    #[test]
    fn shots_without_a_verdict_stay_in_and_count_as_unchecked() {
        let p = pack();
        let filters = speaks("b1", PackFilterState::Ready);
        judge(&p, p.walter, "b1", &[(0, true)]);
        let v = filtered(&p, &on(), &filters);
        assert_eq!(starts(&v, true), [0, 50, 290]);
        assert_eq!((v.unchecked, v.filters[0].checked, v.filters[0].total), (2, 1, 3));
        // Only shots the pack's own filters take are counted: the flash cut too, once it is taken.
        let all = with(|s| (s.plugin_filters, s.skip_tiny) = (vec![SPEAKS.into()], false));
        let v = filtered(&p, &all, &filters);
        assert_eq!((v.unchecked, v.filters[0].total), (3, 4));
        // The chip off: nothing it lacks counts against the pack.
        assert_eq!(filtered(&p, &PackSpec::default(), &filters).unchecked, 0);

        // A check goes on with the episode, and sends the shots it lacks.
        let next = |after| p.project.filter_next(p.walter, &on(), &filters, SPEAKS, after).unwrap();
        assert_eq!((next(None), next(Some(p.media))), (Some(p.media), None));
        let check = p.project.filter_shots(p.walter, &on(), &filters, SPEAKS, p.media).unwrap().unwrap();
        assert_eq!((check.character, check.name.as_str(), check.media), (p.walter, "Walter", p.media));
        let episode = p.project.media(p.media).unwrap();
        assert_eq!((check.path, check.fps, check.frames), (episode.path, Some(Rational::new(24, 1)), Some(FRAMES)));
        let shots: Vec<_> = check.shots.iter().map(|s| (s.start, s.end, s.faceless)).collect();
        assert_eq!(shots, [(50, 290, false), (290, 410, false)]);
        // With the box of Walter's clearest face in it.
        let (frame, b) = check.shots[0].face.unwrap();
        assert!((50..290).contains(&frame) && (b[3] - b[1] - 0.5).abs() < 1e-6, "{frame} {b:?}");
        assert!((check.shots[0].face_height - 0.5).abs() < 1e-6);
        // Unknown filters judge nothing.
        assert!(p.project.filter_shots(p.walter, &on(), &filters, "dev.a.other/x", p.media).unwrap().is_none());

        judge(&p, p.walter, "b1", &[(50, true), (290, false)]);
        let v = filtered(&p, &on(), &filters);
        assert_eq!((v.unchecked, v.filters[0].checked, starts(&v, true)), (0, 3, vec![0, 50]));
        assert_eq!(next(None), None);
        assert!(p.project.filter_shots(p.walter, &on(), &filters, SPEAKS, p.media).unwrap().is_none());
    }

    #[test]
    fn a_check_goes_through_the_episodes_in_their_order() {
        // Added as 03, 01, 02: their ids run in another order than the episodes.
        let names = ["Show S01E03.mkv", "Show S01E01.mkv", "Show S01E02.mkv"];
        let (_dir, project, ids) = crate::analysis::tests::project_with_episodes(&names, FRAMES);
        let (e3, e1, e2) = (ids[0], ids[1], ids[2]);
        assert!(e3 < e1 && e1 < e2, "ids in the order the files were added");
        let position = |m| project.media(m).unwrap().position;
        assert!(position(e1) < position(e2) && position(e2) < position(e3));
        for m in [e1, e2, e3] {
            project.db().store_segments(m, &shots(290)).unwrap();
            project.store_face_chunk(m, 0, FRAMES, &tracks::encode(&faces(290)), &[], &[]).unwrap();
            project.finish_faces(m).unwrap();
        }
        let tracks: Vec<u32> = project
            .cast_tracks(TrackFilter::Unassigned)
            .unwrap()
            .iter()
            .filter(|t| (t.face_height - 0.2).abs() > 0.01)
            .map(|t| t.id)
            .collect();
        assert_eq!(tracks.len(), 12);
        let walter =
            project.cast_edit(&CastEdit::NewCharacter { tracks, name: Some("Walter".into()) }).unwrap().unwrap();
        let filters = speaks("b1", PackFilterState::Ready);
        let next = |after| project.filter_next(walter, &on(), &filters, SPEAKS, after).unwrap();
        // In episode order, each after the one before, and none after the last.
        assert_eq!((next(None), next(Some(e1)), next(Some(e2)), next(Some(e3))), (Some(e1), Some(e2), Some(e3), None));
        // An episode judged is passed over, from the start and after the one before it.
        let judge =
            |m| assert!(project.keep_verdicts(SPEAKS, walter, m, "b1", &[(0, true), (50, true), (290, true)]).unwrap());
        judge(e2);
        assert_eq!((next(None), next(Some(e1))), (Some(e1), Some(e3)));
        // A check goes on after its episode, never back to an earlier one that still lacks verdicts.
        assert_eq!(next(Some(e3)), None);
        // Its episode removed meanwhile: it goes on with the first that lacks them.
        project.remove_media(e2).unwrap();
        assert_eq!(next(Some(e2)), Some(e1));
        judge(e1);
        judge(e3);
        assert_eq!(next(None), None);
    }

    #[test]
    fn a_filter_of_a_plugin_that_isn_t_active_is_ignored() {
        let p = pack();
        judge(&p, p.walter, "b1", &[(50, false)]);
        let off = speaks("b1", PackFilterState::Off);
        let v = filtered(&p, &on(), &off);
        assert_eq!((starts(&v, true), v.unchecked), (vec![0, 50, 290], 0));
        assert_eq!((v.filters[0].state, v.filters[0].checked, v.filters[0].total), (PackFilterState::Off, 0, 0));
        assert_eq!(p.project.filter_next(p.walter, &on(), &off, SPEAKS, None).unwrap(), None);
        // One the app doesn't list (its plugin is gone) is ignored too.
        let gone = with(|s| s.plugin_filters = vec!["dev.a.gone/speaks".into()]);
        let v = filtered(&p, &gone, &PluginFilters::NONE);
        assert_eq!((starts(&v, true), v.unchecked, v.filters.len()), (vec![0, 50, 290], 0, 0));
        // Checking or failed, it still applies.
        for state in [PackFilterState::Checking, PackFilterState::Failed] {
            assert_eq!(starts(&filtered(&p, &on(), &speaks("b1", state)), true), [0, 290]);
        }
    }

    #[test]
    fn stale_verdicts_apply_until_new_ones_come() {
        let p = pack();
        judge(&p, p.walter, "b1", &[(50, false), (290, true)]);
        // A new setting: the old word stands, and every shot counts as unchecked.
        let now = speaks("b2", PackFilterState::Ready);
        let v = filtered(&p, &on(), &now);
        assert_eq!((starts(&v, true), v.unchecked, v.filters[0].checked), (vec![0, 290], 3, 0));
        // New verdicts replace the old ones shot by shot.
        judge(&p, p.walter, "b2", &[(290, false)]);
        let v = filtered(&p, &on(), &now);
        assert_eq!((starts(&v, true), v.unchecked, v.filters[0].checked), (vec![0], 2, 1));
        judge(&p, p.walter, "b2", &[(50, true), (0, true)]);
        let v = filtered(&p, &on(), &now);
        assert_eq!((starts(&v, true), v.unchecked, v.filters[0].checked), (vec![0, 50], 0, 3));
    }

    #[test]
    fn two_characters_in_one_shot_get_their_own_verdicts() {
        let p = pack();
        let filters = speaks("b1", PackFilterState::Ready);
        // Walter doesn't speak in shot 0; Jesse, with him there, isn't judged yet.
        judge(&p, p.walter, "b1", &[(0, false)]);
        let jesse = |p: &Pack| p.project.pack_preview(p.jesse, &on(), &filters).unwrap();
        let v = jesse(&p);
        assert_eq!((starts(&v, true), v.unchecked), (vec![0], 1));
        judge(&p, p.jesse, "b1", &[(0, true)]);
        assert_eq!((starts(&jesse(&p), true), jesse(&p).unchecked), (vec![0], 0));
        assert_eq!(clip(&filtered(&p, &on(), &filters), 0).reason, Some(PackReason::Plugin));
        // A check of Jesse's shots sends his alone.
        judge(&p, p.jesse, "b2", &[]);
        let now = speaks("b2", PackFilterState::Ready);
        let check = p.project.filter_shots(p.jesse, &on(), &now, SPEAKS, p.media).unwrap().unwrap();
        assert_eq!((check.name.as_str(), check.shots.len(), check.shots[0].start), ("Jesse", 1, 0));
    }

    #[test]
    fn an_old_spec_without_plugin_filters_reads() {
        let spec: PackSpec = with_defaults(r#"{"solo":true,"limit":3}"#);
        assert_eq!(spec, PackSpec { solo: true, limit: Some(3), ..PackSpec::default() });
        assert!(spec.plugin_filters.is_empty());
        // As serde reads one from an older app.
        let old = r#"{"minSec":null,"maxSec":null,"sizes":[],"solo":false,"skipTiny":true,"media":[],"order":"episodes","limit":null,"handles":0}"#;
        assert_eq!(serde_json::from_str::<PackSpec>(old).unwrap(), PackSpec::default());
        // Kept and read back with them.
        let p = pack();
        let state = PackState { spec: on(), naming: PackNaming::default() };
        p.project.save_pack_state(p.walter, &state).unwrap();
        assert_eq!(p.project.pack_state(p.walter).unwrap().spec.plugin_filters, [SPEAKS]);
    }

    /// Each clip of `clips` as (episode, first frame, end, name).
    fn cuts(clips: &[ClipRequest]) -> Vec<(MediaId, FrameIdx, FrameIdx, String)> {
        clips.iter().map(|c| (c.media_id, c.start, c.end, c.name.clone().unwrap_or_default())).collect()
    }

    #[test]
    fn a_pack_action_gets_the_clips_an_export_would_cut() {
        let p = pack();
        let spec = with(|s| (s.handles, s.order) = (2, PackOrder::Best));
        let naming = PackNaming { merge: false, ..PackNaming::default() };
        let v = preview(&p, &spec);
        let out = p.dir.path().join("Exports");
        let req = |spec: &PackSpec, naming: &PackNaming, stamp: &str| PackExportRequest {
            spec: spec.clone(),
            naming: naming.clone(),
            preset_id: "h264-edit".into(),
            out_dir: None,
            stamp: stamp.into(),
        };
        let export = |spec: &PackSpec, naming: &PackNaming, stamp: &str, filters: &PluginFilters| {
            p.project.pack_export_request(p.walter, &req(spec, naming, stamp), filters, &out).unwrap().0.clips
        };
        let cut = p.project.pack_cut(p.walter, &spec, &PluginFilters::NONE, &naming, &v.stamp).unwrap();
        // It only reads: unlike an export, it keeps no settings for next time.
        assert_eq!(p.project.pack_state(p.walter).unwrap(), PackState::default());
        // The clips an export cuts, named alike, in the pack's order: the best first here.
        assert_eq!(cuts(&cut.clips), cuts(&export(&spec, &naming, &v.stamp, &PluginFilters::NONE)));
        let shown: Vec<_> = v.clips.iter().filter(|c| c.included).map(|c| (c.shot.media_id, c.start, c.end)).collect();
        assert_eq!(cut.clips.iter().map(|c| (c.media_id, c.start, c.end)).collect::<Vec<_>>(), shown);
        assert_eq!(cut.clips[0].start, 48, "the best first, with its handles");
        assert_eq!((cut.character, cut.name.as_str()), (p.walter, "Walter"));
        // Its one episode, with its file, frame rate and frames.
        let m = p.project.media(p.media).unwrap();
        let episodes: Vec<_> = cut.episodes.iter().map(|e| (e.media, e.path.clone(), e.fps, e.frames)).collect();
        assert_eq!(episodes, [(p.media, m.path.clone(), m.fps, m.frames)]);
        assert!(m.fps.is_some() && m.frames == Some(FRAMES));
        // In one file the names are its chapters', as the export's.
        let joined = PackNaming::default();
        let one = p.project.pack_cut(p.walter, &spec, &PluginFilters::NONE, &joined, &v.stamp).unwrap();
        assert_eq!(cuts(&one.clips), cuts(&export(&spec, &joined, &v.stamp, &PluginFilters::NONE)));

        // A plugin filter's drop is out, as the grid shows it.
        let filters = speaks("b1", PackFilterState::Ready);
        judge(&p, p.walter, "b1", &[(50, false), (0, true), (290, true)]);
        let v = filtered(&p, &on(), &filters);
        let cut = p.project.pack_cut(p.walter, &on(), &filters, &naming, &v.stamp).unwrap();
        assert_eq!(cut.clips.iter().map(|c| c.start).collect::<Vec<_>>(), [0, 290]);
        assert_eq!(cuts(&cut.clips), cuts(&export(&on(), &naming, &v.stamp, &filters)));
    }

    #[test]
    fn a_pack_action_names_the_episodes_in_the_project_s_order() {
        // Added as 03, 01, 02, and Walter big (his best shot) only in 03: the
        // pack's best order starts there, the episodes are in the project's.
        let names = ["Show S01E03.mkv", "Show S01E01.mkv", "Show S01E02.mkv"];
        let (_dir, project, ids) = crate::analysis::tests::project_with_episodes(&names, FRAMES);
        let (e3, e1, e2) = (ids[0], ids[1], ids[2]);
        for m in [e1, e2, e3] {
            let mut seen = faces(290).to_vec();
            if m != e3 {
                seen.remove(3);
            }
            project.db().store_segments(m, &shots(290)).unwrap();
            project.store_face_chunk(m, 0, FRAMES, &tracks::encode(&seen), &[], &[]).unwrap();
            project.finish_faces(m).unwrap();
        }
        let tracks: Vec<u32> = project
            .cast_tracks(TrackFilter::Unassigned)
            .unwrap()
            .iter()
            .filter(|t| (t.face_height - 0.2).abs() > 0.01)
            .map(|t| t.id)
            .collect();
        assert_eq!(tracks.len(), 10);
        let walter =
            project.cast_edit(&CastEdit::NewCharacter { tracks, name: Some("Walter".into()) }).unwrap().unwrap();
        let spec = with(|s| s.order = PackOrder::Best);
        let naming = PackNaming::default();
        let v = project.pack_preview(walter, &spec, &PluginFilters::NONE).unwrap();
        let cut = project.pack_cut(walter, &spec, &PluginFilters::NONE, &naming, &v.stamp).unwrap();
        let from: Vec<MediaId> = cut.clips.iter().map(|c| c.media_id).collect();
        assert_eq!(from[0], e3, "the best first");
        assert!(from.contains(&e1) && from.contains(&e2), "{from:?}");
        let episodes: Vec<MediaId> = cut.episodes.iter().map(|e| e.media).collect();
        assert_eq!(episodes, [e1, e2, e3]);
        // Each with its own file.
        let path = |m| project.media(m).unwrap().path;
        assert_eq!(cut.episodes.iter().map(|e| e.path.clone()).collect::<Vec<_>>(), [path(e1), path(e2), path(e3)]);
    }

    #[test]
    fn a_changed_pack_is_refused_as_for_export() {
        let p = pack();
        let spec = PackSpec::default();
        let naming = PackNaming::default();
        let v = preview(&p, &spec);
        let out = p.dir.path().join("Exports");
        let refused = |spec: &PackSpec, naming: &PackNaming, stamp: &str| {
            let req = PackExportRequest {
                spec: spec.clone(),
                naming: naming.clone(),
                preset_id: "h264-edit".into(),
                out_dir: None,
                stamp: stamp.into(),
            };
            let export = p.project.pack_export_request(p.walter, &req, &PluginFilters::NONE, &out).unwrap_err();
            let cut = p.project.pack_cut(p.walter, spec, &PluginFilters::NONE, naming, stamp).err().unwrap();
            assert_eq!(cut.to_string(), export.to_string());
            cut.to_string()
        };
        // Other settings than those it was shown with, or a pick since: the export's words.
        let changed = "The pack changed meanwhile. Check it and export again.";
        assert!(refused(&with(|s| s.solo = true), &naming, &v.stamp).contains(changed));
        p.project.set_pack_overrides(p.walter, &[PackKey { media_id: p.media, shot: 50 }], Some(false)).unwrap();
        assert!(refused(&spec, &naming, &v.stamp).contains(changed));
        // Nothing in it, or names it can't make: refused alike.
        let none = with(|s| s.min_sec = Some(600.0));
        let empty = preview(&p, &none);
        assert!(refused(&none, &naming, &empty.stamp).contains("nothing in the pack"));
        let bad = PackNaming { template: "{series} {nme}".into(), ..PackNaming::default() };
        let now = p.project.pack_choices(p.walter, &spec, &PluginFilters::NONE).unwrap();
        refused(&spec, &bad, &now.stamp);
        // As the pack is now, it is cut.
        assert_eq!(
            p.project.pack_cut(p.walter, &spec, &PluginFilters::NONE, &naming, &now.stamp).unwrap().clips.len(),
            2
        );
    }

    #[test]
    fn a_plan_names_and_sizes_the_pack() {
        let p = pack();
        let naming =
            PackNaming { template: "{character} {index}".into(), folder: true, merge: false, ..PackNaming::default() };
        let presets = as_export::builtin_presets();
        let plan = p
            .project
            .pack_plan(p.walter, &PackSpec::default(), &PluginFilters::NONE, &naming, &presets, &Caps::default())
            .unwrap();
        assert_eq!(plan.names, ["Walter_001", "Walter_002", "Walter_003"]);
        assert_eq!((plan.file, plan.folder.as_deref()), (None, Some("Walter")));
        assert!(plan.sizes.iter().all(|s| s.bytes > 0.0));
    }

    const FPS_24: Option<Rational> = Some(Rational { num: 24, den: 1 });

    /// The first frame of each group.
    fn group_starts(groups: &[ShotGroup]) -> Vec<FrameIdx> {
        groups.iter().map(|g| g.shots[0].start).collect()
    }

    /// One 24 fps episode cut into `segments`, with Walter's face (0.3 tall)
    /// in the shots starting at `walter` and Jesse's (0.2 tall) in the
    /// shots starting at `jesse`; each face spans its shot.
    fn cast_in(
        segments: &[Segment],
        walter: &[FrameIdx],
        jesse: &[FrameIdx],
    ) -> (tempfile::TempDir, Arc<Project>, CharacterId) {
        let (dir, project, media) = project_with_media(FRAMES);
        project.db().store_segments(media, segments).unwrap();
        let shot = |start: FrameIdx| segments.iter().find(|s| s.start == start && s.kind == SegmentKind::Shot).unwrap();
        let face = |who, start: FrameIdx, height| {
            let s = shot(start);
            sized(seen(who, s.start, s.start, s.end, 0.1), height)
        };
        let mut faces: Vec<FaceTrack> = walter.iter().map(|&s| face(1, s, 0.3)).collect();
        faces.extend(jesse.iter().map(|&s| face(2, s, 0.2)));
        faces.sort_by_key(|t| t.start);
        project.store_face_chunk(media, 0, FRAMES, &tracks::encode(&faces), &[], &[]).unwrap();
        project.finish_faces(media).unwrap();
        let tracks = project.cast_tracks(TrackFilter::Unassigned).unwrap();
        let pick =
            |h: f32| -> Vec<u32> { tracks.iter().filter(|t| (t.face_height - h).abs() < 0.01).map(|t| t.id).collect() };
        let new = |tracks: Vec<u32>, name: &str| {
            project.cast_edit(&CastEdit::NewCharacter { tracks, name: Some(name.into()) }).unwrap().unwrap()
        };
        let walter = new(pick(0.3), "Walter");
        if !jesse.is_empty() {
            new(pick(0.2), "Jesse");
        }
        (dir, project, walter)
    }

    fn segment(start: FrameIdx, end: FrameIdx, kind: SegmentKind) -> Segment {
        Segment { start, end, kind, tiny: false }
    }

    #[test]
    fn shots_with_someone_else_between_share_a_group() {
        // A 0–48, B 48–100 (Jesse's), A 100–150: shot and reverse shot.
        let segments = [
            segment(0, 48, SegmentKind::Shot),
            segment(48, 100, SegmentKind::Shot),
            segment(100, 150, SegmentKind::Shot),
        ];
        let (_dir, project, walter) = cast_in(&segments, &[0, 100], &[48]);
        let groups = project.character_shot_groups(walter).unwrap();
        assert_eq!(groups.len(), 1);
        let shots: Vec<_> = groups[0].shots.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(shots, [(0, 48), (100, 150)]);
    }

    #[test]
    fn a_gap_over_six_seconds_starts_a_group() {
        // 6 s at 24 fps: 144 frames.
        assert_eq!(group_lengths(&[(0, 48), (192, 240)], &[], FPS_24), [2]);
        assert_eq!(group_lengths(&[(0, 48), (193, 240)], &[], FPS_24), [1, 1]);
        // At 23.976 fps 6 s is 143.86 frames, rounded up: 144 still joins.
        let ntsc = Some(Rational::new(24000, 1001));
        assert_eq!(group_lengths(&[(0, 48), (192, 240)], &[], ntsc), [2]);
        assert_eq!(group_lengths(&[(0, 48), (193, 240)], &[], ntsc), [1, 1]);
        // Each gap counts on its own: a chain can be longer than 6 s.
        assert_eq!(group_lengths(&[(0, 48), (100, 148), (200, 248), (500, 548)], &[], FPS_24), [3, 1]);
    }

    #[test]
    fn black_or_a_fade_between_starts_a_group_however_close() {
        let shots = [(0, 48), (60, 100)];
        let r = FrameRange::new;
        // In the gap.
        assert_eq!(group_lengths(&shots, &[r(50, 55)], FPS_24), [1, 1]);
        // Filling it, or touching either end of it.
        assert_eq!(group_lengths(&shots, &[r(48, 60)], FPS_24), [1, 1]);
        assert_eq!(group_lengths(&shots, &[r(44, 48)], FPS_24), [1, 1]);
        assert_eq!(group_lengths(&shots, &[r(60, 64)], FPS_24), [1, 1]);
        // However short the gap.
        assert_eq!(group_lengths(&[(0, 48), (49, 100)], &[r(48, 49)], FPS_24), [1, 1]);
        // Fades elsewhere in the episode don't part them.
        assert_eq!(group_lengths(&shots, &[r(10, 20), r(30, 47), r(61, 70), r(200, 210)], FPS_24), [2]);
    }

    #[test]
    fn a_dissolve_between_keeps_the_group() {
        // Walter at 0, 60 and 136; a dissolve, a flash, then a fade through black.
        let segments = [
            segment(0, 48, SegmentKind::Shot),
            segment(48, 60, SegmentKind::Dissolve),
            segment(60, 100, SegmentKind::Shot),
            segment(100, 101, SegmentKind::Flash),
            segment(101, 120, SegmentKind::Shot),
            segment(120, 124, SegmentKind::FadeOut),
            segment(124, 130, SegmentKind::Black),
            segment(130, 136, SegmentKind::FadeIn),
            segment(136, 200, SegmentKind::Shot),
        ];
        let (_dir, project, walter) = cast_in(&segments, &[0, 60, 101, 136], &[]);
        let groups = project.character_shot_groups(walter).unwrap();
        assert_eq!(group_starts(&groups), [0, 136]);
        assert_eq!(groups[0].shots.len(), 3);
    }

    /// A shot of the character, as [`groups_of`] takes it.
    fn shot_in(media: MediaId, start: FrameIdx, end: FrameIdx) -> CharacterShot {
        CharacterShot {
            media_id: media,
            start,
            end,
            sprite: None,
            cells: 0,
            track_id: 0,
            face: String::new(),
            face_height: 0.0,
            faceless: false,
            bodies: Vec::new(),
        }
    }

    #[test]
    fn another_episode_starts_a_group() {
        let (e1, e2) = (MediaId::new(), MediaId::new());
        let fps = HashMap::from([(e1, FPS_24.unwrap()), (e2, FPS_24.unwrap())]);
        // The last shot of one episode and the first of the next, frames apart.
        let shots = vec![shot_in(e1, 0, 48), shot_in(e1, 50, 100), shot_in(e2, 0, 48), shot_in(e2, 60, 90)];
        let groups = groups_of(shots, &fps, &HashMap::new());
        let media: Vec<Vec<MediaId>> = groups.iter().map(|g| g.shots.iter().map(|s| s.media_id).collect()).collect();
        assert_eq!(media, [vec![e1, e1], vec![e2, e2]]);
    }

    #[test]
    fn without_a_frame_rate_every_shot_is_its_own_group() {
        let shots = [(0, 48), (48, 96), (100, 150)];
        assert_eq!(group_lengths(&shots, &[], None), [1, 1, 1]);
        // A broken one is no frame rate either.
        assert_eq!(group_lengths(&shots, &[], Some(Rational { num: 0, den: 1 })), [1, 1, 1]);
        assert_eq!(group_lengths(&shots, &[], FPS_24), [3]);
        assert!(group_lengths(&[], &[], FPS_24).is_empty());
        // An episode without one, beside an episode with one.
        let (known, unknown) = (MediaId::new(), MediaId::new());
        let fps = HashMap::from([(known, FPS_24.unwrap())]);
        let all =
            vec![shot_in(known, 0, 48), shot_in(known, 48, 96), shot_in(unknown, 0, 48), shot_in(unknown, 48, 96)];
        let sizes: Vec<usize> = groups_of(all, &fps, &HashMap::new()).iter().map(|g| g.shots.len()).collect();
        assert_eq!(sizes, [2, 1, 1]);
    }

    #[test]
    fn groups_hold_every_shot_once_in_order() {
        let p = pack();
        let key = |s: &CharacterShot| (s.media_id, s.start, s.end);
        let flat = |groups: &[ShotGroup]| groups.iter().flat_map(|g| g.shots.iter().map(key)).collect::<Vec<_>>();
        let groups = p.project.character_shot_groups(p.walter).unwrap();
        let shots: Vec<_> = p.project.character_shots(p.walter).unwrap().iter().map(key).collect();
        assert_eq!(flat(&groups), shots);
        // Its four shots touch: one group.
        assert_eq!((groups.len(), groups[0].shots.len()), (1, 4));

        // Found again with black before the last shot: it starts a group.
        let segments = [
            segment(0, 48, SegmentKind::Shot),
            Segment { tiny: true, ..segment(48, 50, SegmentKind::Shot) },
            segment(50, 280, SegmentKind::Shot),
            segment(280, 290, SegmentKind::Black),
            segment(290, 410, SegmentKind::Shot),
        ];
        let mut found = faces(290);
        found[3] = sized(seen(1, 50, 50, 280, 0.1), 0.5);
        p.project.db().store_segments(p.media, &segments).unwrap();
        p.project.reset_faces(p.media).unwrap();
        p.project.store_face_chunk(p.media, 0, FRAMES, &tracks::encode(&found), &[], &[]).unwrap();
        p.project.finish_faces(p.media).unwrap();
        let groups = p.project.character_shot_groups(p.walter).unwrap();
        let shots: Vec<_> = p.project.character_shots(p.walter).unwrap().iter().map(key).collect();
        assert_eq!(flat(&groups), shots);
        assert_eq!(group_starts(&groups), [0, 290]);
        assert_eq!(groups.iter().map(|g| g.shots.len()).collect::<Vec<_>>(), [3, 1]);
    }
}
