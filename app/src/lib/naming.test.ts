import { describe, expect, it } from "vitest"
import { FILE_PARTS, insertion, parseTemplate, reportedPart } from "./naming"

/** The pieces as [kind, text], easier to read than indices. */
const pieces = (template: string, allowed?: Parameters<typeof parseTemplate>[1]) =>
  parseTemplate(template, allowed).map((s) => [s.kind, s.text])

describe("parseTemplate", () => {
  it("reads a template as the backend does", () => {
    expect(pieces("{series} {ep} {tc}")).toEqual([
      ["part", "{series}"],
      ["text", " "],
      ["unknown", "{ep}"],
      ["text", " "],
      ["part", "{tc}"],
    ])
    // A "{" runs to the first "}" after it, whatever is between.
    expect(pieces("{a{b}")).toEqual([["unknown", "{a{b}"]])
    expect(pieces("{}x")).toEqual([
      ["unknown", "{}"],
      ["text", "x"],
    ])
    // A part not closed runs to the end.
    expect(pieces("E{series} {tc")).toEqual([
      ["text", "E"],
      ["part", "{series}"],
      ["text", " "],
      ["unclosed", "{tc"],
    ])
    expect(parseTemplate("")).toEqual([])
  })

  it("tells a part the field may use from one that names a shot", () => {
    expect(pieces("{series} {tc}", FILE_PARTS)).toEqual([
      ["part", "{series}"],
      ["text", " "],
      ["shot", "{tc}"],
    ])
    expect(pieces("{tc}")).toEqual([["part", "{tc}"]])
  })

  it("counts where each piece is as the field does (UTF-16)", () => {
    expect(parseTemplate("第05話{tc}")).toEqual([
      { kind: "text", start: 0, end: 4, text: "第05話" },
      { kind: "part", start: 4, end: 8, text: "{tc}" },
    ])
    // A character outside the BMP is two units in the field.
    expect(parseTemplate("🎬{tc}").map((s) => [s.start, s.end])).toEqual([
      [0, 2],
      [2, 6],
    ])
    const template = "{series} 第05話 {nme} {tc"
    expect(
      parseTemplate(template)
        .map((s) => s.text)
        .join(""),
    ).toBe(template)
  })
})

describe("reportedPart", () => {
  /** What the backend names in `template`, for the one file's name or not: [kind, text], or null. */
  const reported = (template: string, forFile = false) => {
    const s = reportedPart(parseTemplate(template, forFile ? FILE_PARTS : undefined), forFile)
    return s && [s.kind, s.text, s.start, s.end]
  }

  it("names the first unknown or unclosed part of a shot's name", () => {
    expect(reported("{ep} {x")).toEqual(["unknown", "{ep}", 0, 4])
    expect(reported("{series} {x {ep}")).toEqual(["unknown", "{x {ep}", 9, 16])
    expect(reported("E{index} {tc")).toEqual(["unclosed", "{tc", 9, 12])
    expect(reported("{series} {tc}")).toBeNull()
    // Empty: refused, but no part says why.
    expect(reported("  ")).toBeNull()
  })

  it("looks at the one file's name twice, as the backend does", () => {
    // First an unclosed part or one that names a shot, wherever an unknown one is.
    expect(reported("{nme} {tc}", true)).toEqual(["shot", "{tc}", 6, 10])
    expect(reported("{nme} {series", true)).toEqual(["unclosed", "{series", 6, 13])
    expect(reported("{index} {tc}", true)).toEqual(["shot", "{index}", 0, 7])
    // Then an unknown one. A "{" runs to the first "}": no shot's part in it.
    expect(reported("{ser {tc}", true)).toEqual(["unknown", "{ser {tc}", 0, 9])
    expect(reported("{series} {nme}", true)).toEqual(["unknown", "{nme}", 9, 14])
    expect(reported("{series} {character}", true)).toBeNull()
  })
})

/** The template after `part` goes in at `start`..`end`, and where the caret ends. */
function put(value: string, start: number, end: number, part: Parameters<typeof insertion>[3]) {
  const at = insertion(value, start, end, part)
  return { value: value.slice(0, at.start) + at.text + value.slice(at.end), caret: at.start + at.text.length }
}

describe("insertion", () => {
  it("puts a space between two parts that would touch", () => {
    expect(put("{series}", 8, 8, "episode")).toEqual({ value: "{series} {episode}", caret: 18 })
    expect(put("{tc}", 0, 0, "series")).toEqual({ value: "{series} {tc}", caret: 9 })
    expect(put("{series}{tc}", 8, 8, "episode").value).toBe("{series} {episode} {tc}")
  })

  it("lets plain text touch a part", () => {
    expect(put("E", 1, 1, "index").value).toBe("E{index}")
    expect(put("_x", 0, 0, "series").value).toBe("{series}_x")
    expect(put("", 0, 0, "series")).toEqual({ value: "{series}", caret: 8 })
  })

  it("replaces the selection, as a key would", () => {
    expect(put("{series} x{tc}", 10, 14, "episode")).toEqual({ value: "{series} x{episode}", caret: 19 })
  })

  it("never splits a part", () => {
    expect(put("{series}", 4, 4, "episode").value).toBe("{series} {episode}")
    expect(put("{ep} x", 2, 2, "tc").value).toBe("{ep} {tc} x")
    // A part still being typed (no "}" yet) is left to the person typing it.
    expect(put("{ser", 4, 4, "episode").value).toBe("{ser{episode}")
  })
})
