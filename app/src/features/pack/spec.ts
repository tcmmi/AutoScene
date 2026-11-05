import type { MediaId, PackChoices, PackClip, PackFilter, PackPreview, PackReason, PackSpec, ShotSize } from "@/bindings"

/**
 * The scenepack builder's filters and picks, as plain data. The backend
 * (as_project::pack) makes the pack; this only names its choices and shows
 * a click at once, before the backend's answer replaces it.
 */

export const DEFAULT_SPEC: PackSpec = {
  minSec: null,
  maxSec: null,
  sizes: [],
  solo: false,
  skipTiny: true,
  skipFaceless: false,
  media: [],
  order: "episodes",
  limit: null,
  handles: 0,
  pluginFilters: [],
}

/** Shot sizes, closest first. */
export const SIZES: { value: ShotSize; label: string; short: string }[] = [
  { value: "extreme_close_up", label: "Extreme close-up", short: "ECU" },
  { value: "close_up", label: "Close-up", short: "CU" },
  { value: "medium_close_up", label: "Medium close-up", short: "MCU" },
  { value: "medium", label: "Medium shot", short: "MS" },
  { value: "wide", label: "Wide shot", short: "WS" },
]

/** Why a shot is left out, as its card says it. */
export const REASONS: Record<PackReason, string> = {
  missing: "File missing",
  episode: "Other episode",
  tiny: "Flash cut",
  short: "Too short",
  long: "Too long",
  size: "Other size",
  not_solo: "Not alone",
  faceless: "No face",
  limit: "Past the limit",
  removed: "Taken out",
  // A filter's own words, when the card knows which filter (`filter`).
  plugin: "Plugin filter",
}

/** Lengths to keep, in seconds. */
export const LENGTHS: { id: string; label: string; min: number | null; max: number | null }[] = [
  { id: "any", label: "Any length", min: null, max: null },
  { id: "1+", label: "1 s or longer", min: 1, max: null },
  { id: "2+", label: "2 s or longer", min: 2, max: null },
  { id: "1-6", label: "1 to 6 s", min: 1, max: 6 },
  { id: "2-10", label: "2 to 10 s", min: 2, max: 10 },
  { id: "-6", label: "Up to 6 s", min: null, max: 6 },
]

export const LIMITS: (number | null)[] = [null, 25, 50, 100, 200]
export const HANDLES: number[] = [0, 6, 12, 24]

/** The length choice `spec` makes (a range set elsewhere gets a label of its own). */
export function lengthOf(spec: PackSpec): { id: string | null; label: string } {
  const preset = LENGTHS.find((l) => l.min === spec.minSec && l.max === spec.maxSec)
  if (preset) return preset
  const s = (v: number) => `${Math.round(v * 10) / 10}`
  const label =
    spec.minSec != null && spec.maxSec != null
      ? `${s(spec.minSec)} to ${s(spec.maxSec)} s`
      : spec.minSec != null
        ? `${s(spec.minSec)} s or longer`
        : `Up to ${s(spec.maxSec ?? 0)} s`
  return { id: null, label }
}

export function isDefault(spec: PackSpec): boolean {
  return (
    spec.minSec === DEFAULT_SPEC.minSec &&
    spec.maxSec === DEFAULT_SPEC.maxSec &&
    spec.sizes.length === 0 &&
    spec.solo === DEFAULT_SPEC.solo &&
    spec.skipTiny === DEFAULT_SPEC.skipTiny &&
    (spec.skipFaceless ?? false) === DEFAULT_SPEC.skipFaceless &&
    spec.media.length === 0 &&
    spec.order === DEFAULT_SPEC.order &&
    spec.limit === DEFAULT_SPEC.limit &&
    spec.handles === DEFAULT_SPEC.handles &&
    pluginFiltersOf(spec).length === 0
  )
}

/**
 * How many filters `spec` turns on, as the Filters button counts them: each
 * control that keeps shots out, or lets flash cuts in, and each plugin filter
 * (one of a plugin that is off too: it is on, and Reset turns it off). The
 * order and the handles shape the pack, they leave no shot out.
 */
export function filtersOn(spec: PackSpec, media: readonly { id: MediaId }[]): number {
  const own = [
    spec.sizes.length > 0,
    spec.minSec != null || spec.maxSec != null,
    episodesOn(spec, media),
    spec.limit != null,
    spec.solo,
    !spec.skipTiny,
    spec.skipFaceless ?? false,
  ]
  return own.filter(Boolean).length + pluginFiltersOf(spec).length
}

/**
 * The pack keeps shots of some episodes only: it names some and leaves one of
 * the project's out. A pack named for episodes that are all still there (or
 * for ones since removed) shows them all, so it is not a filter.
 */
export const episodesOn = (spec: PackSpec, media: readonly { id: MediaId }[]): boolean =>
  spec.media.length > 0 && media.some((m) => !spec.media.includes(m.id))

const NONE: readonly string[] = []

/** The plugin filters `spec` turns on (`<pluginId>/<filterId>`); the same list while there are none. */
export const pluginFiltersOf = (spec: PackSpec): readonly string[] => spec.pluginFilters ?? NONE

/** `spec` with the plugin filter `key` turned on or off. */
export function withPluginFilter(spec: PackSpec, key: string, on: boolean): PackSpec {
  const rest = pluginFiltersOf(spec).filter((k) => k !== key)
  return { ...spec, pluginFilters: on ? [...rest, key] : rest }
}

/**
 * What the plugin filters a spec turns on are doing, as the footer says it:
 * a check of shots that waits or runs (Export waits for it), else one that
 * failed, was stopped or couldn't start. Until a shot is checked it keeps
 * the word the filter gave it with its settings before, and stays in when
 * there is none: the grid shows it as an export takes it, so Export goes
 * on. Filters of a plugin that is off count for nothing: the pack
 * ignores them.
 */
export type PackCheck =
  | { kind: "checking"; checked: number; total: number }
  | { kind: "failed"; unchecked: number }

/**
 * The check `filters` are doing for the pack, with `unchecked` the pack's
 * own count of shots a filter turned on hasn't judged with its settings as
 * they are (`PackPreview.unchecked`): a shot counts once, however many
 * filters are on. Every filter the pack reads weighs the same shots, so
 * one's `total` is the pack's.
 */
export function checkOf(filters: readonly PackFilter[], on: readonly string[], unchecked: number): PackCheck | null {
  const turnedOn = filters.filter((f) => on.includes(f.key) && f.state !== "off" && f.state !== "gone")
  const total = turnedOn.reduce((n, f) => Math.max(n, f.total), 0)
  const left = Math.min(Math.max(0, unchecked), total)
  if (turnedOn.some((f) => f.state === "checking")) return { kind: "checking", checked: total - left, total }
  const failed = turnedOn.some((f) => f.state === "failed" && f.checked < f.total)
  return failed && left > 0 ? { kind: "failed", unchecked: left } : null
}

/**
 * What a check says of its shots. A filter's words count once its episode's
 * run ends, so while the first runs (all of a film) there is no number to
 * give yet: how far that run is shows in Activity.
 */
export function checkingText(checked: number, total: number): string {
  const shots = total === 1 ? "1 shot" : `${total} shots`
  return checked > 0 ? `Checking ${checked} of ${shots}` : `Checking ${shots}`
}

/**
 * Whether the pack can be exported now: it takes shots, is shown as the
 * backend has it, has its character, isn't exported already, and no
 * filter of it checks shots (the backend's export waits for that too).
 */
export function exportable(p: {
  taken: number
  current: boolean
  character: boolean
  exporting: boolean
  check: PackCheck | null
}): boolean {
  return p.taken > 0 && p.current && p.character && !p.exporting && p.check?.kind !== "checking"
}

/**
 * Whether the pack can be saved with a plugin's action now: as it can be
 * exported, but also while an export of it runs (saving is a file the
 * pack's own clips go into, not a second export).
 */
export function savable(p: { taken: number; current: boolean; character: boolean; check: PackCheck | null }): boolean {
  return exportable({ ...p, exporting: false })
}

/**
 * What the plugins' save entries are headed with: their group, when it can't
 * be used now, says what it waits for (the ⋯ and the right-click menu alike).
 */
export function saveHeading(p: { taken: number; current: boolean; check: PackCheck | null }): string {
  if (p.check?.kind === "checking") return "Save with a plugin waits for the check"
  if (p.taken === 0) return "Save with a plugin needs a shot in the pack"
  if (!p.current) return "Save with a plugin waits for the pack"
  return "Save with a plugin"
}

/** The words a shot that a plugin's filter left out wears: the filter's own, else a general one. */
export const filterReason = (f: PackFilter | undefined): string => (f?.reason ? f.reason : REASONS.plugin)

/** A shot of the pack by its media and first frame — what picks by hand stick to. */
export const keyOf = (c: PackClip) => `${c.shot.mediaId}:${c.shot.start}`

/** A shot whose file is gone can't be taken. */
export const pickable = (c: PackClip) => c.reason !== "missing"

/**
 * How a click on every shot at once holds the shots it leaves be: the limit
 * fills the places they leave with the next best (`limit`), and a plugin's
 * check that is not done judges shots the grid shows as in (`check`).
 */
export type Pin = "limit" | "check"

/**
 * Picks that take `clips` in (`included`) or out: by hand where the filters
 * say otherwise, back to the filters where they agree — so taking a shot
 * out and in again leaves no pick behind. A click on every shot at once
 * (`pin`) hands nothing back, as a shot handed back would take a place from
 * one that stays, or be dropped when the check ends: each is picked. Under a
 * limit not those the limit can't touch as they are; during a check, all.
 */
export function picksFor(clips: PackClip[], included: boolean, pin?: Pin): { pick: PackClip[]; unpick: PackClip[] } {
  const pick: PackClip[] = []
  const unpick: PackClip[] = []
  for (const c of clips) {
    if (!pickable(c)) continue
    if (pin) {
      // A shot as asked and not held by the limit has a manual pick of that
      // answer or none: `included` and `manual` agree.
      const stays = pin === "limit" && c.included === included && c.reason !== "limit"
      if (!stays && c.manual !== included) pick.push(c)
    } else if (c.byRules === included) {
      if (c.manual != null) unpick.push(c)
    } else if (c.manual !== included) {
      pick.push(c)
    }
  }
  return { pick, unpick }
}

/** The shots from one card to another (both ends too), in the order shown. */
export function between(clips: PackClip[], from: string, to: string): PackClip[] {
  const a = clips.findIndex((c) => keyOf(c) === from)
  const b = clips.findIndex((c) => keyOf(c) === to)
  if (a < 0 || b < 0) return b < 0 ? [] : [clips[b]]
  return clips.slice(Math.min(a, b), Math.max(a, b) + 1)
}

/**
 * The preview with `clips` taken in or out at once, totals included. The
 * backend's answer follows within moments and replaces it — with what this
 * can't know: the reason a shot handed back to the filters is out, and the
 * next-best shot a limit lets in. Unchanged clips keep their identity, so
 * only the cards that changed draw again.
 */
export function withIncluded(
  p: PackPreview,
  clips: PackClip[],
  included: boolean,
  fpsOf: (mediaId: string) => number | null,
): PackPreview {
  return withPicks(p, new Map(clips.map((c) => [keyOf(c), included])), fpsOf)
}

/** The preview with shots (by [`keyOf`]) taken in or out; the same preview when nothing changes. */
export function withPicks(
  p: PackPreview,
  picks: ReadonlyMap<string, boolean>,
  fpsOf: (mediaId: string) => number | null,
): PackPreview {
  if (picks.size === 0) return p
  let changed = false
  const next = p.clips.map((c) => {
    const included = picks.get(keyOf(c))
    if (included === undefined || c.included === included || !pickable(c)) return c
    changed = true
    const manual = c.byRules === included ? null : included
    const reason: PackReason | null = included ? null : manual === false ? "removed" : null
    return { ...c, included, manual, reason }
  })
  return changed ? { ...p, clips: next, totals: totalsOf(next, fpsOf) } : p
}

/** Whether choices after a pick are a preview's: the same clips in the same order (else the pack changed otherwise). */
export const fits = (p: PackPreview, a: PackChoices) => p.shape === a.shape && p.clips.length === a.choices.length

/**
 * The preview as the backend has it after a pick: each clip's choice (in
 * the preview's order — see [`fits`]), the totals and the stamp. Clips
 * whose choice stays keep their identity, so only the cards that changed
 * draw again.
 */
export function withChoices(p: PackPreview, a: PackChoices): PackPreview {
  let changed = false
  const clips = p.clips.map((c, i) => {
    const { included, byRules, reason, manual, filter } = a.choices[i]
    if (c.included === included && c.byRules === byRules && c.reason === reason && c.manual === manual && c.filter === filter) {
      return c
    }
    changed = true
    return { ...c, included, byRules, reason, manual, filter }
  })
  return { ...p, clips: changed ? clips : p.clips, totals: a.totals, stamp: a.stamp }
}

export function totalsOf(clips: PackClip[], fpsOf: (mediaId: string) => number | null): PackPreview["totals"] {
  let n = 0
  let frames = 0
  let seconds = 0
  for (const c of clips) {
    if (!c.included) continue
    n += 1
    frames += c.end - c.start
    const fps = fpsOf(c.shot.mediaId)
    if (fps) seconds += (c.end - c.start) / fps
  }
  return { clips: n, frames, seconds }
}

/** What the grid shows. */
export type Show = "all" | "in" | "out"

export function shown(clips: PackClip[], show: Show): PackClip[] {
  if (show === "all") return clips
  return clips.filter((c) => c.included === (show === "in"))
}

/** Picks by hand in a preview: shots taken against the filters, either way. */
export const handPicked = (clips: PackClip[]) => clips.filter((c) => c.manual != null)
