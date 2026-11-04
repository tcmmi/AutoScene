import { describe, expect, it } from "vitest"
import { pluginInfo } from "@/test/plugins"
import { pluginMatches, pluginPicture, pluginsLit } from "./plugin-items"

describe("finding a plugin", () => {
  const p = pluginInfo({ id: "dev.example.discord-notify", name: "Discord Notify", author: "Example Dev", description: "Posts each export's clip list." })

  it("looks at its name, author, description and id, whatever the case", () => {
    for (const q of ["discord", "NOTIFY", "example dev", "clip list", "dev.example", "  posts  "]) expect(pluginMatches(p, q)).toBe(true)
    expect(pluginMatches(p, "slack")).toBe(false)
  })

  it("finds each word wherever it is", () => {
    expect(pluginMatches(p, "example posts")).toBe(true)
    expect(pluginMatches(p, "example slack")).toBe(false)
  })

  it("finds every plugin with nothing to find", () => {
    for (const q of ["", "   "]) expect(pluginMatches(p, q)).toBe(true)
  })
})

describe("a plugin's picture", () => {
  it("is only ever a picture's base64 data URL", () => {
    for (const kind of ["png", "jpeg", "webp", "svg+xml"]) {
      const url = `data:image/${kind};base64,AAAA`
      expect(pluginPicture(url)).toBe(url)
    }
    for (const src of [
      null,
      undefined,
      "",
      "https://example.com/banner.png",
      "data:image/gif;base64,R0lGOD==",
      "data:text/html;base64,PHNjcmlwdD4=",
      "data:image/png,not-base64",
      "data:image/jpeg;base64,/9j/\"><script>",
    ])
      expect(pluginPicture(src)).toBeNull()
  })
})

describe("the rail's Plugins tile", () => {
  it("is lit on Plugins and on a plugin's settings, nowhere else", () => {
    expect(pluginsLit("/plugins")).toBe(true)
    expect(pluginsLit("/plugins/dev.a.one")).toBe(true)
    expect(pluginsLit("/")).toBe(false)
    expect(pluginsLit("/settings/general")).toBe(false)
    expect(pluginsLit("/pluginsx")).toBe(false)
  })
})
