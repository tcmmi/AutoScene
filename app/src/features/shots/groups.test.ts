import { describe, expect, it } from "vitest"
import type { CharacterShot } from "@/bindings"
import { groupCellAt, groupSeconds, reopen, shotKey, stackCards } from "./groups"

const shot = (start: number, end: number, more: Partial<CharacterShot> = {}): CharacterShot => ({
  mediaId: "e1",
  start,
  end,
  sprite: `s${start}.jpg`,
  cells: 8,
  trackId: start,
  face: `f${start}.jpg`,
  faceHeight: 0.2,
  ...more,
})

describe("stackCards", () => {
  it("makes a card of each group, in order, keyed by its first shot", () => {
    const a = [shot(0, 24), shot(30, 60)]
    const b = [shot(500, 600, { mediaId: "e2" })]
    const cards = stackCards([{ shots: a }, { shots: b }])
    expect(cards.map((c) => [c.key, c.mediaId, c.start, c.end, c.shots.length])).toEqual([
      ["e1:0", "e1", 0, 60, 2],
      ["e2:500", "e2", 500, 600, 1],
    ])
  })

  it("leads with their biggest face", () => {
    const big = shot(30, 40, { faceHeight: 0.6 })
    expect(stackCards([{ shots: [shot(0, 24), big, shot(50, 90)] }])[0].lead).toBe(big)
  })

  it("leads with a shot without their face only when there is no other", () => {
    const small = shot(30, 40, { faceHeight: 0.1 })
    const body = shot(0, 24, { faceless: true, faceHeight: 0.9 })
    expect(stackCards([{ shots: [body, small] }])[0].lead).toBe(small)
    const other = shot(50, 90, { faceless: true, faceHeight: 0.9 })
    expect(stackCards([{ shots: [body, other] }])[0].lead).toBe(other)
  })

  it("leads with the longest shot on a tie, and counts an unknown size as none", () => {
    const long = shot(30, 90)
    expect(stackCards([{ shots: [shot(0, 24), long] }])[0].lead).toBe(long)
    const sized = shot(0, 10, { faceHeight: 0.01 })
    expect(stackCards([{ shots: [sized, shot(30, 90, { faceHeight: null })] }])[0].lead).toBe(sized)
  })
})

describe("groupSeconds", () => {
  it("adds up the shots' lengths, leaving the gaps out", () => {
    expect(groupSeconds([shot(0, 24), shot(1000, 1048)], { num: 24, den: 1 })).toBe(3)
    expect(groupSeconds([shot(0, 30000)], { num: 30000, den: 1001 })).toBeCloseTo(1001)
  })

  it("is nothing without a frame rate", () => {
    expect(groupSeconds([shot(0, 24)], null)).toBe(0)
  })
})

describe("groupCellAt", () => {
  const shots = [shot(0, 200), shot(200, 201, { cells: 1 }), shot(201, 401)]

  it("gives each shot the same share of the width, however long it is", () => {
    expect(groupCellAt(0, 300, shots)).toMatchObject({ index: 0, shot: shots[0], cell: 0 })
    expect(groupCellAt(99.9, 300, shots)).toMatchObject({ index: 0, cell: 7 })
    expect(groupCellAt(100, 300, shots)).toMatchObject({ index: 1, shot: shots[1], cell: 0 })
    expect(groupCellAt(199.9, 300, shots)).toMatchObject({ index: 1, cell: 0 })
    expect(groupCellAt(200, 300, shots)).toMatchObject({ index: 2, cell: 0 })
    expect(groupCellAt(250, 300, shots)).toMatchObject({ index: 2, cell: 4 })
  })

  it("stays on the first and last shot past the edges", () => {
    expect(groupCellAt(300, 300, shots)).toMatchObject({ index: 2, cell: 7 })
    expect(groupCellAt(-5, 300, shots)).toMatchObject({ index: 0, cell: 0 })
    expect(groupCellAt(10, 0, shots)).toMatchObject({ index: 0, cell: 0 })
  })

  it("has no cell in a shot without a sprite", () => {
    const old = [shot(0, 24), shot(24, 48, { sprite: null, cells: 0 })]
    expect(groupCellAt(120, 300, old)).toMatchObject({ index: 0, cell: 6 })
    expect(groupCellAt(250, 300, old)).toMatchObject({ index: 1, cell: 0 })
  })
})

describe("reopen", () => {
  const [a, b, c] = [shot(0, 24), shot(30, 50), shot(60, 80)]
  const keys = [a, b, c].map(shotKey)

  it("keeps a stack open while its shots are listed", () => {
    const cards = stackCards([{ shots: [a, b, c] }])
    expect(reopen(keys, cards)).toEqual({ open: cards[0], focus: null })
  })

  it("stays on the rest of the group when its first shot goes", () => {
    const cards = stackCards([{ shots: [shot(900, 950)] }, { shots: [b, c] }])
    expect(reopen(keys, cards)).toEqual({ open: cards[1], focus: null })
  })

  it("closes on the card of the one shot left", () => {
    const cards = stackCards([{ shots: [c] }])
    expect(reopen(keys, cards)).toEqual({ open: null, focus: cards[0] })
  })

  it("closes without a place to go when none of its shots are left", () => {
    expect(reopen(keys, stackCards([{ shots: [shot(900, 950)] }]))).toEqual({ open: null, focus: null })
    expect(reopen([], stackCards([{ shots: [a, b] }]))).toEqual({ open: null, focus: null })
  })
})
