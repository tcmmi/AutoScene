import { describe, expect, it } from "vitest"
import type { GraphicsClass, ScanChoice, ScanDefaults } from "@/bindings"
import { settingsLine } from "./fast-scan"

/** What the library answers on `cls`, like `as_project::scan::defaults`. */
function defaults(cls: GraphicsClass, choice: ScanChoice = "auto", findBodies = false): ScanDefaults {
  const fastHere = cls === "notForAi" || cls === "integrated" || cls === "processor"
  const fast = (content: "live" | "mixed" | "animation" | "anime") =>
    choice === "auto" ? fastHere && content !== "animation" && (content === "anime" || !findBodies) : choice === "fast"
  return {
    computer: { class: cls, name: cls === "processor" ? "" : "Graphics" },
    choice,
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({ content, fast: fast(content) })),
  }
}

const KEEPS = " 3D animation keeps the full scan."
const KEEPS_BODIES = " 3D animation keeps the full scan, and so does live action while Find characters without a visible face is on."

describe("settingsLine", () => {
  it("says Automatic means Fast Scan where the graphics aren't built for AI work", () => {
    expect(settingsLine(defaults("integrated"), "windows")).toBe(
      `Here, Automatic means Fast Scan: this computer analyses video on its built-in graphics.${KEEPS}`,
    )
    expect(settingsLine(defaults("notForAi"), "windows")).toBe(
      `Here, Automatic means Fast Scan: this computer's graphics card has no hardware made for AI work.${KEEPS}`,
    )
    expect(settingsLine(defaults("processor"), "windows")).toBe(
      `Here, Automatic means Fast Scan: AutoScene analyses video on this computer's processor.${KEEPS}`,
    )
    expect(settingsLine(defaults("processor"), "linux")).toBe(
      `Here, Automatic means Fast Scan: AutoScene analyses video on this computer's processor.${KEEPS}`,
    )
  })

  it("speaks of this Mac on a Mac", () => {
    expect(settingsLine(defaults("integrated"), "macos")).toBe(
      `Here, Automatic means Fast Scan: this Mac analyses video on its built-in graphics.${KEEPS}`,
    )
    expect(settingsLine(defaults("processor"), "macos")).toBe(`Here, Automatic means Fast Scan: AutoScene analyses video on this Mac's processor.${KEEPS}`)
  })

  it("says Automatic means the full scan where the graphics are built for AI work or unknown", () => {
    for (const platform of ["windows", "macos", "linux"] as const) {
      expect(settingsLine(defaults("forAi"), platform)).toBe("Here, Automatic means the full scan: this computer's graphics card is built for AI work.")
      expect(settingsLine(defaults("unknown"), platform)).toBe(
        "Here, Automatic means the full scan: AutoScene couldn't recognise this computer's graphics.",
      )
      // Finding faceless people changes nothing where the full scan is the default anyway.
      expect(settingsLine(defaults("forAi", "auto", true), platform)).toBe(settingsLine(defaults("forAi"), platform))
    }
  })

  it("names live action among what keeps the full scan while faceless people are found", () => {
    for (const cls of ["integrated", "notForAi", "processor"] as const) {
      const line = settingsLine(defaults(cls, "auto", true), "windows")
      expect(line.startsWith("Here, Automatic means Fast Scan: ")).toBe(true)
      expect(line.endsWith(KEEPS_BODIES)).toBe(true)
    }
  })

  it("says On and Off apply to every new project, whatever the computer", () => {
    for (const cls of ["forAi", "notForAi", "integrated", "processor", "unknown"] as const) {
      for (const bodies of [false, true]) {
        expect(settingsLine(defaults(cls, "fast", bodies), "windows")).toBe("Every new project starts with Fast Scan, 3D animation too.")
        expect(settingsLine(defaults(cls, "full", bodies), "macos")).toBe("Every new project starts with the full scan.")
      }
    }
  })

  it("never names the device, the engine or a number", () => {
    for (const cls of ["forAi", "notForAi", "integrated", "processor", "unknown"] as const) {
      const d = { ...defaults(cls), computer: { class: cls, name: "NVIDIA GeForce GTX 1060 6GB" } }
      const line = settingsLine(d, "windows")
      // "3D animation" is the footage's name, not a number.
      expect(line.replaceAll("3D animation", "")).not.toMatch(/GTX|NVIDIA|DirectML|YuNet|SCRFD|\d|Recommended/)
    }
  })
})
