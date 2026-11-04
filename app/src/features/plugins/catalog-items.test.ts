import { describe, expect, it } from "vitest"
import { catalogPlugin, catalogReview, catalogView, pluginInfo } from "@/test/plugins"
import { asksNothingNew, catalogAsks, catalogLine, catalogMatches, oneClick, stuckSince, updatesOf } from "./catalog-items"

const mine = pluginInfo({ id: "dev.a.mine", name: "Mine" })

describe("catalog items", () => {
  it("oneClick: only a reviewed update that asks for nothing new", () => {
    expect(oneClick(catalogReview(mine, {}))).toBe(true)
    // Not an update: the person says yes to a new plugin.
    expect(oneClick(catalogReview(mine))).toBe(false)
    expect(oneClick(catalogReview(mine, { addedPermissions: ["network"] }))).toBe(false)
    expect(oneClick(catalogReview(mine, { addedHosts: ["discord.com"] }))).toBe(false)
    expect(oneClick({ ...catalogReview(mine, {}), plugin: { ...mine, trust: "pinned" } })).toBe(false)
    expect(oneClick({ ...catalogReview(mine, {}), needsUnreviewed: true })).toBe(false)
    // What the review marks New, and what it says the person should know first.
    expect(oneClick(catalogReview(mine, { addedPages: ["poster"] }))).toBe(false)
    expect(oneClick(catalogReview(mine, { startsProgram: true }))).toBe(false)
    expect(oneClick(catalogReview(mine, { becomesService: true }))).toBe(false)
    expect(oneClick(catalogReview(mine, { sourceBefore: "folder" }))).toBe(false)
    expect(oneClick(catalogReview(mine, { trustBefore: "unreviewed" }))).toBe(false)
    expect(oneClick(catalogReview(mine, { trustBefore: "pinned" }))).toBe(false)
    expect(oneClick({ ...catalogReview(mine, {}), uvMissing: true })).toBe(false)
  })

  it("catalogMatches: each word in its name, summary, author or id", () => {
    const p = catalogPlugin({ id: "dev.autoscene.poster-maker", name: "Poster Maker", summary: "Makes a poster of each character.", author: "AutoScene" })
    expect(catalogMatches(p, "")).toBe(true)
    expect(catalogMatches(p, "POSTER")).toBe(true)
    expect(catalogMatches(p, "each character")).toBe(true)
    expect(catalogMatches(p, "autoscene maker")).toBe(true)
    expect(catalogMatches(p, "poster-maker")).toBe(true)
    expect(catalogMatches(p, "discord")).toBe(false)
  })

  it("updatesOf: newer versions of installed plugins that run here", () => {
    const list = [
      catalogPlugin({ id: "dev.a.new", name: "New" }),
      catalogPlugin({ id: "dev.a.same", name: "Same", installed: "1.0.0" }),
      catalogPlugin({ id: "dev.a.newer", name: "Newer", installed: "0.9.0", update: true }),
      catalogPlugin({ id: "dev.a.elsewhere", name: "Elsewhere", installed: "0.9.0", update: true, runsHere: false }),
      catalogPlugin({ id: "dev.a.later", name: "Later", installed: "0.9.0", update: true, needsNewerApp: true }),
    ]
    expect(updatesOf(list).map((p) => p.id)).toEqual(["dev.a.newer"])
    expect(updatesOf(undefined)).toEqual([])
    expect(asksNothingNew(list[2])).toBe(true)
    expect(asksNothingNew({ ...list[2], addsHosts: ["discord.com"] })).toBe(false)
  })

  it("catalogAsks: what it may do in one line", () => {
    expect(catalogAsks(catalogPlugin({ id: "dev.a.n", name: "N", permissions: ["exports:read", "network"], hosts: ["discord.com", "x.com"] }))).toBe(
      "Reads the exported clips · Talks to discord.com, x.com",
    )
    expect(catalogAsks(catalogPlugin({ id: "dev.a.n", name: "N", permissions: ["network"] }))).toBe("Talks to the internet")
    expect(catalogAsks(catalogPlugin({ id: "dev.a.n", name: "N", gpu: true }))).toBe("Uses the graphics card")
    expect(catalogAsks(catalogPlugin({ id: "dev.a.n", name: "N" }))).toBe("Names no permissions")
  })

  it("catalogLine: which catalog, and when it was checked", () => {
    const { status } = catalogView([])
    const at = (y: number, m: number, d: number, h = 12) => new Date(y, m, d, h).getTime()
    const s = { ...status, published: at(2026, 10, 3), checkedAt: at(2026, 10, 4, 8) }
    expect(catalogLine(s, at(2026, 10, 4, 20))).toBe("Catalog of 3 November 2026. Checked today.")
    expect(catalogLine(s, at(2026, 10, 5, 1))).toBe("Catalog of 3 November 2026. Checked yesterday.")
    expect(catalogLine(s, at(2026, 10, 9))).toBe("Catalog of 3 November 2026. Checked on 4 November 2026.")
    expect(catalogLine({ ...s, published: null, checkedAt: null })).toBe("No catalog yet. Not checked yet.")
    expect(catalogLine({ ...s, error: "The catalog ran out on 2026-10-25." }, at(2026, 10, 4, 20))).toBe(
      "Catalog of 3 November 2026. Last check failed today.",
    )
    // Browse says why on its own.
    expect(catalogLine({ ...s, ready: false })).toBeNull()
    expect(catalogLine({ ...s, ready: false, needsNewerApp: true })).toBeNull()
    expect(stuckSince({ ...s, fetchedAt: at(2026, 8, 24) })).toBe("24 September 2026")
    expect(stuckSince({ ...s, fetchedAt: null, published: null })).toBeNull()
  })
})
