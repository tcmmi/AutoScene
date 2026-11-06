import { invoke } from "@tauri-apps/api/core"
import { clearMocks } from "@tauri-apps/api/mocks"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import type { Cast, CastEdit, FaceTrackItem, ProjectSummary } from "@/bindings"
import { installMockBackend } from "./backend"

describe("the mock's characters", () => {
  beforeAll(() => installMockBackend())
  afterAll(() => clearMocks())

  it("carry the number an unnamed one is called by, named or not", async () => {
    const [project] = await invoke<ProjectSummary[]>("projects_list")
    const { characters } = await invoke<Cast>("cast_get", { projectId: project.id })
    expect(characters.length).toBeGreaterThan(0)
    const numbers = characters.map((c) => c.number)
    expect(new Set(numbers).size).toBe(numbers.length)
    for (const c of characters) {
      expect(Number.isInteger(c.number) && c.number > 0).toBe(true)
    }
    // The cast has both kinds, so both are checked.
    expect(characters.some((c) => c.name === null)).toBe(true)
    expect(characters.some((c) => c.name !== null)).toBe(true)
  })

  it("keep a number through a rename and never hand it out again", async () => {
    const [project] = await invoke<ProjectSummary[]>("projects_list")
    const projectId = project.id
    const edit = (e: CastEdit) => invoke<string | null>("cast_edit", { projectId, edit: e })
    const cast = async () => (await invoke<Cast>("cast_get", { projectId })).characters
    const loose = () => invoke<FaceTrackItem[]>("cast_tracks", { projectId, filter: { kind: "unassigned" } })
    const top = Math.max(...(await cast()).map((c) => c.number))

    // A new character takes the next number; naming it and unnaming it keeps that number.
    const first = (await edit({ op: "new_character", tracks: [(await loose())[0].id], name: null })) as string
    const find = async (id: string) => (await cast()).find((c) => c.id === id)
    expect(await find(first)).toMatchObject({ number: top + 1, name: null })
    await edit({ op: "rename", characterId: first, name: "Hero" })
    expect(await find(first)).toMatchObject({ number: top + 1, name: "Hero" })
    await edit({ op: "rename", characterId: first, name: null })
    expect(await find(first)).toMatchObject({ number: top + 1, name: null })

    // Merging it away leaves the others' numbers alone, and the next new one does not reuse its number.
    const [into] = (await cast()).filter((c) => c.id !== first)
    await edit({ op: "merge", from: first, into: into.id })
    expect(await find(first)).toBeUndefined()
    expect((await find(into.id))?.number).toBe(into.number)
    const second = (await edit({ op: "new_character", tracks: [(await loose())[0].id], name: null })) as string
    expect((await find(second))?.number).toBe(top + 2)
  })
})
