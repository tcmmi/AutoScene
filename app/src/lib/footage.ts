import type { ContentKind } from "@/bindings"
import { t } from "@/lib/i18n"

/**
 * What a project's footage can be, with example stills for the picker's
 * hover preview (`public/footage`: cropped and scaled stills from open
 * films and artwork under Creative Commons Attribution licences — credited
 * under them and in full, with the licences, in Settings → About).
 *
 * `label` and `blurb` are read when shown, in the language in use: this
 * list holds keys and pictures, never translated text.
 */
export type Footage = {
  value: ContentKind
  label: string
  /** One line: what it is and what the app does with it. */
  blurb: string
  /** A wide still, then two 16:9 close-ups. */
  stills: [string, string, string]
  /** The pictures' source and licence: names, which stay as they are. */
  credit: string
}

const footage = (value: ContentKind, stills: Footage["stills"], credit: string): Footage => ({
  value,
  get label() {
    return contentLabel(value)
  },
  get blurb() {
    return t(`library:footage.${value}.blurb`)
  },
  stills,
  credit,
})

// i18n-ignore: each credit is the pictures' source and licence: names, which stay as they are
export const FOOTAGE: Footage[] = [
  footage(
    "live",
    ["/footage/live-1.webp", "/footage/live-2.webp", "/footage/live-3.webp"],
    "Tears of Steel · Blender Foundation · CC BY 3.0",
  ),
  footage(
    "animation",
    ["/footage/animation-1.webp", "/footage/animation-2.webp", "/footage/animation-3.webp"],
    "Sprite Fright, Spring · Blender Studio · CC BY 4.0",
  ),
  footage(
    "anime",
    ["/footage/anime-1.webp", "/footage/anime-2.webp", "/footage/anime-3.webp"],
    "Pepper&Carrot artwork · David Revoy · Hero · Blender Studio · CC BY 4.0",
  ),
  footage(
    "mixed",
    ["/footage/mixed-1.webp", "/footage/mixed-2.webp", "/footage/mixed-3.webp"],
    "Tears of Steel · CC BY 3.0 · Sprite Fright · CC BY 4.0 · Blender Foundation",
  ),
]

/** "Live action": a footage's name in the language in use. */
export const contentLabel = (v: ContentKind): string => t(`library:footage.${v}.label`)

/**
 * `CONTENT_LABEL[kind]`, still read by code of other screens: each name is
 * looked up when it is read, so it follows the language. `contentLabel` is
 * the same without the object.
 */
export const CONTENT_LABEL = Object.defineProperties(
  {},
  Object.fromEntries(FOOTAGE.map((f) => [f.value, { get: () => contentLabel(f.value), enumerable: true }])),
) as Record<ContentKind, string>

/** The footage's line: what it is and what the app does with it. */
export const footageBlurb = (v: ContentKind) => FOOTAGE.find((f) => f.value === v)?.blurb
