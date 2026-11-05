import { beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import { inLanguage } from "@/test/i18n"
import { pluginToast } from "./plugin-toast"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

beforeEach(() => vi.clearAllMocks())

describe("a plugin's toast", () => {
  it("a plugin called AutoScene still reads as a plugin", () => {
    pluginToast("AutoScene", "jobFinished", { action: "Update ready" }, { description: "Restart now." })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “AutoScene”: Update ready", { description: "Restart now." })
    pluginToast("AutoScene", "jobFailed", { action: "Update" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “AutoScene”: Update failed", { description: undefined })
  })

  it("can offer an action", () => {
    const action = { label: "Show in folder", onClick: () => {} }
    pluginToast("FCPXML", "fileSaved", { file: "Walter.fcpxml" }, { action })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: saved Walter.fcpxml", { description: undefined, action })
  })

  it("names no plugin it doesn't know", () => {
    pluginToast(null, "jobFailed", { action: "Post to Discord" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("A plugin: Post to Discord failed", { description: undefined })
    expect(toast).not.toHaveBeenCalled()
  })

  it("says each kind in the words it has always used", () => {
    pluginToast("Discord", "alreadyWaiting", { action: "Post" }, { description: "It's in Activity." })
    expect(toast).toHaveBeenLastCalledWith("Plugin “Discord”: Post is already waiting or running", { description: "It's in Activity." })
    pluginToast("Discord", "cantStart", { action: "Post" }, { description: "No uv." })
    expect(toast.error).toHaveBeenLastCalledWith("Plugin “Discord”: Post couldn't start", { description: "No uv." })
    pluginToast("Discord", "updatedTo", { version: "1.2.0" })
    expect(toast).toHaveBeenLastCalledWith("Plugin “Discord”: updated to 1.2.0", { description: undefined })
    pluginToast("Discord", "updateFailed", {}, { description: "Offline." })
    expect(toast.error).toHaveBeenLastCalledWith("Plugin “Discord”: couldn't be updated", { description: "Offline." })
    pluginToast(null, "updatedTo", { version: "1.2.0" })
    expect(toast).toHaveBeenLastCalledWith("A plugin: updated to 1.2.0", { description: undefined })
  })

  it("keeps its fixed first word in another language, whatever the plugin calls itself", async () => {
    await inLanguage("ro", {
      plugins: {
        toast: {
          failed: { named: "Pluginul „{{name}}”: {{action}} a eșuat", anonymous: "Un plugin: {{action}} a eșuat" },
          finished: { named: "Pluginul „{{name}}”: {{action}}", anonymous: "Un plugin: {{action}}" },
        },
      },
    })
    pluginToast("AutoScene", "jobFinished", { action: "Actualizare gata" }, { description: "Repornește acum." })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Pluginul „AutoScene”: Actualizare gata", { description: "Repornește acum." })
    pluginToast("AutoScene", "jobFailed", { action: "Actualizare" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Pluginul „AutoScene”: Actualizare a eșuat", { description: undefined })
    // A kind the language has no words for yet reads in English, still after the fixed word.
    pluginToast("AutoScene", "fileSaved", { file: "a.fcpxml" })
    expect(toast).toHaveBeenLastCalledWith("Plugin “AutoScene”: saved a.fcpxml", { description: undefined })
    // A plugin AutoScene does not know by name gets the anonymous message.
    pluginToast(null, "jobFailed", { action: "Postare" })
    expect(toast.error).toHaveBeenLastCalledWith("Un plugin: Postare a eșuat", { description: undefined })
  })
})
