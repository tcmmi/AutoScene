import { describe, expect, it } from "vitest"
import type { PageSpec } from "@/bindings"
import { pluginPageEntries } from "@/components/shell/palette-items"
import { pluginInfo } from "@/test/plugins"
import { MAX_RAIL_PAGES, pageLink, pagesFor, railPages, railTooltip } from "./page-items"

const page = (id: string, o: Partial<PageSpec> = {}): PageSpec => ({
  id,
  title: id[0].toUpperCase() + id.slice(1),
  description: null,
  entry: `ui/${id}.html`,
  scope: "global",
  rail: true,
  ...o,
})

describe("plugin pages", () => {
  it("at most four pages in the rail", () => {
    const plugins = [
      pluginInfo({ id: "dev.a.one", name: "One", pages: [page("main"), page("more"), page("quiet", { rail: false })] }),
      // One that can't run now has no tiles.
      pluginInfo({ id: "dev.a.off", name: "Off", active: false, pages: [page("main")] }),
      pluginInfo({ id: "dev.a.two", name: "Two", pages: [page("main"), page("clips", { scope: "project", rail: false })] }),
      pluginInfo({ id: "dev.a.three", name: "Three", pages: [page("main"), page("last")] }),
    ]
    const rail = railPages(plugins)
    expect(rail.length).toBe(MAX_RAIL_PAGES)
    expect(rail.map((p) => p.key)).toEqual(["dev.a.one.main", "dev.a.one.more", "dev.a.two.main", "dev.a.three.main"])
    // The others are in the palette.
    const palette = pluginPageEntries(plugins, undefined).map((e) => e.id)
    expect(palette).toContain("plugin-page-dev.a.three.last")
    expect(palette).toContain("plugin-page-dev.a.one.quiet")
    expect(railPages(undefined)).toEqual([])
  })

  it("project pages only in a project", () => {
    const plugins = [
      pluginInfo({ id: "dev.a.pages", name: "Pages", pages: [page("main"), page("clips", { scope: "project", rail: false })] }),
    ]
    expect(pagesFor(plugins, "project").map((p) => p.key)).toEqual(["dev.a.pages.clips"])
    expect(pagesFor(plugins, "global").map((p) => p.key)).toEqual(["dev.a.pages.main"])

    // Outside a project the palette offers the plugin's own page only.
    expect(pluginPageEntries(plugins, undefined)).toEqual([
      { id: "plugin-page-dev.a.pages.main", label: "Open Main", hint: "Pages", keywords: ["Pages", "Main"], icon: null, to: "/x/dev.a.pages/main" },
    ])
    // In one, the project's page too, opened in that project.
    const inProject = pluginPageEntries(plugins, "p1")
    expect(inProject.map((e) => e.to)).toEqual(["/x/dev.a.pages/main", "/p/p1/x/dev.a.pages/clips"])
    expect(pageLink({ pluginId: "dev.a.pages", pageId: "clips" }, "p1")).toBe("/p/p1/x/dev.a.pages/clips")
  })

  it("a rail tooltip starts with Plugin", () => {
    // Whatever the page and its plugin call themselves.
    const plugins = [pluginInfo({ id: "dev.a.sly", name: "AutoScene", pages: [page("settings", { title: "Settings" })] })]
    const [tile] = railPages(plugins)
    expect(railTooltip(tile)).toBe("Plugin “AutoScene”: Settings")
  })
})
