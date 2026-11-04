import { describe, expect, it } from "vitest"
import { pluginInfo } from "@/test/plugins"
import { downloadSize, firstTime, joinWords, nextTime, pluginMatches, pluginPicture, pluginsLit, resourceLine, trustLabel, withdrawnLine } from "./plugin-items"

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

describe("a withdrawn plugin", () => {
  it("says Withdrawn before any trust, a loaded folder's too", () => {
    expect(trustLabel({ trust: "reviewed", source: "package", withdrawn: null })).toEqual({ label: "Reviewed by AutoScene", tone: "neutral" })
    for (const source of ["package", "folder"] as const)
      expect(trustLabel({ trust: "reviewed", source, withdrawn: "It sent export paths." })).toEqual({ label: "Withdrawn", tone: "danger" })
  })

  it("its banner gives the catalog's reason as a sentence, then what that means", () => {
    expect(withdrawnLine("It sent export paths to a server.")).toBe("It sent export paths to a server. It is turned off.")
    expect(withdrawnLine("  It sent paths  ")).toBe("It sent paths. It is turned off.")
    expect(withdrawnLine("Is it safe?")).toBe("Is it safe? It is turned off.")
  })
})

describe("what a plugin downloads", () => {
  const files = (resourceBytes: number | null, ...resourceHosts: string[]) => ({ resourceBytes, resourceHosts })

  it("resourceLine: how much its first run downloads and from where", () => {
    expect(resourceLine(files(327_155_712, "huggingface.co"), "process")).toBe(
      "It downloads 312 MB from huggingface.co at its first run. AutoScene keeps the files in its cache and checks each one by its hash.",
    )
    // Hosts as a person lists them; a service gets its files at its first start.
    expect(resourceLine(files(3 * 1024 ** 3, "a.example", "b.example", "c.example"), "service")).toBe(
      "It downloads 3 GB from a.example, b.example and c.example at its first start. AutoScene keeps the files in its cache and checks each one by its hash.",
    )
    // Files with no host to name still have a size to say.
    expect(resourceLine(files(1024 ** 2), "process")).toMatch(/^It downloads 1 MB at its first run\./)
  })

  it("resourceLine: a size is never rounded down by half a unit from a gigabyte up", () => {
    expect(resourceLine(files(1.5 * 1024 ** 3, "a.example"), "process")).toMatch(/^It downloads 1\.5 GB from a\.example/)
    expect(resourceLine(files(1.49 * 1024 ** 3), "process")).toMatch(/^It downloads 1\.5 GB /)
    expect(downloadSize(1023 * 1024 ** 2)).toBe("1023 MB")
    expect(downloadSize(8 * 1024 ** 3)).toBe("8 GB")
  })

  it("resourceLine: an update's plugin has run, so its files come at its next run or start", () => {
    expect(resourceLine(files(5 * 1024 ** 2, "a.example"), "process", true)).toMatch(/ at its next run\. /)
    expect(resourceLine(files(5 * 1024 ** 2, "a.example"), "service", true)).toMatch(/ at its next start\. /)
    expect(nextTime("service")).toBe("next start")
    expect(nextTime("process")).toBe("next run")
  })

  it("resourceLine: files AutoScene keeps already come from somewhere, and a plugin without says nothing", () => {
    expect(resourceLine(files(0, "huggingface.co"), "process")).toBe(
      "The large files it names come from huggingface.co. AutoScene already has them, and checks each one by its hash.",
    )
    expect(resourceLine(files(0), "process")).toBeNull()
    expect(resourceLine(files(null), "process")).toBeNull()
  })

  it("joinWords and firstTime", () => {
    expect(joinWords([])).toBe("")
    expect(joinWords(["a"])).toBe("a")
    expect(joinWords(["a", "b"])).toBe("a and b")
    expect(joinWords(["a", "b", "c"])).toBe("a, b and c")
    expect(firstTime("service")).toBe("first start")
    for (const kind of ["process", "none", "wasm"]) expect(firstTime(kind)).toBe("first run")
  })
})
