import { readFileSync } from "node:fs"
import { join } from "node:path"
import { describe, expect, it } from "vitest"
import { en } from "@/locales"
import { i18n } from "@/lib/i18n"
import { PURPOSE } from "./model-purpose"
import contributors from "./contributors.json"

// The catalog says in its own words what two files of the repository say in English: a model's purpose
// (models/manifest.toml) and a contributor's role (contributors.json). They must keep saying the same.
const manifest = readFileSync(join(process.cwd(), "..", "models", "manifest.toml"), "utf8")

describe("what the About page's files say", () => {
  it("is in the catalog, in the same words: every model's purpose", () => {
    const entries = manifest.split("[[model]]").slice(1).map((block) => ({
      id: /^id = "([^"]+)"/m.exec(block)?.[1],
      purpose: /^purpose = "([^"]+)"/m.exec(block)?.[1],
    }))
    expect(entries.length).toBeGreaterThan(5)
    // Every model has a key (model-purpose.ts), and the English at that key is the manifest's own sentence.
    expect(entries.map((e) => e.id).sort()).toEqual(Object.keys(PURPOSE).sort())
    for (const e of entries) expect(i18n.t(`settings:${PURPOSE[e.id as keyof typeof PURPOSE]}`), e.id).toBe(e.purpose)
    // And no key is left without a model.
    expect(Object.keys(en.settings.about.models.purpose).sort()).toEqual(Object.values(PURPOSE).map((k) => k.split(".").pop()).sort())
  })

  it("is in the catalog, in the same words: every contributor's role", () => {
    const said = en.settings.contributors.role as Record<string, string>
    expect(Object.fromEntries(contributors.map((c) => [c.handle, c.role]))).toEqual(said)
  })
})
