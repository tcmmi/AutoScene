import { describe, expect, it } from "vitest"
import { formatScreenTime, plural } from "@/features/cast/cast"
import { inLanguage } from "@/test/i18n"
import {
  formatBytes,
  formatCount,
  formatDateTime,
  formatDuration,
  formatEta,
  formatPace,
  formatPercent,
  formatProgress,
  presetDescription,
  presetName,
  shortFormatName,
} from "./format"

describe("presetName", () => {
  const preset = {
    name: "H.264 · Mobile (CapCut, Alight Motion)",
    nameMsg: { key: "export.preset.h264Mobile.name", text: "H.264 · Mobile (CapCut, Alight Motion)" },
    description: "1080p H.264 with stereo sound that imports cleanly into phone editors.",
    descriptionMsg: { key: "export.preset.h264Mobile.description", text: "1080p H.264 with stereo sound that imports cleanly into phone editors." },
  }

  it("says a format in the UI's language", async () => {
    expect(presetName(preset)).toBe("H.264 · Mobile (CapCut, Alight Motion)")
    expect(presetDescription(preset)).toBe(preset.description)
    await inLanguage("ro", {
      backend: { export: { preset: { h264Mobile: { name: "H.264 · Telefon (CapCut)", description: "H.264 la 1080p." } } } },
    })
    expect(presetName(preset)).toBe("H.264 · Telefon (CapCut)")
    expect(presetDescription(preset)).toBe("H.264 la 1080p.")
    expect(shortFormatName(presetName(preset))).toBe("H.264 · Telefon")
  })

  it("an export kept before formats had keys says its English in every language", async () => {
    await inLanguage("ro", { backend: { export: { preset: { h264Mobile: { name: "H.264 · Telefon" } } } } })
    expect(presetName({ name: "ProRes 422 HQ" })).toBe("ProRes 422 HQ")
    expect(presetName({ name: "ProRes 422 HQ", nameMsg: null })).toBe("ProRes 422 HQ")
    expect(presetDescription({ description: "Large files." })).toBe("Large files.")
  })
})

describe("shortFormatName", () => {
  it("drops what the brackets add", () => {
    expect(shortFormatName("H.264 · Mobile (CapCut, Alight Motion)")).toBe("H.264 · Mobile")
    expect(shortFormatName("H.264 · All-Intra (Twixtor)")).toBe("H.264 · All-Intra")
    expect(shortFormatName("HEVC 10-bit (keeps HDR)")).toBe("HEVC 10-bit")
  })

  it("keeps a name without them, or that is nothing but them", () => {
    expect(shortFormatName("ProRes 422 HQ")).toBe("ProRes 422 HQ")
    expect(shortFormatName("(custom)")).toBe("(custom)")
  })
})

describe("indexing numbers", () => {
  it("groups counts and gives paces the digits they hold", () => {
    expect(formatCount(14_200)).toBe("14,200")
    expect(formatCount(88_499.6)).toBe("88,500")
    expect(formatPace(612.4)).toBe("612")
    expect(formatPace(2853.4)).toBe("2,850")
    expect(formatPace(25.3)).toBe("25")
    expect(formatPace(7.46)).toBe("7.5")
    expect(formatPace(0.8)).toBe("0.8")
    expect(formatPace(9.97)).toBe("10")
  })
})

describe("formatProgress", () => {
  it("gives the time left before the percentage, as the Activity drawer does", () => {
    expect(formatProgress(0.42, 150_000)).toBe("about 3 min left · 42%")
    expect(formatProgress(0.42, 30_000)).toBe("30 s left · 42%")
    expect(formatProgress(0.99, 2_000)).toBe("almost done · 99%")
  })

  it("leaves out what isn't known yet", () => {
    expect(formatProgress(0.42, null)).toBe("42%")
    expect(formatProgress(null, null)).toBe("")
    expect(formatEta(Number.NaN)).toBe("")
  })
})

describe("English as it always read", () => {
  it("writes sizes with AutoScene's units, ungrouped", () => {
    expect(formatBytes(0)).toBe("0 B")
    expect(formatBytes(-1)).toBe("0 B")
    expect(formatBytes(512)).toBe("512 B")
    expect(formatBytes(1536)).toBe("1.5 KB")
    expect(formatBytes(1.5 * 1024 ** 3, 2)).toBe("1.50 GB")
    expect(formatBytes(3 * 1024 ** 5)).toBe("3072.0 TB")
  })

  it("writes dates the en-GB way, times as clock digits and screen time as people say it", () => {
    // Noon UTC: the 13th in every time zone the tests run in.
    expect(formatDateTime(Date.UTC(2026, 9, 13, 12, 0))).toMatch(/^13 Oct, \d\d:\d\d$/)
    expect(formatDuration(3725.4)).toBe("1:02:05")
    expect(formatDuration(65)).toBe("1:05")
    expect(formatDuration(-1)).toBe("–")
    expect(formatScreenTime(0)).toBe("0 s")
    expect(formatScreenTime(42.4)).toBe("42 s")
    expect(formatScreenTime(59.6)).toBe("60 s")
    expect(formatScreenTime(720)).toBe("12 min")
    expect(formatScreenTime(3900)).toBe("1 h 05 min")
    expect(plural(1, "face")).toBe("1 face")
    expect(plural(14_200, "face")).toBe("14,200 faces")
    expect(plural(2, "person", "people")).toBe("2 people")
  })

  it("keeps the time left and percentages", () => {
    expect(formatEta(4_000)).toBe("almost done")
    expect(formatEta(59_000)).toBe("59 s left")
    expect(formatEta(59 * 60_000)).toBe("about 59 min left")
    expect(formatEta(90 * 60_000)).toBe("about 1.5 h left")
    expect(formatEta(120 * 60_000)).toBe("about 2 h left")
    expect(formatPercent(0.4)).toBe("40%")
    expect(formatPercent(null)).toBe("")
  })
})

describe("another language", () => {
  it("writes dates, sizes, paces and lists the ro-RO way, with the catalog's words", async () => {
    await inLanguage("ro", { common: { format: { eta: { minutes: "cam {{m}} min rămase" }, percent: "{{value}} %" } } })
    expect(formatDateTime(Date.UTC(2026, 9, 13, 12, 0))).toMatch(/^13 oct\., \d\d:\d\d$/)
    expect(formatBytes(1.5 * 1024 ** 3)).toBe("1,5 GB")
    expect(formatCount(14_200)).toBe("14.200")
    expect(formatPace(7.46)).toBe("7,5")
    expect(formatPace(2853.4)).toBe("2.850")
    expect(formatEta(150_000)).toBe("cam 3 min rămase")
    expect(formatPercent(0.42)).toBe("42 %")
    // A word the language doesn't have yet stays English.
    expect(formatEta(30_000)).toBe("30 s left")
  })
})
