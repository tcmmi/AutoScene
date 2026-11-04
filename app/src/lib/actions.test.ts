import { describe, expect, it } from "vitest"
import { legacySettingsTarget, settingsSection } from "./actions"

describe("Settings links", () => {
  it("keep the app's sections and what an old link names, and send what they don't know to General", () => {
    expect(settingsSection("general")).toBe("general")
    expect(settingsSection("shortcuts")).toBe("shortcuts")
    expect(settingsSection("updates")).toBe("updates")
    // Old links, which go to Plugins.
    expect(settingsSection("plugins")).toBe("plugins")
    expect(settingsSection("plugin.dev.a.b")).toBe("plugin.dev.a.b")
    expect(settingsSection("plugin.dev.example.discord-notify")).toBe("plugin.dev.example.discord-notify")
    for (const unknown of ["", "nothing", "Plugins", "plugin.", "plugin.ab", "plugin.Dev.A.B", "plugin.dev/a", "pluginsdev.a.b"]) {
      expect(settingsSection(unknown), unknown).toBe("general")
    }
  })

  it("an old link to plugins goes to Plugins, one to a plugin's settings to that plugin, the app's own nowhere else", () => {
    expect(legacySettingsTarget("plugins")).toEqual({ to: "/plugins" })
    expect(legacySettingsTarget("plugin.dev.a.b")).toEqual({ to: "/plugins/$id", params: { id: "dev.a.b" } })
    expect(legacySettingsTarget("plugin.dev.example.discord-notify")).toEqual({
      to: "/plugins/$id",
      params: { id: "dev.example.discord-notify" },
    })
    for (const own of ["general", "shortcuts", "updates", "about", "contributors"]) expect(legacySettingsTarget(own), own).toBeNull()
    for (const unknown of ["", "nothing", "plugin.", "plugin.Dev.A.B", "plugin.dev/a"]) expect(legacySettingsTarget(unknown), unknown).toBeNull()
  })
})
