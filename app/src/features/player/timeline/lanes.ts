import type { Lane } from "@/lib/presence"

/** Lanes shown under the shots; everyone else is listed in the details panel. */
export const MAX_LANES = 5

/**
 * The lanes the timeline shows: the `max` characters most on screen. A
 * highlighted character further down the list takes the last slot instead, so
 * highlighting someone from the panel always shows where they are, and the
 * timeline keeps its height.
 */
export function visibleLanes(lanes: Lane[], focus: string | null, max: number = MAX_LANES): Lane[] {
  const top = lanes.slice(0, max)
  if (focus === null || max < 1 || top.some((l) => l.character.id === focus)) return top
  const focused = lanes.find((l) => l.character.id === focus)
  return focused ? [...top.slice(0, max - 1), focused] : top
}
