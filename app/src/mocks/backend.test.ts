import { invoke } from "@tauri-apps/api/core"
import { clearMocks } from "@tauri-apps/api/mocks"
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest"
import { installMockBackend } from "./backend"

describe("the mock's errors", () => {
  beforeAll(() => installMockBackend())
  afterAll(() => clearMocks())

  it("carry a msg as the real backend's do, with no key, as the words are the mock's", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {})
    const message = "Mock backend has no handler for not_a_command"
    await expect(invoke("not_a_command")).rejects.toEqual({ code: "not_found", message, msg: { key: "", text: message } })
  })
})
