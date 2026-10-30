import { describe, expect, it } from "vitest"
import type { Character } from "@/bindings"
import { formatScreenTime, plural, splitTiers, unplacedNote } from "./cast"

describe("cast wording", () => {
  it("splits main and supporting characters", () => {
    const chars: Character[] = [
      { id: "1", label: "Hero", name: "Patrick Jane", screenSec: 840, shots: 415, tracks: 1, media: 1, waiting: 0, unsure: 0, hidden: false, cover: null, coverSmall: null },
      { id: "2", label: "Character 2", name: null, screenSec: 120, shots: 50, tracks: 1, media: 1, waiting: 0, unsure: 0, hidden: false, cover: null, coverSmall: null },
      { id: "3", label: "Character 3", name: null, screenSec: 15, shots: 5, tracks: 1, media: 1, waiting: 0, unsure: 0, hidden: false, cover: null, coverSmall: null },
    ]
    const { main, supporting } = splitTiers(chars)
    // When <= 8 total characters, all remain together
    expect(main.length).toBe(3)
    expect(supporting.length).toBe(0)

    // With many characters, separates >= 60s / named from background
    const manyChars = Array.from({ length: 15 }, (_, i) => ({
      id: `${i}`,
      label: `Char ${i}`,
      name: i === 0 ? "Named" : null,
      screenSec: i === 1 ? 90 : 20,
      shots: 10,
      tracks: 1,
      media: 1,
      waiting: 0,
      unsure: 0,
      hidden: false,
      cover: null,
      coverSmall: null,
    }))
    const res = splitTiers(manyChars)
    expect(res.main.length).toBe(2)
    expect(res.supporting.length).toBe(13)
  })
  it("says screen time the way people do", () => {
    expect([0, 42.4, 61, 59 * 60, 3725].map(formatScreenTime)).toEqual(["0 s", "42 s", "1 min", "59 min", "1 h 02 min"])
  })

  it("counts", () => {
    expect([plural(1, "shot"), plural(1414, "face"), plural(2, "episode")]).toEqual(["1 shot", "1,414 faces", "2 episodes"])
  })

  it("says how many decisions wait for their faces, and that they're tried again", () => {
    expect([unplacedNote(0), unplacedNote(-1), unplacedNote(Number.NaN)]).toEqual([null, null, null])
    expect(unplacedNote(1)).toBe(
      "1 of your decisions waits for a face the latest analysis didn't find again. It's tried again at the next analysis.",
    )
    expect(unplacedNote(1414)).toBe(
      "1,414 of your decisions wait for faces the latest analysis didn't find again. They're tried again at the next analysis.",
    )
  })
})
