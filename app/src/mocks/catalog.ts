/**
 * AutoScene's plugin catalog for the browser backend, shaped like
 * `plugins/catalog.rs`: an index of three reviewed plugins (Poster Maker,
 * new; Hello Export 1.1.0, an update that asks for the network and
 * discord.com; Discord Notify 1.0.1), checked an hour ago. What each offers
 * is worked out against the mock's installed plugins when it is shown.
 *
 * `?mockCatalog=` shows the other states: `expired` (the index ran out and
 * the last check failed), `offline` (the last check failed, the index held
 * still lives), `unset` (this build has no catalog keys), `newer` (a later
 * AutoScene checked it), `withdrawn` (the index withdraws
 * `dev.example.leaky`).
 */
import type { CatalogPlugin, CatalogView } from "@/bindings"
import { mockPlugin } from "./plugins"

type MockCatalog = "living" | "expired" | "offline" | "unset" | "newer" | "withdrawn"

const HOUR = 3_600_000
const DAY = 24 * HOUR
const UNREACHABLE = "AutoScene couldn't reach its plugin catalog."

/** A plain rounded square in `fill`, as the catalog's SVG icons arrive (base64). */
const icon = (fill: string) =>
  `data:image/svg+xml;base64,${btoa(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="20" rx="6" fill="${fill}"/><path d="M8 12h8" stroke="#0b0b0c" stroke-width="2"/></svg>`)}`

/** An entry as `index.json` lists it, with what the view works out. */
type Entry = Omit<CatalogPlugin, "installed" | "update" | "addsPermissions" | "addsHosts" | "runsHere" | "needsNewerApp">

function entry(o: Pick<Entry, "id" | "name" | "summary" | "version" | "permissions" | "hosts"> & Partial<Entry>): Entry {
  return {
    author: "AutoScene",
    license: "MIT",
    platforms: [],
    gpu: false,
    uvScript: true,
    runtime: "process",
    repository: "https://github.com/tcmmi/AutoScene",
    commit: "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a090",
    path: `examples/plugins/${o.id.split(".").pop()}`,
    icon: null,
    sizeBytes: 48_213,
    approved: Date.now() - 3 * DAY,
    ...o,
  }
}

const ENTRIES: Entry[] = [
  entry({
    id: "dev.autoscene.poster-maker",
    name: "Poster Maker",
    summary: "Makes a poster of each character from their best frames.",
    version: "1.0.0",
    permissions: ["project:read", "exports:write"],
    hosts: [],
    icon: icon("#e0b45c"),
    gpu: true,
  }),
  entry({
    id: "dev.autoscene.hello-export",
    name: "Hello Export",
    summary: "Writes a list of an export's clips beside them, and can post it to Discord.",
    version: "1.1.0",
    permissions: ["exports:read", "exports:write", "network"],
    hosts: ["discord.com"],
    icon: icon("#7aa2f7"),
  }),
  entry({
    id: "dev.autoscene.discord-notify",
    name: "Discord Notify",
    summary: "Posts to a Discord channel when an export is done.",
    version: "1.0.1",
    permissions: ["exports:read", "network"],
    hosts: ["discord.com"],
    sizeBytes: 21_904,
  }),
]

const WITHDRAWN = [{ id: "dev.example.leaky", reason: "It sent export paths to a server." }]

function mode(): MockCatalog {
  const asked = new URLSearchParams(window.location.search).get("mockCatalog")
  const known: MockCatalog[] = ["expired", "offline", "unset", "newer", "withdrawn"]
  return known.find((m) => m === asked) ?? "living"
}

/** When the catalog was last fetched and tried; a check moves both. */
let fetchedAt = Date.now() - HOUR
let checkedAt = fetchedAt
/** The withdrawals already told of (`catalog:changed` says each once). */
let told = false

function newer(a: string, b: string): boolean {
  const parts = (v: string) => v.split(/[.+-]/).slice(0, 3).map(Number)
  const [x, y] = [parts(a), parts(b)]
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i]
  return false
}

function offer(e: Entry): CatalogPlugin {
  const p = mockPlugin(e.id)
  const adds = (wanted: string[], has: string[]) => (p ? wanted.filter((w) => !has.includes(w)) : [])
  return {
    ...e,
    installed: p?.version ?? null,
    update: p != null && newer(e.version, p.version),
    addsPermissions: adds(e.permissions, p?.permissions ?? []),
    addsHosts: adds(e.hosts, p?.networkHosts ?? []),
    runsHere: true,
    needsNewerApp: false,
  }
}

/** `catalog_list`, with `daily` the setting. */
export function catalogList(daily: boolean): CatalogView {
  const m = mode()
  const now = Date.now()
  const living = m !== "expired" && m !== "unset" && m !== "newer"
  const published = m === "expired" ? now - 40 * DAY : now - DAY
  const held = m !== "unset"
  return {
    status: {
      ready: m !== "unset" && m !== "newer",
      daily,
      published: held ? published : null,
      expires: held ? published + 30 * DAY : null,
      fetchedAt: held ? (m === "expired" ? published : fetchedAt) : null,
      checkedAt: held ? checkedAt : null,
      outOfDate: !living,
      error: m === "expired" ? `The catalog ran out on ${new Date(published + 30 * DAY).toISOString().slice(0, 10)}.` : m === "offline" ? UNREACHABLE : null,
      needsNewerApp: m === "newer",
    },
    plugins: living ? ENTRIES.map(offer) : [],
  }
}

/**
 * `catalog_refresh`: a check now, which moves its times (only those of a
 * failure while it can't succeed); gives the view and the withdrawals it
 * newly tells of.
 */
export function catalogRefresh(daily: boolean): { view: CatalogView; withdrawn: string[] } {
  const m = mode()
  const now = Date.now()
  if (m !== "unset" && m !== "newer") checkedAt = now
  if (m === "living" || m === "withdrawn") fetchedAt = now
  const withdrawn = m === "withdrawn" && !told ? WITHDRAWN.map((w) => w.id) : []
  told ||= withdrawn.length > 0
  return { view: catalogList(daily), withdrawn }
}
