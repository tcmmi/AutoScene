import { describe, expect, it } from "vitest"
import type { AppError, Msg } from "@/bindings"
import { inLanguage } from "@/test/i18n"
import { say } from "./i18n"
import { errorMessage } from "./ipc"

const msg = (key: string, text: string, args: Msg["args"] = {}): Msg => ({ key, args, text })

const romanian = {
  backend: {
    file: { diskFull: "Discul este plin." },
    fixture: {
      named: "Pluginul {{name}} așteaptă.",
      wrote: "Nu pot scrie în {{path}}: {{why}}",
      clips_one: "{{count}} clip",
      clips_few: "{{count}} clipuri",
      clips_other: "{{count}} de clipuri",
    },
  },
}

const diskFull: AppError = { code: "io", message: "The disk is full.", msg: msg("file.diskFull", "The disk is full.") }

describe("say", () => {
  it("says a known key in the language, with its values", async () => {
    await inLanguage("ro", romanian)
    expect(say(msg("fixture.named", "Poster waits.", { name: "Poster" }))).toBe("Pluginul Poster așteaptă.")
  })

  it("picks the language's plural form from the count, and groups it", async () => {
    await inLanguage("ro", romanian)
    const clips = (count: number) => say(msg("fixture.clips", `${count} clips`, { count }))
    expect(clips(1)).toBe("1 clip")
    expect(clips(3)).toBe("3 clipuri")
    expect(clips(20)).toBe("20 de clipuri")
    expect(clips(14200)).toBe("14.200 de clipuri")
  })

  it("groups each number of a job's sentence, not only its count", () => {
    const left = msg("job.clipsFailedSomeLeft", "1200 clips couldn't be exported; the file has the other 3400.", { count: 1200, rest: 3400 })
    expect(say(left)).toBe("1,200 clips couldn't be exported; the file has the other 3,400.")
    const stage = (key: string, word: string) => say(msg(key, `${word} 1200 of 3400`, { n: 1200, total: 3400 }))
    expect(stage("job.stage.clip", "Clip")).toBe("Clip 1,200 of 3,400")
    expect(stage("job.stage.poster", "Poster")).toBe("Poster 1,200 of 3,400")
    expect(stage("job.stage.portrait", "Portrait")).toBe("Portrait 1,200 of 3,400")
  })

  it("says a nested Msg first", async () => {
    await inLanguage("ro", romanian)
    const why = msg("file.diskFull", "The disk is full.")
    const said = msg("fixture.wrote", "Can't write to D:/out: The disk is full.", { path: "D:/out", why })
    expect(say(said)).toBe("Nu pot scrie în D:/out: Discul este plin.")
  })

  it("shows the English for a key the catalog doesn't have", async () => {
    expect(say(msg("not.aKey", "From a newer backend."))).toBe("From a newer backend.")
    await inLanguage("ro", romanian)
    expect(say(msg("not.aKey", "From a newer backend."))).toBe("From a newer backend.")
  })

  it("falls back to the English catalog for a key the language doesn't have yet", async () => {
    await inLanguage("ro", romanian)
    expect(say(msg("internal", "text"))).toBe("Something went wrong. Details are in the log.")
  })

  it("leaves words AutoScene didn't write and plain strings as they came", async () => {
    await inLanguage("ro", romanian)
    expect(say("The webhook said no.")).toBe("The webhook said no.")
    expect(say(msg("", "The webhook said no."))).toBe("The webhook said no.")
    expect(say({ key: "", text: "{{name}} $t(file.diskFull)" })).toBe("{{name}} $t(file.diskFull)")
  })

  it("keeps a value a value, whatever it says or is called", async () => {
    await inLanguage("ro", romanian)
    const name = "{{name}} $t(backend:file.diskFull)"
    expect(say(msg("fixture.named", "text", { name }))).toBe(`Pluginul ${name} așteaptă.`)
    // Each of these means something to `t` as an option.
    const bundle = { backend: { fixture: { options: "{{lng}} {{ns}} {{context}} {{defaultValue}}" } } }
    await inLanguage("ro", bundle)
    const said = msg("fixture.options", "text", { lng: "de", ns: "cast", context: "male", defaultValue: "no" })
    expect(say(said)).toBe("de cast male no")
  })
})

describe("errorMessage", () => {
  it("says an AppError's msg in the language", async () => {
    expect(errorMessage(diskFull)).toBe("The disk is full.")
    await inLanguage("ro", romanian)
    expect(errorMessage(diskFull)).toBe("Discul este plin.")
  })

  it("says the English an error without a key has", async () => {
    await inLanguage("ro", romanian)
    expect(errorMessage({ code: "not_found", message: "Gone.", msg: msg("", "Gone.") })).toBe("Gone.")
    // As a test's or an old mock's error has it.
    expect(errorMessage({ code: "not_found", message: "Gone." })).toBe("Gone.")
  })

  it("reads what isn't an AppError as before", () => {
    expect(errorMessage(new Error("A bug."))).toBe("A bug.")
    expect(errorMessage("Text.")).toBe("Text.")
  })
})
