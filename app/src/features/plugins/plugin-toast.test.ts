import { beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import { pluginToast } from "./plugin-toast"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

beforeEach(() => vi.clearAllMocks())

describe("a plugin's toast", () => {
  it("a plugin called AutoScene still reads as a plugin", () => {
    pluginToast("AutoScene", { text: "Update ready", description: "Restart now." })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “AutoScene”: Update ready", { description: "Restart now." })
    pluginToast("AutoScene", { text: "Update failed", tone: "error" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “AutoScene”: Update failed", { description: undefined })
  })

  it("names no plugin it doesn't know", () => {
    pluginToast(null, { text: "Post to Discord failed", tone: "error" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("A plugin: Post to Discord failed", { description: undefined })
    expect(toast).not.toHaveBeenCalled()
  })
})
