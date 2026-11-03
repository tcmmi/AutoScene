import { describe, expect, it } from "vitest"
import type { GraphicsClass, ScanChoice, ScanDefaults } from "@/bindings"
import { fastScanHint, newProjectHint, settingsLine, showsSuits, suits, what } from "./fast-scan"

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

describe("the New Project dialog's line", () => {
  const COST = "Much faster, finds fewer faces."
  const hint = (cls: GraphicsClass, content: "live" | "mixed" | "animation" | "anime" = "live", choice: ScanChoice = "auto", bodies = false) =>
    newProjectHint(content, bodies, defaults(cls, choice, bodies))

  it("says only what Fast Scan costs on graphics built for AI work, or where the person chose On or Off", () => {
    expect(hint("forAi")).toBe(COST)
    expect(hint("unknown")).toBe(COST)
    for (const cls of ["forAi", "notForAi", "integrated", "processor", "unknown"] as const) {
      expect(hint(cls, "live", "fast")).toBe(COST)
      expect(hint(cls, "live", "full")).toBe(COST)
      expect(hint(cls, "mixed", "fast")).toBe(COST)
    }
  })

  it("says why it is on where the graphics aren't built for AI work", () => {
    expect(hint("integrated")).toBe("Much faster, finds fewer faces. Suits this computer's built-in graphics.")
    expect(hint("notForAi")).toBe("Much faster, finds fewer faces. Suits this computer's graphics card.")
    expect(hint("processor")).toBe("Much faster, finds fewer faces. Suits analysis on the processor.")
    expect(hint("integrated", "mixed")).toBe("Much faster, finds fewer faces. Suits this computer's built-in graphics.")
  })

  it("says it misses faceless people while they are found, and then gives no reason: live action keeps the full scan", () => {
    for (const cls of ["forAi", "integrated", "notForAi", "processor"] as const) {
      expect(hint(cls, "live", "auto", true)).toBe("Much faster, finds fewer faces and no one without a visible face.")
    }
  })

  it("says what it costs 3D animation, which keeps the full scan on Automatic", () => {
    for (const cls of ["forAi", "integrated", "notForAi", "processor"] as const) {
      expect(hint(cls, "animation")).toBe("Much faster, but misses many animated faces.")
    }
    expect(hint("integrated", "animation", "auto", true)).toBe(
      "Much faster, but misses many animated faces and anyone without a visible face.",
    )
    // On is the person's choice: no reason from the graphics.
    expect(hint("integrated", "animation", "fast")).toBe("Much faster, but misses many animated faces.")
  })

  it("says anime finds fewer characters, and why it is on, faceless people or not", () => {
    expect(hint("integrated", "anime")).toBe("Much faster, finds fewer characters. Suits this computer's built-in graphics.")
    expect(hint("integrated", "anime", "auto", true)).toBe(
      "Much faster, finds fewer characters. Suits this computer's built-in graphics.",
    )
    expect(hint("forAi", "anime")).toBe("Much faster, finds fewer characters.")
  })

  it("without the defaults says only what Fast Scan costs", () => {
    expect(newProjectHint("live", false, undefined)).toBe(COST)
    expect(newProjectHint("anime", true, undefined)).toBe(what("anime", true))
  })

  it("suits only the classes Automatic gives Fast Scan, and never names the device or a number", () => {
    expect(suits("forAi")).toBe("")
    expect(suits("unknown")).toBe("")
    expect(showsSuits("live", defaults("forAi"))).toBe(false)
    for (const cls of ["forAi", "notForAi", "integrated", "processor", "unknown"] as const) {
      for (const content of ["live", "mixed", "animation", "anime"] as const) {
        for (const bodies of [false, true]) {
          const d = { ...defaults(cls, "auto", bodies), computer: { class: cls, name: "NVIDIA GeForce GTX 1060 6GB" } }
          const line = newProjectHint(content, bodies, d).replaceAll("3D animation", "")
          expect(line).not.toMatch(/GTX|NVIDIA|DirectML|YuNet|SCRFD|\d|Recommended/)
        }
      }
    }
  })
})

describe("Edit details' Fast Scan line", () => {
  const FULL = "Faces are found in full: every few frames, with the larger face models (for non-commercial use only)."
  const ANIME_FULL = "Characters are found with the anime models every few frames."

  it("says Fast Scan suits this computer's graphics for a full-scan project a new one would give Fast Scan", () => {
    expect(fastScanHint("live", false, false, defaults("integrated"))).toBe(`${FULL} Fast Scan suits this computer's built-in graphics.`)
    expect(fastScanHint("mixed", false, false, defaults("notForAi"))).toBe(`${FULL} Fast Scan suits this computer's graphics card.`)
    expect(fastScanHint("anime", false, false, defaults("processor"))).toBe(`${ANIME_FULL} Fast Scan suits analysis on the processor.`)
  })

  it("adds nothing where a new project of its footage keeps the full scan, or the person chose", () => {
    expect(fastScanHint("live", false, false, defaults("forAi"))).toBe(FULL)
    expect(fastScanHint("live", false, false, defaults("unknown"))).toBe(FULL)
    expect(fastScanHint("animation", false, false, defaults("integrated"))).toBe(FULL)
    expect(fastScanHint("live", false, false, defaults("integrated", "auto", true))).toBe(FULL)
    expect(fastScanHint("live", false, false, defaults("integrated", "fast"))).toBe(FULL)
    expect(fastScanHint("live", false, false, undefined)).toBe(FULL)
    expect(fastScanHint("anime", false, false, defaults("forAi"))).toBe(ANIME_FULL)
  })

  it("keeps what it said for Fast Scan and for a switch", () => {
    const d = defaults("integrated")
    expect(fastScanHint("live", false, true, d)).toBe("Faces are found the way the Scener app does: much faster, less sure who is who.")
    expect(fastScanHint("live", true, false, d)).toBe(
      "Faces are found again this way, then the characters regrouped. Your names and decisions stay.",
    )
    expect(fastScanHint("anime", false, true, d)).toBe(
      "Characters are found with the anime models in only a few frames of each shot: much faster, less sure who is who.",
    )
    expect(fastScanHint("anime", true, true, d)).toBe("Characters are found again this way, then regrouped. Your names and decisions stay.")
  })
})
