import { describe, expect, it } from "vitest"
import { type Bundle, messagesOf } from "./check"

// What the catalog test can't see in the Romanian: a word kept for one thing, a sentence that must read right.
// Each is a rule of docs/TRANSLATING.md that a draft once broke.
const files = import.meta.glob<Record<string, unknown>>(["./ro/*.json", "!./ro/meta.json"], { eager: true, import: "default" })
const ro = Object.fromEntries(Object.entries(files).map(([file, words]) => [file.replace(/^.*\/(.+)\.json$/, "$1"), words])) as Bundle
const messages = messagesOf(ro)

describe("the Romanian", () => {
  it("keeps Plan apropiat for the medium close-up: shots close together are something else", () => {
    const uses = [...messages].filter(([, forms]) => [...forms.values()].some((t) => /plan\w* apropiat/i.test(t))).map(([key]) => key)
    expect(uses).toEqual(["pack:size.mediumCloseUp.label"])
  })
})
