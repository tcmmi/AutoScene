import { describe, expect, it } from "vitest"
import { type Bundle, messagesOf } from "./check"

// What the catalog test can't see in the Romanian: a word kept for one thing, a sentence that must read right.
// Each is a rule of docs/TRANSLATING.md that a draft once broke.
const files = import.meta.glob<Record<string, unknown>>(["./ro/*.json", "!./ro/meta.json"], { eager: true, import: "default" })
const ro = Object.fromEntries(Object.entries(files).map(([file, words]) => [file.replace(/^.*\/(.+)\.json$/, "$1"), words])) as Bundle
const messages = messagesOf(ro)
const text = (key: string, form = "") => messages.get(key)?.get(form)

describe("the Romanian", () => {
  it("keeps Plan apropiat for the medium close-up: shots close together are something else", () => {
    const uses = [...messages].filter(([, forms]) => [...forms.values()].some((t) => /plan\w* apropiat/i.test(t))).map(([key]) => key)
    expect(uses).toEqual(["pack:size.mediumCloseUp.label"])
  })

  it("says what each word means here, not a word next to it", () => {
    const all = [...messages.values()].flatMap((forms) => [...forms.values()]).join("\n")
    // "pe loc" is at once; "de epocă" is vintage; "Mulțumiri" is thanks, on a legal notices file.
    for (const wrong of ["actualizează pe loc", "Certificatul de epocă", "Mulțumiri", "mulțumirile", "cele dintre.", "distribuția: {{series}}"]) {
      expect(all).not.toContain(wrong)
    }
  })

  it("sets a job's title apart from the verb that stops it: the title is a clause of its own", () => {
    // "Anulează Se indexează E04" is broken Romanian, read aloud on every running job.
    for (const key of ["shell:activity.cancelJob", "shell:activity.stopJob"]) expect(text(key)).toMatch(/^\p{L}+: \{\{title\}\}$/u)
  })
})
