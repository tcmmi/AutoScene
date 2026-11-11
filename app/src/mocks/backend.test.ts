import { invoke } from "@tauri-apps/api/core"
import { clearMocks } from "@tauri-apps/api/mocks"
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest"
import { commands } from "@/bindings"
import { installMockBackend } from "./backend"

beforeAll(() => installMockBackend())
afterAll(() => clearMocks())

describe("the mock's errors", () => {
  it("carry a msg as the real backend's do, with no key, as the words are the mock's", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {})
    const message = "Mock backend has no handler for not_a_command"
    await expect(invoke("not_a_command")).rejects.toEqual({ code: "not_found", message, msg: { key: "", text: message } })
  })
})

describe("the mock's projects", () => {
  // Each was once stamped with the moment the mock made it. On a busy machine the clock moved between
  // two of them, the film came first, and a test that takes the first project looked for "Episodes".
  it("are listed by the age each is given, the last changed first, whenever the mock made them", async () => {
    const list = await commands.projectsList()
    expect(list.map((p) => [p.title, p.kind])).toEqual([
      ["Sousou no Frieren", "series"],
      ["Dark", "series"],
      ["Blade Runner 2049", "movie"],
    ])
    const hour = 3_600_000
    expect(list[0]!.updatedAt - list[1]!.updatedAt).toBeGreaterThan(hour)
    expect(list[1]!.updatedAt - list[2]!.updatedAt).toBeGreaterThan(hour)
  })
})
