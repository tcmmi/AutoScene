/**
 * Name parts: what a pack's file and chapter names are made of. The list is
 * the backend's (as_project::naming::NAME_PARTS, exported into bindings.ts),
 * so the buttons, the mock and the backend's checks can't drift apart.
 */
import { FILE_PARTS as BACKEND_FILE_PARTS, NAME_PARTS } from "@/bindings"

/** A name part, as a template spells it between braces: `tc` for `{tc}`. */
export type NamePart = (typeof NAME_PARTS)[number][0]

/** Every name part, in the backend's order: what a shot's or a chapter's name may use. */
export const ALL_PARTS: readonly NamePart[] = NAME_PARTS.map(([part]) => part)

/** What the name of one file holding the whole pack may use: the others name a shot. */
export const FILE_PARTS: readonly NamePart[] = BACKEND_FILE_PARTS

/** The label of the button that puts a part in: "Number" for `{index}`. */
export const PART_LABELS = Object.fromEntries(NAME_PARTS) as Record<NamePart, string>

/**
 * What each part gives, with an example of its form: the line under the part's
 * spelling in its button's tooltip. The real names are in the dialog's preview,
 * so these stay fixed. A part the backend adds fails the type check until it
 * has one.
 */
export const PART_HINTS: Record<NamePart, string> = {
  series: "The project's title",
  character: "The character's name",
  episode: "The episode, as S01E03, or its file's name",
  index: "The shot's place in the pack: 001, 002, …",
  tc: "Where the shot starts, as 00-12-03-05",
  frame: "The frame the shot starts on, as 17357",
}

/**
 * What a piece of a template is: plain `text`; a `part` the field may use; a
 * part that names a `shot`, in the one file's name; an `unknown` part; or an
 * `unclosed` one, a `{` with no `}` after it.
 */
export type SegmentKind = "text" | "part" | "shot" | "unknown" | "unclosed"

/** A piece of a template, where it is in the field (UTF-16, as the field counts) and what it says, braces and all. */
export type Segment = { kind: SegmentKind; start: number; end: number; text: string }

const isNamePart = (name: string): name is NamePart => (ALL_PARTS as readonly string[]).includes(name)

/**
 * A template in pieces, read as the backend reads it (as_project::naming::
 * render_name): a `{` up to the first `}` after it is one part, whatever is
 * between them. `allowed` are the parts this field may use.
 */
export function parseTemplate(template: string, allowed: readonly NamePart[] = ALL_PARTS): Segment[] {
  const segments: Segment[] = []
  const push = (kind: SegmentKind, start: number, end: number) => {
    if (end > start) segments.push({ kind, start, end, text: template.slice(start, end) })
  }
  let at = 0
  for (let open = template.indexOf("{"); open >= 0; open = template.indexOf("{", at)) {
    push("text", at, open)
    const close = template.indexOf("}", open + 1)
    if (close < 0) {
      push("unclosed", open, template.length)
      return segments
    }
    const name = template.slice(open + 1, close)
    push(isNamePart(name) ? (allowed.includes(name) ? "part" : "shot") : "unknown", open, close + 1)
    at = close + 1
  }
  push("text", at, template.length)
  return segments
}

/**
 * The piece of a template the backend names when it refuses it, or null when
 * it has nothing to say about the parts. A chapter's or a shot's name
 * (render_name) stops at the first unknown or unclosed part. The one file's
 * name (render_file_name) looks twice: first for an unclosed part or one that
 * names a shot, then for an unknown one. `segments` are the template's, read
 * with the parts its field may use. An empty name has no such piece, but is
 * refused too.
 */
export function reportedPart(segments: readonly Segment[], forFile: boolean): Segment | null {
  const first = (...kinds: SegmentKind[]) => segments.find((s) => kinds.includes(s.kind)) ?? null
  return forFile ? (first("unclosed", "shot") ?? first("unknown")) : first("unknown", "unclosed")
}

/**
 * What a name-part button puts in, and where: `text` replaces `start`..`end`
 * of `value`, which was the selection. A caret inside a part moves past it,
 * so a part is never split (inside an unclosed one it stays: that part is
 * being typed). Two parts that would touch get a space between them, as part
 * of the same text, so one undo takes both; plain text may touch a part
 * (`E{index}`).
 */
export function insertion(
  value: string,
  start: number,
  end: number,
  part: NamePart,
): { start: number; end: number; text: string } {
  if (start === end) {
    const closed = parseTemplate(value).filter((s) => s.kind !== "text" && s.kind !== "unclosed")
    const inside = closed.find((s) => s.start < start && start < s.end)
    if (inside) start = end = inside.end
  }
  const before = value[start - 1] === "}" ? " " : ""
  const after = value[end] === "{" ? " " : ""
  return { start, end, text: `${before}{${part}}${after}` }
}
