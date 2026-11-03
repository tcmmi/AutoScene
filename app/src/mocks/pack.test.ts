import { invoke } from "@tauri-apps/api/core"
import { clearMocks } from "@tauri-apps/api/mocks"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import type { Cast, PackNaming, PackPreview, ProjectSummary } from "@/bindings"
import { installMockBackend } from "./backend"
import { defaultPackState, fileProblem, templateProblem } from "./pack"

// The backend's sentences (as_project::naming), as its tests pin them.
const unclosed = "A “{” isn't closed with “}”."
const unknown = (part: string) => `“{${part}}” isn't a name part. Use {series}, {character}, {episode}, {index}, {tc} or {frame}.`
const shot = (part: string) => `“{${part}}” names a shot, not the file. The file's name can use {series} and {character}.`

describe("the mock's name checks", () => {
  it("stop at the first unknown or unclosed part of a shot's name", () => {
    expect(templateProblem("{series} {ep} {x")).toBe(unknown("ep"))
    expect(templateProblem("{series} {x {ep}")).toBe(unknown("x {ep"))
    expect(templateProblem("{series} {tc")).toBe(unclosed)
    expect(templateProblem(" ")).toBe("The name would be empty.")
    expect(templateProblem("{series} {character} {episode} {index} {tc} {frame}")).toBeNull()
  })

  it("look at the one file's name twice, as the backend does", () => {
    // First an unclosed part or one that names a shot, wherever an unknown one is.
    expect(fileProblem("{nme} {tc}")).toBe(shot("tc"))
    expect(fileProblem("{nme} {index} {tc}")).toBe(shot("index"))
    expect(fileProblem("{nme} {series")).toBe(unclosed)
    // Then an unknown one: a "{" runs to the first "}", so this one names no shot.
    expect(fileProblem("{ser {tc}")).toBe(unknown("ser {tc"))
    expect(fileProblem("{series} {nme}")).toBe(unknown("nme"))
    expect(fileProblem("")).toBe("The name would be empty.")
    expect(fileProblem("{series} {character} scenepack")).toBeNull()
  })
})

describe("the mock's pack commands", () => {
  beforeAll(() => installMockBackend())
  afterAll(() => clearMocks())

  /** A character of the first project with shots in its pack, and that pack. */
  async function aPack() {
    const [project] = await invoke<ProjectSummary[]>("projects_list")
    const projectId = project.id
    const cast = await invoke<Cast>("cast_get", { projectId })
    const { spec } = defaultPackState()
    for (const c of cast.characters) {
      const preview = await invoke<PackPreview>("pack_preview", { projectId, characterId: c.id, spec })
      if (preview.totals.clips > 0) return { projectId, characterId: c.id, spec, stamp: preview.stamp }
    }
    throw new Error("no character with shots in the mock")
  }

  const naming = (template: string, file: string): PackNaming => ({ template, file, merge: true, folder: false })
  const refusal = (call: Promise<unknown>) => call.then(() => null, (e: { message: string }) => e.message)

  it("plan the one file's name before the names' template, as pack_plan does", async () => {
    const { projectId, characterId, spec } = await aPack()
    const plan = (n: PackNaming) => refusal(invoke("pack_plan", { projectId, characterId, spec, naming: n }))
    expect(await plan(naming("{series} {ep}", "{series} {tc}"))).toBe(shot("tc"))
    expect(await plan(naming("{series} {ep}", "{series} {character}"))).toBe(unknown("ep"))
    // A file per shot: its name isn't checked.
    expect(await plan({ ...naming("{series} {tc}", "{series} {tc}"), merge: false })).toBeNull()
  })

  it("export the names' template first, then the pack as shown, then the one file's name, as pack_export does", async () => {
    const { projectId, characterId, spec, stamp } = await aPack()
    const send = (n: PackNaming, s = stamp) =>
      refusal(invoke("pack_export", { projectId, characterId, req: { spec, naming: n, presetId: "prores-hq", outDir: null, stamp: s } }))
    expect(await send(naming("{series} {ep}", "{series} {tc}"), "old")).toBe(unknown("ep"))
    expect(await send(naming("{series} {tc}", "{series} {tc}"), "old")).toBe("The pack changed meanwhile. Check it and export again.")
    expect(await send(naming("{series} {tc}", "{series} {tc}"))).toBe(shot("tc"))
  })
})
