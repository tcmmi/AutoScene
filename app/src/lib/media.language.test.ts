import { afterEach, describe, expect, it, vi } from "vitest"
import type { MediaInfo, MediaItem } from "@/bindings"
import { inLanguage } from "@/test/i18n"

const { open } = vi.hoisted(() => ({ open: vi.fn() }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open, save: vi.fn() }))

import { audioLabel, audioOption, formatFlags, languageName, pickPluginFile, pickVideoFile, pickVideoFiles, PREFERRED_AUDIO_LANGUAGES } from "./media"

afterEach(() => open.mockReset())

/** The names the code table held before the platform named languages: ISO 639-2 (both forms) and 639-1. */
const NAMES: Record<string, string> = {
  eng: "English", en: "English",
  jpn: "Japanese", ja: "Japanese",
  deu: "German", ger: "German", de: "German",
  fra: "French", fre: "French", fr: "French",
  spa: "Spanish", es: "Spanish",
  ita: "Italian", it: "Italian",
  por: "Portuguese", pt: "Portuguese",
  rus: "Russian", ru: "Russian",
  kor: "Korean", ko: "Korean",
  zho: "Chinese", chi: "Chinese", zh: "Chinese",
  hin: "Hindi", hi: "Hindi",
  ara: "Arabic", ar: "Arabic",
  tur: "Turkish", tr: "Turkish",
  pol: "Polish", pl: "Polish",
  nld: "Dutch", dut: "Dutch", nl: "Dutch",
  swe: "Swedish", sv: "Swedish",
  nor: "Norwegian", no: "Norwegian",
  dan: "Danish", da: "Danish",
  fin: "Finnish", fi: "Finnish",
  ces: "Czech", cze: "Czech", cs: "Czech",
  hun: "Hungarian", hu: "Hungarian",
  ell: "Greek", gre: "Greek", el: "Greek",
  heb: "Hebrew", he: "Hebrew",
  tha: "Thai", th: "Thai",
  vie: "Vietnamese", vi: "Vietnamese",
  ind: "Indonesian", id: "Indonesian",
  ukr: "Ukrainian", uk: "Ukrainian",
  ron: "Romanian", rum: "Romanian", ro: "Romanian",
  tam: "Tamil", ta: "Tamil",
  tel: "Telugu", te: "Telugu",
}

const track = (o: Partial<MediaInfo["audio"][number]>): MediaInfo["audio"][number] => ({
  index: 1,
  ordinal: 0,
  codec: "ac3",
  profile: null,
  channels: 2,
  channelLayout: null,
  sampleRate: 48000,
  language: null,
  title: null,
  default: true,
  commentary: false,
  ...o,
})

describe("audio languages are named in the UI's language", () => {
  it("names every code of the table the way English always did, byte for byte", () => {
    for (const [code, name] of Object.entries(NAMES)) expect(languageName(code), code).toBe(name)
    expect(Object.keys(NAMES)).toHaveLength(67)
  })

  it("reads the code as the code where AutoScene has no name for it", () => {
    expect(languageName("xyz")).toBe("XYZ")
    expect(languageName("mul")).toBe("MUL")
    expect(languageName("und")).toBe("UND")
    expect(languageName("hrv")).toBe("HRV")
    expect(languageName(null)).toBeNull()
    expect(languageName("")).toBeNull()
  })

  it("asks the platform for the /T form of a /B code, which an older platform may not know", () => {
    const of = vi.spyOn(Intl.DisplayNames.prototype, "of")
    try {
      for (const [b, t] of [["ger", "deu"], ["fre", "fra"], ["chi", "zho"], ["dut", "nld"], ["gre", "ell"], ["rum", "ron"], ["cze", "ces"]]) {
        of.mockClear()
        languageName(b)
        expect(of, b).toHaveBeenCalledWith(t)
      }
    } finally {
      of.mockRestore()
    }
  })

  it("names a code with a region or in capitals as the language", () => {
    expect(languageName("pt-BR")).toBe("Portuguese")
    expect(languageName("ENG")).toBe("English")
  })

  it("capitalises a name like a list item in another language: Germană, not germană", async () => {
    await inLanguage("ro")
    expect(languageName("ger")).toBe("Germană")
    expect(languageName("deu")).toBe("Germană")
    expect(languageName("xyz")).toBe("XYZ")
    expect(audioLabel(track({ language: "deu" }))).toBe("Germană · Dolby Digital · Stereo")
  })

  it("labels the automatic choices in it too, the file's own track included", async () => {
    expect(PREFERRED_AUDIO_LANGUAGES.map((l) => l.label)).toEqual([
      "English", "Japanese", "German", "French", "Spanish", "Italian", "Portuguese", "Korean", "Chinese", "Hindi", "Russian",
      "Each file's default track",
    ])
    expect(PREFERRED_AUDIO_LANGUAGES.map((l) => l.value).at(-1)).toBe("")
    await inLanguage("ro", { media: { audio: { defaultTrack: "Pista implicită a fiecărui fișier" } } })
    const labels = PREFERRED_AUDIO_LANGUAGES.map((l) => l.label)
    expect(labels[0]).toBe("Engleză")
    expect(labels[2]).toBe("Germană")
    expect(labels.at(-1)).toBe("Pista implicită a fiecărui fișier")
    // A copy made while rendering, as Settings makes one, keeps the words it had then.
    expect(PREFERRED_AUDIO_LANGUAGES.map((l) => ({ ...l, value: l.value || "default" }))[2].label).toBe("Germană")
  })
})

describe("media facts in the UI's language", () => {
  it("keeps today's English for tracks and flags", () => {
    expect(audioLabel(track({ language: "hin", channels: 1 }))).toBe("Hindi · Dolby Digital · Mono")
    expect(audioLabel(track({ ordinal: 2, codec: "aac", channels: 4, commentary: true }))).toBe("Track 3 · AAC · 4 ch · Commentary")
    expect(audioLabel(track({ language: "eng", channels: 6 }))).toBe("English · Dolby Digital · 5.1")
    // Every layout has its own word or number; any other count is "N ch".
    for (const [channels, label] of [[1, "Mono"], [2, "Stereo"], [3, "2.1"], [4, "4 ch"], [5, "5 ch"], [6, "5.1"], [7, "7 ch"], [8, "7.1"], [10, "10 ch"]] as const) {
      expect(audioLabel(track({ language: "eng", channels })), String(channels)).toBe(`English · Dolby Digital · ${label}`)
    }
    const m = { info: { video: { hdr: "pq", bitDepth: 10 } }, vfr: true, interlaced: true } as unknown as MediaItem
    expect(formatFlags(m)).toEqual(["HDR10", "10-bit", "VFR", "Interlaced"])
  })

  it("says tracks and flags in fixture words, product names as they are", async () => {
    await inLanguage("ro", {
      media: {
        audio: { mono: "Mono", stereo: "Stereo", channels: "{{n}} can.", track: "Pista {{n}}", commentary: "Comentarii" },
        flags: { bitDepth: "{{n}} biți", interlaced: "Întrețesut" },
      },
    })
    expect(audioLabel(track({ ordinal: 2, codec: "aac", channels: 4, commentary: true }))).toBe("Pista 3 · AAC · 4 can. · Comentarii")
    expect(audioOption(track({ language: "eng", title: "Director" })).description).toBe("Director")
    const m = { info: { video: { hdr: "dolby_vision", bitDepth: 10 } }, interlaced: true } as unknown as MediaItem
    expect(formatFlags(m)).toEqual(["Dolby Vision", "10 biți", "Întrețesut"])
  })
})

describe("the system's file dialogs", () => {
  it("name the file types in the language in use", async () => {
    open.mockResolvedValue(null)
    await pickVideoFiles()
    await pickVideoFile("Pick")
    await pickPluginFile()
    expect(open.mock.calls.map(([o]) => o.filters[0].name)).toEqual(["Video", "Video", "AutoScene plugin"])
    expect(open.mock.calls[2][0].title).toBe("Install a plugin")

    open.mockClear()
    await inLanguage("ro", { media: { dialog: { videoFilter: "Video", pluginFilter: "Modul AutoScene", installPlugin: "Instalează un modul" } } })
    await pickVideoFiles()
    await pickPluginFile()
    expect(open.mock.calls[1][0].filters[0].name).toBe("Modul AutoScene")
    expect(open.mock.calls[1][0].title).toBe("Instalează un modul")
  })
})
