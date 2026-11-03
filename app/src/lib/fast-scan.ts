import type { Platform, ScanDefaults } from "@/bindings"

/*
 * Every sentence about Fast Scan's default for new projects (design-i52 §5.5),
 * so Settings, the New Project dialog and Edit details say the same thing. No
 * engine names, no numbers, no "Recommended".
 */

/** Whether a new live-action project starts with Fast Scan. */
const liveFast = (d: ScanDefaults) => d.footage.find((f) => f.content === "live")?.fast ?? false

/**
 * Settings → General → Fast Scan: what the chosen default means on this
 * computer. It names no device, so it stays true whichever card is chosen.
 */
export function settingsLine(d: ScanDefaults, platform: Platform): string {
  if (d.choice === "fast") return "Every new project starts with Fast Scan, 3D animation too."
  if (d.choice === "full") return "Every new project starts with the full scan."
  const here = platform === "macos" ? "this Mac" : "this computer"
  // On Automatic, live action keeps the full scan only while people without a visible face are found.
  const keeps = liveFast(d)
    ? " 3D animation keeps the full scan."
    : " 3D animation keeps the full scan, and so does live action while Find characters without a visible face is on."
  switch (d.computer.class) {
    case "integrated":
      return `Here, Automatic means Fast Scan: ${here} analyses video on its built-in graphics.${keeps}`
    case "notForAi":
      return `Here, Automatic means Fast Scan: this computer's graphics card has no hardware made for AI work.${keeps}`
    case "processor":
      return `Here, Automatic means Fast Scan: AutoScene analyses video on ${here}'s processor.${keeps}`
    case "forAi":
      return "Here, Automatic means the full scan: this computer's graphics card is built for AI work."
    case "unknown":
      return "Here, Automatic means the full scan: AutoScene couldn't recognise this computer's graphics."
  }
}
