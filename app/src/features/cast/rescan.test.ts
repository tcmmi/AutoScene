import { describe, expect, it } from "vitest"
import type { FaceTrackItem, RescanFace, RescanVerdict } from "@/bindings"
import { clearMatches, rescanParts } from "./rescan"

const face = (id: number, verdict: RescanVerdict, target: string | null, score = 0.5, other: string | null = null): RescanFace => ({
  face: { id } as FaceTrackItem,
  verdict,
  target,
  other,
  score,
  score2: 0,
  read: true,
})

describe("what Rescan found", () => {
  it("sorts faces into who they may be, clear ones first and the biggest lane on top", () => {
    const parts = rescanParts([
      face(1, "maybe", "a", 0.9),
      face(2, "newly", "a", 0.5),
      face(3, "elsewhere", "a", 0.6),
      face(4, "newly", "b"),
      face(5, "between", "a", 0.5, "b"),
      face(6, "doubt", "b"),
      face(7, "no_face", null),
    ])
    expect(parts.lanes.map((l) => l.target)).toEqual(["a", "b"])
    expect(parts.lanes[0].faces.map((f) => f.face.id)).toEqual([3, 2, 1])
    expect(parts.between.map((f) => f.face.id)).toEqual([5])
    expect(parts.doubts).toHaveLength(1)
    expect(parts.noFace.map((f) => f.face.id)).toEqual([7])
  })

  it("offers a guess with someone close behind as a choice", () => {
    const parts = rescanParts([face(1, "maybe", "a", 0.5, "b"), face(2, "maybe", "a")])
    expect(parts.between.map((f) => f.face.id)).toEqual([1])
    expect(parts.lanes[0].faces.map((f) => f.face.id)).toEqual([2])
  })

  it("counts only clear answers as clear matches", () => {
    const m = clearMatches([face(1, "newly", "a"), face(2, "maybe", "a"), face(3, "between", "a"), face(4, "elsewhere", "b")])
    expect([...m]).toEqual([
      ["a", [1]],
      ["b", [4]],
    ])
  })
})
