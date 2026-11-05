// Where the UI's words live. English is the source and ships in the main
// bundle, so the first paint never waits; another language loads on demand,
// one lazy module per namespace file (`./ro/cast.json` …).
//
// Every namespace is registered here once, empty until its sweep group fills
// `en/<ns>.ts`, so the groups never edit this file (docs/DESIGN.md, Copy).
// `backend` is what Rust says: generated from its `msg!`s (`cargo xtask i18n`).
import backend from "./en/backend.json"
import cast from "./en/cast"
import common from "./en/common"
import exportNs from "./en/export"
import library from "./en/library"
import media from "./en/media"
import pack from "./en/pack"
import player from "./en/player"
import plugins from "./en/plugins"
import project from "./en/project"
import settings from "./en/settings"
import shell from "./en/shell"
import shortcuts from "./en/shortcuts"
import ui from "./en/ui"

export const en = {
  common,
  shell,
  ui,
  shortcuts,
  settings,
  cast,
  project,
  library,
  media,
  player,
  pack,
  export: exportNs,
  plugins,
  backend,
} as const

export type Namespace = keyof typeof en
export const NAMESPACES = Object.keys(en) as Namespace[]

type Bundle = Record<string, unknown>
type Loaders = Record<string, () => Promise<{ default: Bundle }>>

// One glob per language: a pattern must be a literal. `meta.json` holds the
// translators' states and hashes (docs/DESIGN.md, Copy), not words.
const LOADERS: Record<string, Loaders> = {
  ro: import.meta.glob<{ default: Bundle }>(["./ro/*.json", "!./ro/meta.json"]),
}

/** A language's namespaces, `{ cast: {…}, … }`; empty when it has none yet. */
export async function loadLanguage(lng: string): Promise<Record<string, Bundle>> {
  const entries = Object.entries(LOADERS[lng] ?? {})
  const loaded = await Promise.all(
    entries.map(async ([path, load]) => [path.replace(/^.*\/(.+)\.json$/, "$1"), (await load()).default] as const),
  )
  return Object.fromEntries(loaded)
}
