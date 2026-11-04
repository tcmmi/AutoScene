/**
 * Mock scenepacks: the backend's rules (as_project::pack) in short, over the
 * mock cast's shots, so the pack builder can be built and checked in a
 * plain browser. Same filters, reasons, order, limit and hand picks.
 */
import type {
  CharacterShot,
  ExportPlan,
  MediaItem,
  PackChoices,
  PackClip,
  PackFilter,
  PackFilterState,
  PackKey,
  PackNaming,
  PackPreview,
  PackReason,
  PackSpec,
  PackState,
  Segment,
  ShotSize,
} from "@/bindings"
import { ALL_PARTS, FILE_PARTS, parseTemplate, reportedPart } from "@/lib/naming"
import { characterLabel, characterShots, othersInShot } from "./cast"
import type { PackFilterHost } from "./plugins"

const states = new Map<string, PackState>()
const picks = new Map<string, Map<string, boolean>>()

export function defaultPackState(): PackState {
  return {
    spec: {
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
    },
    naming: { template: "{series} {character} {episode} {tc}", folder: true, merge: true, file: "{series} {character} scenepack" },
  }
}

const keyOf = (pid: string, cid: string) => `${pid}:${cid}`

export function packGet(pid: string, cid: string): PackState {
  return structuredClone(states.get(keyOf(pid, cid)) ?? defaultPackState())
}

export function packSave(pid: string, cid: string, pack: PackState) {
  states.set(keyOf(pid, cid), structuredClone(pack))
}

export function packOverride(pid: string, cid: string, shots: PackKey[], include: boolean | null) {
  const m = picks.get(keyOf(pid, cid)) ?? new Map<string, boolean>()
  for (const s of shots) {
    if (include == null) m.delete(`${s.mediaId}:${s.shot}`)
    else m.set(`${s.mediaId}:${s.shot}`, include)
  }
  picks.set(keyOf(pid, cid), m)
}

function sizeOf(h: number): ShotSize {
  if (h >= 0.45) return "extreme_close_up"
  if (h >= 0.25) return "close_up"
  if (h >= 0.15) return "medium_close_up"
  if (h >= 0.08) return "medium"
  return "wide"
}

const lengthFit = (sec: number) => (sec < 1 ? sec : sec > 6 ? 6 / sec : 1)

type Cand = { clip: PackClip; rule: PackReason | null; seconds: number; tenths: number }

/** Every shot of the character with what the pack's own filters say of it (a plugin's are for the preview). */
function candidates(pid: string, cid: string, spec: PackSpec, media: MediaItem[], segments: Segment[]): Cand[] {
  const manual = picks.get(keyOf(pid, cid)) ?? new Map<string, boolean>()
  return characterShots(pid, cid, media, segments).flatMap((g) => g.shots).map((shot) => {
    const m = media.find((x) => x.id === shot.mediaId)!
    const fps = m.fps ? m.fps.num / m.fps.den : 24
    const seconds = (shot.end - shot.start) / fps
    const tiny = shot.end - shot.start < 3
    const others = othersInShot(pid, cid, shot.mediaId, shot.start, shot.end)
    const faceHeight = shot.faceHeight ?? 0
    const size = sizeOf(faceHeight)
    const quality = 0.6 + (shot.trackId % 4) * 0.1
    const score = 0.35 * quality + 0.3 * Math.min(faceHeight / 0.45, 1) + 0.2 + 0.1 * lengthFit(seconds) + (others === 0 ? 0.05 : 0)
    const rule: PackReason | null =
      m.status !== "ready"
        ? "missing"
        : spec.media.length > 0 && !spec.media.includes(m.id)
          ? "episode"
          : spec.skipTiny && tiny
            ? "tiny"
            : spec.skipFaceless && shot.faceless
              ? "faceless"
              : spec.minSec != null && seconds < spec.minSec
              ? "short"
              : spec.maxSec != null && seconds > spec.maxSec
                ? "long"
                : spec.sizes.length > 0 && !spec.sizes.includes(size)
                  ? "size"
                  : spec.solo && others > 0
                    ? "not_solo"
                    : null
    const frames = m.frames ?? shot.end
    const start = Math.max(0, shot.start - spec.handles)
    const end = Math.min(frames, shot.end + spec.handles)
    const pick = manual.get(`${shot.mediaId}:${shot.start}`)
    return {
      clip: {
        shot,
        start,
        end,
        size,
        quality,
        coverage: 1,
        others,
        score,
        included: false,
        byRules: rule == null,
        reason: rule,
        manual: pick ?? null,
        filter: null,
      },
      rule,
      seconds: (end - start) / fps,
      tenths: Math.round(seconds * 10),
    }
  })
}

/** The verdicts plugins' filters gave, by pack and filter: `keep` and the basis they were made with. */
const verdicts = new Map<string, Map<string, { keep: boolean; basis: string }>>()
const chain = (pid: string, cid: string, filter: string) => [pid, cid, filter].join(" ")
const shotKey = (s: Pick<CharacterShot, "mediaId" | "start">) => `${s.mediaId}:${s.start}`

/** The shots a filter has to judge in the pack `spec` makes (those the pack's own filters take that lack a verdict of its basis now), by episode in order. */
export function shotsToJudge(
  pid: string,
  cid: string,
  spec: PackSpec,
  media: MediaItem[],
  segments: Segment[],
  filter: PackFilterHost,
): CharacterShot[][] {
  const given = verdicts.get(chain(pid, cid, filter.key))
  const lacking = candidates(pid, cid, spec, media, segments)
    .filter((c) => c.rule == null && given?.get(shotKey(c.clip.shot))?.basis !== filter.basis)
    .map((c) => c.clip.shot)
  const episodes = new Map<string, CharacterShot[]>()
  for (const m of media) episodes.set(m.id, [])
  for (const shot of lacking) episodes.get(shot.mediaId)?.push(shot)
  return [...episodes.values()].filter((shots) => shots.length > 0)
}

/** Keep a filter's verdicts on `shots` (all but the first `skip`: a check that fails halfway). */
export function giveVerdicts(pid: string, cid: string, filter: PackFilterHost, shots: CharacterShot[], skip = 0) {
  const key = chain(pid, cid, filter.key)
  const given = verdicts.get(key) ?? new Map()
  for (const shot of shots.slice(0, shots.length - skip)) given.set(shotKey(shot), { keep: filter.keeps(shot), basis: filter.basis })
  verdicts.set(key, given)
}

export function packPreview(
  pid: string,
  cid: string,
  spec: PackSpec,
  media: MediaItem[],
  segments: Segment[],
  filters: PackFilterHost[] = [],
  stateOf: (key: string) => PackFilterState = () => "ready",
): PackPreview {
  const order = new Map(media.map((m, i) => [m.id, i]))
  const cands = candidates(pid, cid, spec, media, segments)
  // The plugins' filters judge what the pack's own take: a shot none of the
  // filters turned on has judged stays in and counts as unchecked; a verdict
  // of an earlier basis stands until the new one comes.
  const on = spec.pluginFilters ?? []
  const counted = filters.map(() => ({ checked: 0, total: 0 }))
  let unchecked = 0
  for (const c of cands) {
    if (c.rule != null) continue
    const given = (f: PackFilterHost) => verdicts.get(chain(pid, cid, f.key))?.get(shotKey(c.clip.shot))
    let unjudged = false
    filters.forEach((f, i) => {
      if (!f.active) return
      counted[i].total += 1
      if (given(f)?.basis === f.basis) counted[i].checked += 1
      else unjudged ||= on.includes(f.key)
    })
    unchecked += unjudged ? 1 : 0
    const dropping = filters.findIndex((f) => f.active && on.includes(f.key) && given(f)?.keep === false)
    if (dropping >= 0) {
      c.rule = "plugin"
      c.clip.byRules = false
      c.clip.filter = dropping
    }
  }
  const episode = (a: Cand, b: Cand) =>
    (order.get(a.clip.shot.mediaId) ?? 1e9) - (order.get(b.clip.shot.mediaId) ?? 1e9) || a.clip.shot.start - b.clip.shot.start
  // Shots without the character's face come behind the ones with it; then
  // the higher score, longer on screen, the bigger face, the earlier shot.
  const better = (a: Cand, b: Cand) =>
    Number(a.clip.shot.faceless ?? false) - Number(b.clip.shot.faceless ?? false) ||
    (b.clip.score ?? 0) - (a.clip.score ?? 0) ||
    b.tenths - a.tenths ||
    (b.clip.shot.faceHeight ?? 0) - (a.clip.shot.faceHeight ?? 0) ||
    episode(a, b)
  if (spec.limit != null) {
    const limit = spec.limit
    const kept = cands.filter((c) => c.rule == null && c.clip.manual == null).sort(better)
    // A pick by hand takes no place: the filters alone would leave it out
    // if it didn't make the limit without the pick.
    const pickedPast = cands.filter((c) => c.rule == null && c.clip.manual != null && kept.filter((k) => better(k, c) < 0).length >= limit)
    for (const c of [...kept.slice(limit), ...pickedPast]) {
      c.rule = "limit"
      c.clip.byRules = false
    }
  }
  for (const c of cands) {
    const missing = c.rule === "missing"
    c.clip.included = !missing && (c.clip.manual ?? c.rule == null)
    c.clip.reason = c.clip.manual === false ? "removed" : c.clip.manual === true && !missing ? null : c.rule
  }
  cands.sort(spec.order === "best" ? better : episode)
  const taken = cands.filter((c) => c.clip.included)
  return {
    clips: cands.map((c) => c.clip),
    totals: {
      clips: taken.length,
      frames: taken.reduce((s, c) => s + (c.clip.end - c.clip.start), 0),
      seconds: taken.reduce((s, c) => s + c.seconds, 0),
    },
    stamp: taken.map((c) => `${c.clip.shot.mediaId}:${c.clip.start}-${c.clip.end}`).join("|"),
    shape: cands.map(({ clip: c }) => `${c.shot.mediaId}:${c.shot.start}:${c.start}-${c.end}:${c.score}`).join("|"),
    filters: filters.map(
      (f, i): PackFilter => ({
        key: f.key,
        title: f.title,
        description: f.description,
        reason: f.reason,
        plugin: f.plugin,
        checked: counted[i].checked,
        total: counted[i].total,
        state: f.active ? stateOf(f.key) : "off",
      }),
    ),
    unchecked,
  }
}

/** What a pick answers (the backend's `pack_choices`): the pack's choices, in its order. */
export function packChoices(preview: PackPreview): PackChoices {
  return {
    choices: preview.clips.map(({ included, byRules, reason, manual, filter }) => ({ included, byRules, reason, manual, filter })),
    totals: preview.totals,
    stamp: preview.stamp,
    shape: preview.shape,
  }
}

/** `{a}, {b} or {c}`: parts as the backend's messages list them. */
function listed(parts: readonly string[], last: "or" | "and"): string {
  const spelled = parts.map((part) => `{${part}}`)
  return spelled.length < 2 ? spelled.join("") : `${spelled.slice(0, -1).join(", ")} ${last} ${spelled[spelled.length - 1]}`
}

/**
 * What the backend says about a name template that won't do
 * (as_project::naming), or null: about the part it finds first, in its order
 * (the one file's name looks twice, see `reportedPart`), else about an empty
 * name.
 */
function problem(template: string, forFile: boolean): string | null {
  const part = reportedPart(parseTemplate(template, forFile ? FILE_PARTS : ALL_PARTS), forFile)
  const name = part?.text.slice(1, -1)
  switch (part?.kind) {
    case "unclosed":
      return "A “{” isn't closed with “}”."
    case "unknown":
      return `“{${name}}” isn't a name part. Use ${listed(ALL_PARTS, "or")}.`
    case "shot":
      return `“{${name}}” names a shot, not the file. The file's name can use ${listed(FILE_PARTS, "and")}.`
  }
  return template.trim() === "" ? "The name would be empty." : null
}

/** Why a shot's or a chapter's name template won't do (as_project::naming::render_name), or null. */
export const templateProblem = (template: string) => problem(template, false)

/** Why the name of one file holding the whole pack won't do (as_project::naming::render_file_name), or null. */
export const fileProblem = (template: string) => problem(template, true)

/** The one file's name, simply. */
export function packFileName(pid: string, cid: string, title: string, naming: PackNaming): string {
  const name = naming.file.replaceAll("{series}", title).replaceAll("{character}", characterLabel(pid, cid))
  return name.trim().replace(/[<>:"/\\|?*]+/g, " ").replace(/\s+/g, "_")
}

/** The backend's template names, simply. */
export function packNames(
  pid: string,
  cid: string,
  title: string,
  preview: PackPreview,
  naming: PackNaming,
  media: MediaItem[],
  tcName: (f: number) => string,
): string[] {
  const taken = preview.clips.filter((c) => c.included)
  const width = Math.max(3, String(taken.length).length)
  return taken.map((c, i) => {
    const m = media.find((x) => x.id === c.shot.mediaId)
    const name = naming.template
      .replaceAll("{series}", title)
      .replaceAll("{character}", characterLabel(pid, cid))
      .replaceAll("{episode}", m?.label || m?.fileName.replace(/\.[^.]+$/, "") || "")
      .replaceAll("{index}", String(i + 1).padStart(width, "0"))
      .replaceAll("{tc}", tcName(c.start))
      .replaceAll("{frame}", String(c.start))
    return name.trim().replace(/[<>:"/\\|?*]+/g, " ").replace(/\s+/g, "_")
  })
}

export function packPlan(
  names: string[],
  preview: PackPreview,
  sizes: Record<string, number>,
  folder: string | null,
  file: string | null,
): ExportPlan {
  return {
    names,
    sizes: Object.entries(sizes).map(([presetId, perFrame]) => ({
      presetId,
      bytes: (preview.totals.frames ?? 0) * perFrame,
      apart: null,
      ext: null,
      note: null,
    })),
    folder,
    file,
  }
}
