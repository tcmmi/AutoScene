import type { CharacterShot, ShotGroup } from "@/bindings"
import type { Fps } from "@/lib/timecode"
import { cellAt } from "./shots"

/** A card of a character's Shots view: one shot, or a group of them stacked. */
export type StackCard = {
  key: string
  mediaId: string
  /** The group's first frame, and where it ends. */
  start: number
  end: number
  /** In time order; two or more make a stack. */
  shots: CharacterShot[]
  /** The shot the card shows at rest. */
  lead: CharacterShot
}

/** A shot's place in its episode: what stays when the list around it changes. */
export const shotKey = (s: CharacterShot) => `${s.mediaId}:${s.start}`

/** Their biggest face first, shots without their face last, the longest on a tie. */
function leadOf(shots: CharacterShot[]): CharacterShot {
  const size = (s: CharacterShot) => s.faceHeight ?? 0
  const length = (s: CharacterShot) => s.end - s.start
  return shots.reduce((best, s) => {
    if (Boolean(s.faceless) !== Boolean(best.faceless)) return s.faceless ? best : s
    if (size(s) !== size(best)) return size(s) > size(best) ? s : best
    return length(s) > length(best) ? s : best
  })
}

/** The Shots view's cards, in episode order. */
export function stackCards(groups: ShotGroup[]): StackCard[] {
  return groups
    .filter((g) => g.shots.length > 0)
    .map(({ shots }) => ({
      key: shotKey(shots[0]),
      mediaId: shots[0].mediaId,
      start: shots[0].start,
      end: shots[shots.length - 1].end,
      shots,
      lead: leadOf(shots),
    }))
}

/** How long the shots run together, gaps left out: the measure of the line above the grid. */
export function groupSeconds(shots: CharacterShot[], fps: Fps | null | undefined): number {
  if (!fps) return 0
  return shots.reduce((sum, s) => sum + ((s.end - s.start) * fps.den) / fps.num, 0)
}

/**
 * The shot under a pointer `x` pixels into a stack `width` wide, and the
 * cell of its sprite. Each shot gets the same share of the width, whatever
 * its length, so a one-frame shot among long ones can still be reached.
 */
export function groupCellAt(x: number, width: number, shots: CharacterShot[]): { index: number; shot: CharacterShot; cell: number } {
  const n = shots.length
  const slice = width / n
  const index = width > 0 ? Math.min(n - 1, Math.max(0, Math.floor(x / slice))) : 0
  const shot = shots[index]
  return { index, shot, cell: cellAt(x - index * slice, slice, shot.sprite ? Math.max(1, shot.cells) : 0) }
}

/**
 * Where an open stack goes when the list changes (an edit, a new analysis):
 * it follows the first of its shots still listed. That shot alone on a card
 * closes it, with the focus on that card.
 */
export function reopen(openKeys: string[], cards: StackCard[]): { open: StackCard | null; focus: StackCard | null } {
  const cardOf = new Map(cards.flatMap((c) => c.shots.map((s) => [shotKey(s), c] as const)))
  const key = openKeys.find((k) => cardOf.has(k))
  const card = key === undefined ? undefined : cardOf.get(key)
  if (!card) return { open: null, focus: null }
  return card.shots.length > 1 ? { open: card, focus: null } : { open: null, focus: card }
}
