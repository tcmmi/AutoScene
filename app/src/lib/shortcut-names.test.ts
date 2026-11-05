import { describe, expect, it } from "vitest"
import { en } from "@/locales"
import { inLanguage } from "@/test/i18n"
import { i18n } from "./i18n"
import {
  ALL_SHORTCUTS,
  PRESETS,
  SHORTCUT_GROUPS,
  keyCap,
  shortcutName,
  shortcutOf,
  slotName,
} from "./shortcuts"

// The words of the shortcuts live in the `shortcuts` catalog under keys the code builds
// (`<scope>.<id>.name`, `.slots.<n>`, `group.<scope>.title` …), so no type check sees them:
// these tests walk every one.

describe("the names of the shortcuts", () => {
  it("every shortcut has a name in the catalog", () => {
    for (const s of ALL_SHORTCUTS) {
      expect(i18n.exists(`shortcuts:${s.scope}.${s.id}.name`), `${s.scope}.${s.id}`).toBe(true)
      expect(shortcutName(s)).toMatch(/^[A-Z]/)
    }
  })

  it("every key of a shortcut that does a few things has its own name, and no other has one", () => {
    for (const s of ALL_SHORTCUTS) {
      s.slots.forEach((_, i) => {
        const own = i18n.exists(`shortcuts:${s.scope}.${s.id}.slots.${i}`)
        expect(own, `${s.scope}.${s.id}.${i}`).toBe(s.slots.length > 1)
        expect(slotName(s, i)).toMatch(/^[A-Z]/)
      })
      // The row's own name, where a slot has none.
      if (s.slots.length === 1) expect(slotName(s, 0)).toBe(shortcutName(s))
    }
  })

  it("the catalog lists exactly the shortcuts there are: no name is left over", () => {
    const listed = Object.fromEntries(
      Object.entries(en.shortcuts)
        .filter(([scope]) => ["global", "player", "review", "cast", "faces", "shots", "pack"].includes(scope))
        .map(([scope, items]) => [scope, Object.keys(items).sort()]),
    )
    const there: Record<string, string[]> = {}
    for (const s of ALL_SHORTCUTS) (there[s.scope] ??= []).push(s.id)
    for (const ids of Object.values(there)) ids.sort()
    expect(listed).toEqual(there)
  })

  it("each group, preset and key name has its words", () => {
    for (const g of SHORTCUT_GROUPS) {
      expect(i18n.exists(`shortcuts:group.${g.id}.title`), g.id).toBe(true)
      expect(i18n.exists(`shortcuts:group.${g.id}.description`), g.id).toBe(true)
      expect(g.title).not.toContain("shortcuts:")
    }
    for (const p of PRESETS) expect(i18n.exists(`shortcuts:preset.${p.id}.description`), p.id).toBe(true)
    for (const k of [" ", "escape", "enter", "backspace", "delete", "insert", "pageup", "pagedown"]) {
      expect(keyCap(k), k).not.toContain("shortcuts:")
    }
  })

  it("a shortcut's slots in the catalog are exactly its keys", () => {
    const shortcuts = en.shortcuts as unknown as Record<string, Record<string, { slots?: Record<string, string> }>>
    for (const s of ALL_SHORTCUTS) {
      const slots = shortcuts[s.scope]?.[s.id]?.slots
      expect(Object.keys(slots ?? {}).sort(), `${s.scope}.${s.id}`).toEqual(s.slots.length > 1 ? s.slots.map((_, i) => String(i)) : [])
    }
  })

  it("names every named key in English as its cap has always read", () => {
    const caps: Record<string, string> = {
      " ": "Space",
      escape: "Esc",
      enter: "Enter",
      backspace: "Backspace",
      delete: "Del",
      insert: "Ins",
      pageup: "Page Up",
      pagedown: "Page Down",
    }
    for (const [k, cap] of Object.entries(caps)) expect(keyCap(k), k).toBe(cap)
  })

  it("reads the same English as it always did", () => {
    expect(shortcutName(shortcutOf("player.frame"))).toBe("Previous / next frame")
    expect(slotName(shortcutOf("player.frame"), 1)).toBe("Next frame")
    expect(slotName(shortcutOf("player.shuttle"), 0)).toBe("Backwards")
    expect(shortcutOf("pack.invert").label).toBe("Invert the pack: what's in goes out, what's out comes in")
    expect(SHORTCUT_GROUPS.map((g) => g.title)).toEqual([
      "Everywhere",
      "Player",
      "Review faces",
      "Cast",
      "A character's faces",
      "A character's shots",
      "Scenepack",
    ])
    expect(SHORTCUT_GROUPS[3].description).toBe("On the Cast page. Ctrl-click selects a character too, Shift-click every one between.")
    expect(PRESETS.map((p) => p.label)).toEqual(["AutoScene", "Premiere Pro", "DaVinci Resolve"])
    expect(PRESETS[1].description).toBe("Export with Ctrl M, zoom with = − \\, clear in and out with Ctrl Shift X.")
    expect(PRESETS[2].description).toBe("Zoom with Ctrl = and Ctrl −.")
    expect(["Space", "Esc", "Enter", "Page Down", "←", "−", "Home", "K"].join()).toBe(
      [" ", "escape", "enter", "pagedown", "arrowleft", "-", "home", "k"].map(keyCap).join(),
    )
  })

  it("speaks the language in use at the moment it is asked, not the one it was made in", async () => {
    const frame = shortcutOf("player.frame")
    await inLanguage("ro", {
      shortcuts: {
        player: { frame: { name: "Cadrul anterior / următor", slots: { "0": "Cadrul anterior", "1": "Cadrul următor" } } },
        group: { player: { title: "Player-ul" } },
        key: { esc: "Ieșire" },
      },
    })
    expect(shortcutName(frame)).toBe("Cadrul anterior / următor")
    expect(frame.label).toBe("Cadrul anterior / următor")
    expect(slotName(frame, 1)).toBe("Cadrul următor")
    expect(SHORTCUT_GROUPS[1].title).toBe("Player-ul")
    expect(keyCap("escape")).toBe("Ieșire")
    // A name the language lacks is the English one.
    expect(shortcutName(shortcutOf("player.mute"))).toBe("Mute")
  })
})

describe("Shortcut.label", () => {
  // The getter stays only for the files that still read a shortcut's `.label` (G6's player dialog);
  // when this fails, no one reads it any more: delete the getter in `shortcuts.ts` and this test.
  it("is still read by a component, or it goes", () => {
    const files = import.meta.glob("../**/*.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>
    const readers = Object.entries(files)
      .filter(([f, src]) => !f.endsWith(".test.tsx") && /from "@\/lib\/shortcuts"/.test(src) && /\bs\.label\b/.test(src))
      .map(([f]) => f)
    expect(readers.length, "readers of Shortcut.label").toBeGreaterThan(0)
  })
})
