/**
 * Mock plugins for the browser backend, shaped like `plugins.rs`: three
 * reviewed plugins installed (Hello Export with its banner, Hello page with
 * its pages, and Hello service, whose page's service counts words),
 * packages to install (each .asp picked offers the next: one AutoScene
 * hasn't reviewed and one its author signed, as every real package is for
 * now, then a reviewed one, an update of the installed one, and a
 * WebAssembly plugin that runs in the sandbox), a developer's folder to
 * load, and what each run says when it ends. The jobs are the backend's to
 * run. Under `?mockCatalog=withdrawn` a fourth is installed that AutoScene
 * withdrew (Leaky Export).
 *
 * A page's frame loads from the mock's own origin (`vite.mock-asp.ts`,
 * started with Vite), as the app's asp scheme serves it; its calls are
 * answered here (`pageCall`), as `plugins/calls.rs` answers them.
 */
import type {
  ActionKind,
  CallCode,
  CharacterShot,
  PageEvent,
  PageMount,
  PageReply,
  Permission,
  PluginInfo,
  PluginReview,
  PluginUpdate,
  ReviewCatalog,
  SettingSpec,
  SettingValue,
  Msg,
} from "@/bindings"
import type { HostMethod } from "@/plugin-kit/protocol"

/** A plugin as installed or offered: what the setting decides is worked out when it is shown. */
type MockPlugin = Omit<PluginInfo, "active" | "blockedReason">

const DATA = String.raw`C:\Users\you\AppData\Local\AutoScene`
const NOT_ALLOWED = "Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins."
const notAllowed: Msg = { key: "plugin.notAllowed", text: NOT_ALLOWED }

/** A plain rounded square in `fill`, as a package's SVG icon arrives (base64). */
const icon = (fill: string) =>
  `data:image/svg+xml;base64,${btoa(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="20" rx="6" fill="${fill}"/><circle cx="12" cy="12" r="4" fill="#0b0b0c"/></svg>`)}`

/**
 * Hello Export's banner, as `plugin_banner` gives a package's JPEG: wide art,
 * drawn once when it is first shown.
 */
let helloArt: string | null = null

function drawArt(): string {
  const [w, h] = [1200, 600]
  const canvas = document.createElement("canvas")
  Object.assign(canvas, { width: w, height: h })
  const g = canvas.getContext("2d")
  if (!g) return ""
  const ground = g.createLinearGradient(0, 0, w, h)
  ground.addColorStop(0, "#3b2a10")
  ground.addColorStop(1, "#0b0b0c")
  g.fillStyle = ground
  g.fillRect(0, 0, w, h)
  for (const [x, y, r, a] of [
    [880, 170, 300, 0.55],
    [260, 480, 220, 0.3],
    [640, 320, 110, 0.85],
  ]) {
    const glow = g.createRadialGradient(x, y, 0, x, y, r)
    glow.addColorStop(0, `rgba(224, 180, 92, ${a})`)
    glow.addColorStop(1, "rgba(224, 180, 92, 0)")
    g.fillStyle = glow
    g.fillRect(0, 0, w, h)
  }
  return canvas.toDataURL("image/jpeg", 0.85)
}

function plugin(o: Pick<MockPlugin, "id" | "name" | "version" | "description" | "trust"> & Partial<MockPlugin>): MockPlugin {
  return {
    author: "AutoScene",
    license: "MIT",
    repository: null,
    authorKeyId: null,
    source: "package",
    folder: `${DATA}\\plugins\\${o.id}\\${o.version}+1a2b3c4d`,
    enabled: true,
    broken: null,
    withdrawn: null,
    permissions: [],
    networkHosts: [],
    gpu: false,
    uvScript: true,
    exportActions: [],
    commands: [],
    packActions: [],
    shotFilters: [],
    settings: [],
    events: [],
    runtime: "process",
    sandbox: null,
    protocol: 1,
    pages: [],
    secretsSet: [],
    icon: null,
    bannerHash: null,
    logPath: `${DATA}\\logs\\plugins\\${o.id}.log`,
    serviceLogPath: o.runtime === "service" ? `${DATA}\\logs\\plugins\\${o.id}.service.log` : null,
    warnings: [],
    resourceBytes: 0,
    ...o,
  }
}

const HELLO_SETTINGS: SettingSpec[] = [
  {
    key: "greeting",
    title: "Greeting",
    description: "What the note beside each export starts with.",
    kind: { type: "string", default: "Hello", placeholder: "Hello", maxLength: 80 },
  },
  {
    key: "clips",
    title: "Clips to name",
    description: "How many of the export's clips the note names.",
    kind: { type: "number", default: 3, min: 1, max: 10, step: 1, integer: true },
  },
  {
    key: "pause",
    title: "Wait before writing",
    description: "In seconds.",
    kind: { type: "number", default: 0.5, min: 0, max: null, step: null, integer: false },
  },
  { key: "signed", title: "Sign the note", description: null, kind: { type: "boolean", default: true } },
  {
    key: "style",
    title: "Style",
    description: null,
    kind: {
      type: "select",
      options: [
        { value: "plain", label: "Plain" },
        { value: "friendly", label: "Friendly" },
        { value: "formal", label: "Formal" },
      ],
      default: "plain",
    },
  },
]

const hello = (version: string, more: Partial<MockPlugin> = {}) =>
  plugin({
    id: "dev.autoscene.hello-export",
    name: "Hello Export",
    version,
    trust: "reviewed",
    description: "Writes a short note beside each export. An example of what plugins can do.",
    repository: "https://github.com/tcmmi/autoscene",
    permissions: ["exports:read", "exports:write"],
    icon: icon("#e0b45c"),
    bannerHash: "5d1c0e7a",
    exportActions: [{ id: "hello", title: "Say hello", description: "Writes hello.txt beside the export.", afterExport: false, afterFailed: false }],
    commands: [{ id: "greet", title: "Say hello to the project", description: null, needsProject: true }],
    settings: HELLO_SETTINGS,
    ...more,
  })

/** What each .asp picked in the mock is, in the order they are offered. */
const PACKAGES: { path: string; plugin: MockPlugin }[] = [
  {
    path: String.raw`C:\Plugins\discord-notify-unreviewed.asp`,
    plugin: plugin({
      id: "dev.example.discord-notify",
      name: "Discord Notify",
      version: "0.3.1",
      trust: "unreviewed",
      author: "Example Dev",
      license: null,
      // Not on GitHub: its source opens only after a yes.
      repository: "https://gitlab.com/example-dev/discord-notify",
      description: "Posts each finished export's clip list to a Discord channel.",
      protocol: 2,
      permissions: ["exports:read", "network"],
      networkHosts: ["discord.com"],
      icon: icon("#7c8cf5"),
      exportActions: [{ id: "post", title: "Post to Discord", description: "Posts the clip list to the channel.", afterExport: false, afterFailed: true }],
      events: [
        { id: "analysis-done", title: "Post when an analysis finishes", description: null, on: "analysisFinished", turnedOn: false },
      ],
      settings: [
        {
          key: "webhook",
          title: "Webhook address",
          description: "From the channel's settings in Discord: Integrations, then Webhooks.",
          kind: { type: "secret", placeholder: "https://discord.com/api/webhooks/…" },
        },
        {
          key: "mention",
          title: "Mention",
          description: null,
          kind: {
            type: "select",
            options: [
              { value: "none", label: "Nobody" },
              { value: "here", label: "Everyone online" },
              { value: "everyone", label: "Everyone" },
              { value: "role", label: "The editors' role" },
            ],
            default: "none",
          },
        },
      ],
    }),
  },
  {
    path: String.raw`C:\Plugins\frame-guard.asp`,
    plugin: plugin({
      id: "dev.someone.frame-guard",
      name: "Frame Guard",
      version: "2.0.0",
      trust: "pinned",
      author: "Someone",
      authorKeyId: "4B2D9C1E0F3A7E51",
      description: "Checks each exported clip for dropped or repeated frames.",
      permissions: ["exports:read"],
      gpu: true,
      uvScript: false,
      exportActions: [{ id: "check", title: "Check for dropped frames", description: null, afterExport: false, afterFailed: false }],
    }),
  },
  {
    path: String.raw`C:\Plugins\shot-notes.asp`,
    plugin: plugin({
      id: "dev.autoscene.shot-notes",
      name: "Shot Notes",
      version: "1.2.0",
      trust: "reviewed",
      description: "Lists each clip's episode and timecodes in a text file beside the export.",
      permissions: ["exports:read", "exports:write", "project:read"],
      icon: icon("#5fc58f"),
      exportActions: [{ id: "notes", title: "Write shot notes", description: null, afterExport: false, afterFailed: false }],
      commands: [{ id: "whats-new", title: "What's new in Shot Notes", description: null, needsProject: false }],
    }),
  },
  {
    path: String.raw`C:\Plugins\hello-export-1.1.0.asp`,
    plugin: hello("1.1.0", {
      permissions: ["exports:read", "exports:write", "network"],
      networkHosts: ["hooks.example.com"],
      description: "Writes a short note beside each export, and can send it on. An example of what plugins can do.",
    }),
  },
  {
    path: String.raw`C:\Plugins\clip-list.asp`,
    plugin: plugin({
      id: "dev.autoscene.clip-list",
      name: "Clip List",
      version: "1.0.0",
      trust: "unreviewed",
      description: "Saves a list of an export's clips in its folder, from AutoScene's sandbox. An example plugin in Python.",
      repository: "https://github.com/tcmmi/AutoScene",
      runtime: "wasm",
      sandbox: { memoryMb: 512, minutes: 30 },
      uvScript: false,
      protocol: 2,
      permissions: ["exports:write"],
      icon: icon("#c58f5f"),
      exportActions: [
        { id: "list", title: "Save a clip list", description: "Saves each clip's name, episode and frames in the export's folder.", afterExport: false, afterFailed: false },
      ],
    }),
  },
]

/** The developer's folder "Load a folder…" picks. */
export const MOCK_PLUGIN_FOLDER = String.raw`D:\Plugins\my-plugin`

const folderPlugin = (path: string) =>
  plugin({
    id: "dev.me.my-plugin",
    name: "My Plugin",
    version: "0.1.0",
    trust: "unreviewed",
    author: "You",
    description: "A plugin being written, loaded from its folder.",
    source: "folder",
    folder: path,
    exportActions: [{ id: "try", title: "Try my plugin", description: null, afterExport: false, afterFailed: false }],
    commands: [{ id: "hello", title: "Run my plugin", description: null, needsProject: false }],
  })

/** What a run of each action says when it ends, by plugin and action. */
const MESSAGES = new Map([
  ["dev.autoscene.hello-export hello", "Wrote hello.txt beside the export."],
  ["dev.autoscene.hello-export greet", "Said hello to the project."],
  ["dev.autoscene.shot-notes notes", "Wrote the shot notes for every clip."],
  ["dev.autoscene.shot-notes whats-new", "Shot Notes 1.2 names each clip's episode."],
  ["dev.example.discord-notify post", "Posted the clip list to Discord."],
  ["dev.someone.frame-guard check", "No frame was dropped or repeated."],
  ["dev.autoscene.clip-list list", "Saved the list of 3 clips."],
])

/** The example page-only plugin (examples/plugins/hello-page), which the mock serves. */
const helloPage = plugin({
  id: "dev.autoscene.hello-page",
  name: "Hello page",
  version: "1.0.0",
  trust: "reviewed",
  description: "A page in the rail that keeps a note, and one in each project that shows its episodes and clips. An example plugin.",
  repository: "https://github.com/tcmmi/AutoScene",
  permissions: ["project:read", "exports:read"],
  icon: icon("#5fb4c5"),
  uvScript: false,
  runtime: "none",
  protocol: 2,
  pages: [
    { id: "main", title: "Hello page", description: "Keeps a note, and says hello.", entry: "ui/index.html", scope: "global", rail: true },
    { id: "clips", title: "Clips", description: "The project's episodes and clips.", entry: "ui/clips.html", scope: "project", rail: false },
  ],
})

/**
 * The example service plugin (examples/plugins/hello-service), which the mock
 * serves: its service counts words. Unlike the example, which has an icon,
 * it has no icon and no banner here, so the plain look shows too.
 */
const helloService = plugin({
  id: "dev.autoscene.hello-service",
  name: "Hello service",
  version: "1.0.0",
  trust: "reviewed",
  description: "A page in the rail whose service counts the words you give it, and says so. An example plugin.",
  repository: "https://github.com/tcmmi/AutoScene",
  runtime: "service",
  protocol: 2,
  pages: [{ id: "main", title: "Word count", description: "Counts words in its service.", entry: "ui/index.html", scope: "global", rail: true }],
})

/**
 * A plugin the catalog withdrew, installed only under `?mockCatalog=withdrawn`:
 * it has a global page with no files, which the banner stands in for.
 */
export const LEAKY_ID = "dev.example.leaky"
export const LEAKY_REASON = "It sent export paths to a server."

const leaky = plugin({
  id: LEAKY_ID,
  name: "Leaky Export",
  version: "1.0.2",
  trust: "reviewed",
  author: "Example Dev",
  description: "Writes a list of each export's clips beside them, and posts it to a server.",
  repository: "https://github.com/example-dev/leaky-export",
  permissions: ["exports:read", "network"],
  networkHosts: ["hooks.example.com"],
  icon: icon("#d9534f"),
  protocol: 2,
  runtime: "none",
  uvScript: false,
  exportActions: [{ id: "list", title: "List the clips", description: null, afterExport: false, afterFailed: false }],
  pages: [{ id: "main", title: "Leaky Export", description: "Lists the clips of an export.", entry: "ui/index.html", scope: "global", rail: true }],
  withdrawn: LEAKY_REASON,
})

/**
 * A plugin whose program needs a model's weights, offered by the catalog:
 * its plugin.json names 312 MiB of files, which its first run gets from
 * huggingface.co. Here they arrive with the install, so its settings can
 * say what they take.
 */
export const BIG_ID = "dev.example.big-model"
const LARGE_FILES = new Map([[`${BIG_ID}@1.0.0`, { bytes: 327_155_712, hosts: ["huggingface.co"] }]])

/** The large files a version's plugin.json names: the same object for the same version, as the same hashes. */
const largeOf = (p: Pick<MockPlugin, "id" | "version">) => LARGE_FILES.get(`${p.id}@${p.version}`)

/**
 * The example shot-filter plugin (examples/plugins/bright-shots has the
 * real one's shape): its filter "Speaks" keeps the shots where the
 * character talks. Its settings let the mock show a verdict that changes
 * with a setting (Strict), and a check that fails (Fail the check).
 */
const speechFilter = plugin({
  id: "dev.example.speech-filter",
  name: "Speech Filter",
  version: "1.0.0",
  trust: "reviewed",
  author: "Example Dev",
  description: "Keeps the shots of a scenepack where the character talks. An example plugin.",
  permissions: ["project:read"],
  icon: icon("#d9776b"),
  protocol: 2,
  settings: [
    {
      key: "strict",
      title: "Strict",
      description: "Leaves out the shots where it is not sure they talk.",
      kind: { type: "boolean", default: false },
    },
    {
      key: "fail",
      title: "Fail the check",
      description: "Only for trying out what a check that fails looks like.",
      kind: { type: "boolean", default: false },
    },
  ],
})

/**
 * The example pack-action plugin: its action saves a scenepack as a Final
 * Cut Pro timeline, before anything is rendered. The mock's save is a
 * prompt, and its run takes a moment (see `pack_action_run`).
 */
const fcpxml = plugin({
  id: "dev.example.fcpxml",
  name: "FCPXML",
  version: "1.0.0",
  trust: "reviewed",
  author: "Example Dev",
  description: "Saves a character's scenepack as a Final Cut Pro timeline, before anything is rendered. An example plugin.",
  permissions: ["project:read"],
  icon: icon("#6b8fd9"),
  protocol: 2,
  packActions: [{ id: "fcpxml", title: "Save as FCPXML…", file: { name: "{character}.fcpxml", extensions: ["fcpxml"] } }],
})

const installed = new Map<string, MockPlugin>([
  [hello("1.0.0").id, hello("1.0.0")],
  [speechFilter.id, speechFilter],
  [fcpxml.id, fcpxml],
  [helloPage.id, helloPage],
  [helloService.id, helloService],
  ...(typeof window !== "undefined" && new URLSearchParams(window.location.search).get("mockCatalog") === "withdrawn"
    ? ([[leaky.id, leaky]] as const)
    : []),
])
const tickets = new Map<string, MockPlugin>()
/** The settings the person changed, by plugin. */
const changed = new Map<string, Map<string, SettingValue>>()
let offered = 0

/** Whether AutoScene reviewed `p`: a loaded folder never is. */
export function reviewed(p: MockPlugin | undefined): boolean {
  return p?.source === "package" && p.trust === "reviewed"
}

function info(p: MockPlugin, allow: boolean): PluginInfo {
  // A withdrawn plugin never runs, and says why itself (`withdrawn`).
  const blocked = !reviewed(p) && !allow && p.withdrawn == null
  return {
    ...p,
    active: p.enabled && p.broken == null && p.withdrawn == null && !blocked,
    blockedReason: blocked && p.broken == null ? notAllowed : null,
  }
}

function get(id: string): MockPlugin {
  const p = installed.get(id)
  if (!p) throw { code: "not_found", message: `${id} isn't installed.` }
  return p
}

export function mockPlugin(id: string): MockPlugin | undefined {
  return installed.get(id)
}

/** The banner of the installed plugin `id`, as a `data:` URL: none without one. */
export function pluginBanner(id: string): string | null {
  if (get(id).bannerHash == null) return null
  helloArt ??= drawArt()
  return helloArt
}

export function pluginsList(allow: boolean): PluginInfo[] {
  return [...installed.values()].sort((a, b) => a.id.localeCompare(b.id)).map((p) => info(p, allow))
}

/** A plugin's shot filter, as the mock plugins have them. */
type MockFilter = {
  pluginId: string
  id: string
  title: string
  description: string | null
  /** What a shot it leaves out says. */
  reason: string
  /** The settings its verdicts depend on: a new value asks for new verdicts. */
  settings: string[]
  keeps: (shot: CharacterShot, values: Record<string, SettingValue>) => boolean
}

const FILTERS: MockFilter[] = [
  {
    pluginId: speechFilter.id,
    id: "speaks",
    title: "Speaks",
    description: "Keeps the shots where they talk.",
    reason: "No speech",
    settings: ["strict"],
    // About three shots in five talk; strict, one in three.
    keeps: (shot, values) => ((shot.start + shot.trackId) % 3 === 0) || (values.strict !== true && (shot.start + shot.trackId) % 3 === 2),
  },
]

/** A shot filter of a pack as the mock backend works it: whose it is, what it judges by, and its verdict. */
export type PackFilterHost = {
  /** `<pluginId>/<filterId>`. */
  key: string
  title: string
  description: string | null
  reason: string
  /** Its plugin's name (its id when none is installed). */
  plugin: string
  /** Its plugin may run now. */
  active: boolean
  /** What its verdicts were made with now (its settings that count). */
  basis: string
  keeps: (shot: CharacterShot) => boolean
  /** Its plugin is set to fail its checks. */
  fails: boolean
}

/**
 * The filters a pack can show, as `pack_filters` lists them: every filter
 * of a plugin that may run, then those `on` turns on that no such plugin has.
 */
export function packFilterHosts(allow: boolean, on: readonly string[]): PackFilterHost[] {
  const out: PackFilterHost[] = []
  for (const f of FILTERS) {
    const p = installed.get(f.pluginId)
    const key = `${f.pluginId}/${f.id}`
    const active = p != null && info(p, allow).active
    if (!p || (!active && !on.includes(key))) continue
    const values = active ? pluginValues(p.id) : {}
    out.push({
      key,
      title: f.title,
      description: f.description,
      reason: f.reason,
      plugin: p.name,
      active,
      basis: JSON.stringify(f.settings.map((k) => values[k] ?? null)),
      keeps: (shot) => f.keeps(shot, values),
      fails: values.fail === true,
    })
  }
  for (const key of on) {
    if (out.some((f) => f.key === key)) continue
    const [plugin, id = ""] = key.split("/")
    out.push({ key, title: id, description: null, reason: "", plugin, active: false, basis: "", keeps: () => true, fails: false })
  }
  return out
}

/** The next package the file dialog offers. */
export function nextPluginPackage(): string {
  return PACKAGES[offered++ % PACKAGES.length].path
}

let openedTaken = false

/**
 * The packages AutoScene was started with (`plugins_take_opened`): none, or
 * with `?mockOpened=1` the first package the dialog offers, taken once.
 */
export function takeOpenedPackages(): string[] {
  if (openedTaken || new URLSearchParams(window.location.search).get("mockOpened") !== "1") return []
  openedTaken = true
  return [PACKAGES[0].path]
}

function review(p: MockPlugin, allow: boolean, catalog: ReviewCatalog | null = null): PluginReview {
  const previous = installed.get(p.id)
  const ticketId = crypto.randomUUID()
  // As installed, it keeps what the version before had turned on.
  const next = previous
    ? {
        ...p,
        enabled: previous.enabled,
        exportActions: p.exportActions.map((a) => ({
          ...a,
          afterExport: previous.exportActions.some((b) => b.id === a.id && b.afterExport),
        })),
        events: p.events.map((e) => ({ ...e, turnedOn: previous.events.some((b) => b.id === e.id && b.turnedOn) })),
      }
    : p
  tickets.set(ticketId, next)
  const update: PluginUpdate | null = previous
    ? {
        fromVersion: previous.version,
        trustBefore: previous.source === "folder" ? "unreviewed" : previous.trust,
        sourceBefore: previous.source,
        addedPermissions: p.permissions.filter((x: Permission) => !previous.permissions.includes(x)),
        addedHosts: p.networkHosts.filter((h) => !previous.networkHosts.includes(h)),
        addedPages: p.pages.filter((x) => !previous.pages.some((y) => y.id === x.id)).map((x) => x.id),
        addedShotFilters: p.shotFilters.filter((x) => !previous.shotFilters.some((y) => y.id === x.id)).map((x) => x.id),
        addedPackActions: p.packActions.filter((x) => !previous.packActions.some((y) => y.id === x.id)).map((x) => x.id),
        startsProgram: previous.runtime === "none" && p.runtime !== "none",
        becomesService: previous.runtime !== "service" && p.runtime === "service",
        // Hosts its files come from that the installed version's plugin.json doesn't name, downloaded or not.
        addedResourceHosts: (largeOf(p)?.hosts ?? []).filter((h) => !(largeOf(previous)?.hosts ?? []).includes(h)),
        leavesSandbox: previous.runtime === "wasm" && (p.runtime === "process" || p.runtime === "service"),
        raisedLimits:
          previous.sandbox != null &&
          p.sandbox != null &&
          (p.sandbox.memoryMb > previous.sandbox.memoryMb || p.sandbox.minutes > previous.sandbox.minutes),
      }
    : null
  const needsUnreviewed = !reviewed(next)
  // As on a computer where no uv script has run yet: AutoScene's uv comes with the first.
  const uv = { uvMissing: next.uvScript, uvBytes: 15236317 }
  // What is still to get: all the files it names, unless the installed version names the same ones and has them.
  const large = largeOf(p)
  const haveThem = previous != null && largeOf(previous) === large && (previous.resourceBytes ?? 0) > 0
  const files = {
    resourceBytes: haveThem ? 0 : (large?.bytes ?? 0),
    resourceHosts: large?.hosts ?? [],
  }
  return {
    ticketId,
    plugin: info(next, allow),
    update,
    needsUnreviewed,
    allowed: !needsUnreviewed || allow,
    ...uv,
    ...files,
    catalog,
    componentWarnings: [],
  }
}

/** What AutoScene's catalog says of a plugin it offers, as `catalog_get` reads it from the package. */
export type CatalogPackage = {
  id: string
  name: string
  version: string
  summary: string
  author: string
  license: string | null
  permissions: string[]
  hosts: string[]
  gpu: boolean
  uvScript: boolean
  icon: string | null
  repository: string
}

/**
 * The review of a package got from the catalog (`catalog_get`): the plugin
 * as the catalog lists it, reviewed, with where it was built from. Hello
 * Export keeps its actions and settings.
 */
export function inspectCatalogPackage(o: CatalogPackage, built: ReviewCatalog, allow: boolean): PluginReview {
  const listed = {
    name: o.name,
    version: o.version,
    trust: "reviewed" as const,
    description: o.summary,
    author: o.author,
    license: o.license,
    repository: o.repository,
    permissions: o.permissions as Permission[],
    networkHosts: o.hosts,
    gpu: o.gpu,
    uvScript: o.uvScript,
    icon: o.icon,
  }
  const p = o.id === "dev.autoscene.hello-export" ? hello(o.version, listed) : plugin({ id: o.id, ...listed })
  return review(p, allow, built)
}

export function inspectPackage(path: string, allow: boolean): PluginReview {
  const offer = PACKAGES.find((x) => x.path === path)
  if (!offer) throw { code: "invalid", message: "This isn't an AutoScene plugin package." }
  return review(offer.plugin, allow)
}

export function inspectFolder(path: string, allow: boolean): PluginReview {
  if (!allow) throw { code: "invalid", message: NOT_ALLOWED }
  return review(folderPlugin(path), allow)
}

/** How long a sandboxed plugin compiles here; `?mockCompile=<ms>` says otherwise. */
const compileMs = () => Number(new URLSearchParams(window.location.search).get("mockCompile") ?? 1500)

/** What ends each compile under way, by ticket. */
const compiling = new Map<string, () => void>()

/** The compile `plugin_install` waits for, past the yes: only a sandboxed plugin has one. */
export async function compilePlugin(ticketId: string, started: () => void) {
  if (!tickets.get(ticketId)?.sandbox) return
  started()
  await new Promise<void>((done, stop) => {
    const timer = window.setTimeout(done, compileMs())
    compiling.set(ticketId, () => {
      window.clearTimeout(timer)
      stop({ code: "cancelled", message: "It wasn't installed." })
    })
  }).finally(() => compiling.delete(ticketId))
}

/** `plugin_compile_cancel`. */
export function stopCompile(ticketId: string) {
  compiling.get(ticketId)?.()
}

export function installPlugin(ticketId: string, allow: boolean, accept: boolean): PluginInfo {
  const p = tickets.get(ticketId)
  if (!p) throw { code: "not_found", message: "This review was closed. Open the plugin again." }
  tickets.delete(ticketId)
  if (!reviewed(p) && !allow) throw { code: "invalid", message: NOT_ALLOWED }
  if (!reviewed(p) && !accept) throw { code: "invalid", message: "Confirm that you want to install it." }
  const kept = { ...p, resourceBytes: largeOf(p)?.bytes ?? p.resourceBytes }
  installed.set(p.id, kept)
  return info(kept, allow)
}

export function discardTicket(ticketId: string) {
  tickets.delete(ticketId)
}

export function uninstallPlugin(id: string) {
  get(id)
  installed.delete(id)
  changed.delete(id)
}

export function setPluginEnabled(id: string, on: boolean, allow: boolean): PluginInfo {
  const p = { ...get(id), enabled: on }
  installed.set(id, p)
  return info(p, allow)
}

export function setAfterExport(id: string, action: string, on: boolean, allow: boolean): PluginInfo {
  const old = get(id)
  if (!old.exportActions.some((a) => a.id === action)) throw { code: "invalid", message: `The plugin has no export action ${action}.` }
  const p = { ...old, exportActions: old.exportActions.map((a) => (a.id === action ? { ...a, afterExport: on } : a)) }
  installed.set(id, p)
  return info(p, allow)
}

export function setEvent(id: string, event: string, on: boolean, allow: boolean): PluginInfo {
  const old = get(id)
  if (!old.events.some((e) => e.id === event)) throw { code: "invalid", message: `The plugin has no event ${event}.` }
  const p = { ...old, events: old.events.map((e) => (e.id === event ? { ...e, turnedOn: on } : e)) }
  installed.set(id, p)
  return info(p, allow)
}

function defaultOf(s: SettingSpec): SettingValue | undefined {
  return s.kind.type === "secret" ? undefined : (s.kind.default ?? undefined)
}

/** Each setting's value: the person's, else its default (none without one). */
export function pluginValues(id: string): Record<string, SettingValue> {
  const own = changed.get(id)
  const values: Record<string, SettingValue> = {}
  for (const s of get(id).settings) {
    const v = own?.get(s.key) ?? defaultOf(s)
    if (v !== undefined) values[s.key] = v
  }
  return values
}

/** Why `v` can't be the value of `s`, as `check_value` words it. */
function problem(s: SettingSpec, v: SettingValue): string | null {
  const k = s.kind
  switch (k.type) {
    case "string":
      return typeof v !== "string" ? "must be text" : v.length > k.maxLength ? `can be at most ${k.maxLength} characters` : null
    case "boolean":
      return typeof v !== "boolean" ? "must be on or off" : null
    case "select":
      return typeof v === "string" && k.options.some((o) => o.value === v) ? null : "must be one of its choices"
    case "number":
      if (typeof v !== "number" || !Number.isFinite(v)) return "must be a number"
      if (k.integer && !Number.isInteger(v)) return "must be a whole number"
      if (k.min != null && v < k.min) return `must be at least ${k.min}`
      if (k.max != null && v > k.max) return `must be at most ${k.max}`
      return null
    case "secret":
      return "is kept in your system's keychain, never as a value"
  }
}

export function setPluginValues(id: string, values: Record<string, SettingValue>): Record<string, SettingValue> {
  const specs = get(id).settings
  const own = new Map(changed.get(id))
  for (const [key, v] of Object.entries(values)) {
    const s = specs.find((x) => x.key === key)
    if (!s) continue
    if (s.kind.type === "secret") throw { code: "invalid", message: "A secret is set with its own button, never as a value." }
    const why = problem(s, v)
    if (why) throw { code: "invalid", message: `${s.title} ${why}.` }
    if (v === defaultOf(s)) own.delete(key)
    else own.set(key, v)
  }
  changed.set(id, own)
  return pluginValues(id)
}

/** The secret setting `key` of `id`, as `secret_set` checks it. The mock keeps no value, only that one is set. */
export function setPluginSecret(id: string, key: string, value: string, allow: boolean): PluginInfo {
  const old = get(id)
  if (!old.settings.some((s) => s.key === key && s.kind.type === "secret")) {
    throw { code: "invalid", message: `The plugin has no secret setting ${key}.` }
  }
  if (value === "") throw { code: "invalid", message: "A secret can't be empty. Remove it instead." }
  if (value.length > 1024) throw { code: "invalid", message: "A secret can be at most 1024 characters." }
  const p = { ...old, secretsSet: old.secretsSet.includes(key) ? old.secretsSet : [...old.secretsSet, key] }
  installed.set(id, p)
  return info(p, allow)
}

export function clearPluginSecret(id: string, key: string, allow: boolean): PluginInfo {
  const old = get(id)
  const p = { ...old, secretsSet: old.secretsSet.filter((k) => k !== key) }
  installed.set(id, p)
  return info(p, allow)
}

/** Where the mock's pages load from: an origin of their own, as the app's asp scheme is. */
export const MOCK_PAGE_BASE = "http://localhost:5174"

/** The pages open in the mock, by session: what each may use is its plugin's as it opened. */
const sessions = new Map<string, { pluginId: string; pageId: string; projectId: string | null; plugin: MockPlugin }>()

/**
 * The page `pageId` of `id`, opened as `plugin_page_open` opens it: its
 * plugin may run now, and a project page is in a project, a global one in
 * none. Its frame loads it from its session's path.
 */
export function openPage(id: string, pageId: string, projectId: string | null, allow: boolean): PageMount {
  const p = get(id)
  const shown = info(p, allow)
  if (!shown.active) throw { code: "invalid", message: p.broken?.text ?? p.withdrawn ?? shown.blockedReason?.text ?? `${p.name} is turned off.` }
  const page = p.pages.find((x) => x.id === pageId)
  if (!page) throw { code: "invalid", message: `${p.name} has no such page.` }
  if (page.scope === "project" && !projectId) throw { code: "invalid", message: "Open a project to see this page." }
  if (page.scope === "global" && projectId) throw { code: "invalid", message: "This page isn't a project's." }
  // As Rust makes one: 32 lowercase hex digits.
  const session = crypto.randomUUID().replaceAll("-", "")
  sessions.set(session, { pluginId: id, pageId, projectId: page.scope === "project" ? projectId : null, plugin: p })
  return {
    session,
    src: `${MOCK_PAGE_BASE}/${session}/${page.entry}`,
    plugin: {
      id: p.id,
      name: p.name,
      version: p.version,
      trust: p.source === "folder" ? "unreviewed" : p.trust,
      source: p.source,
      icon: p.icon,
    },
    page,
    projectId: page.scope === "project" ? projectId : null,
    permissions: p.permissions,
    hosts: p.permissions.includes("network") ? p.networkHosts : [],
    service: p.runtime === "service",
    protocol: p.protocol,
  }
}

/** Closes the page `session`; one closed already is no error. */
export function closePage(session: string) {
  sessions.delete(session)
  shown.delete(session)
  copies.delete(session)
}

/** Closes the pages of the plugins `of` matches: their sessions. */
export function pagesOf(of: (pluginId: string) => boolean): string[] {
  const closing = [...sessions].filter(([, s]) => of(s.pluginId)).map(([session]) => session)
  closing.forEach(closePage)
  return closing
}

/**
 * Tells the mock's page server (`vite.mock-asp.ts`, through Vite's own
 * origin) what it serves: a session's plugin, or a file a page was given.
 * Outside Vite (tests) there is none, and nothing is served.
 */
export async function tellPageServer(said: { session: string; pluginId: string } | { session: string; token: string; fixture: string }) {
  const body = JSON.stringify(said)
  await fetch("/__mock-asp", { method: "POST", headers: { "content-type": "application/json" }, body }).catch(() => undefined)
}

/** What a page's calls read of the mock backend: its projects, with their episodes and exports. */
export type PagePlaces = {
  project(id: string): { id: string; title: string; episodes: { id: string; name: string; frames: number | null; fps: string | null; missing: boolean }[] } | null
  exports(id: string): {
    id: string
    label: string | null
    preset: string
    createdAt: number
    merged: boolean
    clips: { name: string; status: string; mediaId: string; episode: string; start: number; end: number; fps: string | null }[]
  }[]
  /** The fixture every file a page is given plays (its name in fixtures/out). */
  fixture: string
  /** Tells the open page `session` what became of it, as `plugin-page:event` does. */
  tell(session: string, event: PageEvent): void
  /**
   * Queues the run a page started as a job, as `queue_run` does with `Trigger::Page`: its id, `null` when the same
   * run (with the same input) waits already. Throws `{ code, message }` when it can't run.
   */
  run(pluginId: string, kind: ActionKind, action: string, projectId: string | null, exportId: string | null, input: unknown): string | null
}

/** How many words Hello service counted since the mock started, as its service counts them since it started. */
let counted = 0

/** Hello service's service: `count` answers how many words its text has, and tells each of its open pages. */
function helloServiceRequest(s: PageSession, params: Record<string, unknown>, places: PagePlaces): PageReply {
  if (s.pluginId !== helloService.id) return refused("failed", `${s.plugin.name}'s service isn't in the mock.`)
  if (params.method !== "count") return refused("failed", `Hello service has no method ${String(params.method)}.`)
  const text = (params.params as { text?: unknown } | null)?.text
  const words = typeof text === "string" ? text.split(/\s+/).filter(Boolean).length : 0
  counted += words
  for (const [session, open] of sessions) {
    if (open.pluginId === s.pluginId) places.tell(session, { type: "service", name: "counted", data: { words, total: counted } })
  }
  return ok({ words })
}

/** What each plugin's pages keep (`state.*`). */
const pageState = new Map<string, Map<string, unknown>>()
/** When each page's toasts were shown, and its copies made, in the last minute: `calls.rs` limits both. */
const shown = new Map<string, number[]>()
const copies = new Map<string, number[]>()

type PageSession = { session: string; pluginId: string; projectId: string | null; plugin: MockPlugin }
type PageHandler = (s: PageSession, params: Record<string, unknown>, places: PagePlaces) => PageReply | Promise<PageReply>

const ok = (result: unknown = null): PageReply => ({ kind: "ok", result })
const refused = (code: CallCode, message: string): PageReply => ({ kind: "err", error: { code, message } })

/** What `method` needs that `p` doesn't hold, as `HostMethod::needs` and `check` say it; null when it holds it. */
function missing(method: HostMethod, p: MockPlugin): string | null {
  const has = (perm: Permission) => p.permissions.includes(perm)
  switch (method) {
    case "secrets.get":
      return p.settings.some((s) => s.kind.type === "secret") ? null : `${method} needs a secret setting, and this plugin has none.`
    case "project.get":
      return has("project:read") ? null : `${method} needs the project:read permission.`
    case "exports.list":
    case "exports.get":
      return has("exports:read") ? null : `${method} needs the exports:read permission.`
    case "files.resolve":
      return has("exports:read") || has("project:read") ? null : `${method} needs the exports:read or the project:read permission.`
    case "net.fetch":
      return has("network") ? null : `${method} needs the network permission.`
    case "service.request":
      return p.runtime === "service" ? null : `${method} needs a plugin that runs a service.`
    case "actions.run":
      return p.runtime === "wasm" || ((p.runtime === "process" || p.runtime === "service") && p.protocol >= 2)
        ? null
        : `${method} needs a plugin whose program speaks protocol 2.`
    default:
      return null
  }
}

/** The project a page reads: the one it is open in, named or not. */
function projectOf(s: PageSession, params: Record<string, unknown>): string | PageReply {
  const asked = typeof params.projectId === "string" ? params.projectId : null
  if (s.projectId && (!asked || asked === s.projectId)) return s.projectId
  if (!s.projectId && !asked) return refused("invalid", "This page isn't open in a project.")
  return refused("denied", "A page reads only the project it is open in.")
}

/** Whether `path` stays inside the page, as `check_page_path` has it. */
function insidePage(path: string): boolean {
  const parts = path.split("/")
  return (
    path.length <= 512 &&
    parts.every(
      (p, i) =>
        /^(?:[A-Za-z0-9._~!&'()*+,;=:@-]|%[0-9A-Fa-f]{2})*$/.test(p) && p !== "." && p !== ".." && (p !== "" || i === parts.length - 1),
    )
  )
}

/** The kinds of file a page never saves, as `RUNNABLE` in `calls.rs` lists them. */
const RUNNABLE = new Set(
  (
    "exe com scr pif cpl dll ocx sys drv bat cmd msi msix msixbundle msp mst appx appxbundle appinstaller application appref-ms " +
    "lnk url website scf hta chm hlp js jse vbs vbe vb wsf wsh wsc ws sct ps1 ps1xml ps2 psc1 psd1 psm1 msh msh1 msh2 mshxml reg " +
    "inf ins isp gadget diagcab msc xll xbap vsix settingcontent-ms library-ms search-ms searchconnector-ms iso img vhd vhdx py " +
    "pyw pyz pyzw app command tool terminal pkg mpkg dmg webloc inetloc fileloc scpt scptd applescript workflow action prefpane " +
    "kext osax desktop sh run bin appimage deb rpm flatpakref snap so jar jnlp"
  ).split(" "),
)

/** Whether the extension `ext`, or a file called `name` (Windows drops the dots and spaces it ends with), runs as it opens. */
const runnable = (ext: string) => RUNNABLE.has(ext.toLowerCase())
export function runnableName(name: string): boolean {
  const trimmed = name.replace(/[. ]+$/, "")
  const dot = trimmed.lastIndexOf(".")
  return dot >= 0 && runnable(trimmed.slice(dot + 1))
}

/** Each host method, answered as `calls.rs` answers a page: one missing here fails `npm run typecheck`. */
export const pluginPageCall: Record<HostMethod, PageHandler> = {
  "settings.get": (s) => ok(pluginValues(s.pluginId)),
  "secrets.get": (s, params) => {
    const key = String(params.key ?? "")
    if (!s.plugin.settings.some((x) => x.key === key && x.kind.type === "secret")) {
      return refused("denied", `secrets.get reads only the plugin's secret settings, and ${key} isn't one.`)
    }
    return ok({ value: s.plugin.secretsSet.includes(key) ? "mock-secret" : null })
  },
  "project.get": (s, params, places) => {
    const id = projectOf(s, params)
    if (typeof id !== "string") return id
    const project = places.project(id)
    return project ? ok(project) : refused("not_found", "That project doesn't exist.")
  },
  "exports.list": (s, params, places) => {
    const id = projectOf(s, params)
    if (typeof id !== "string") return id
    const summaries = places.exports(id).map(({ clips, ...e }) => {
      const done = clips.filter((c) => c.status === "done").length
      return { ...e, clips: clips.length, done, status: done === clips.length ? "done" : "running" }
    })
    return ok(summaries)
  },
  "exports.get": (s, params, places) => {
    const id = projectOf(s, params)
    if (typeof id !== "string") return id
    const found = places.exports(id).find((e) => e.id === params.exportId)
    if (!found) return refused("not_found", "This export no longer exists.")
    return ok({ ...found, clips: found.clips.map((c, index) => ({ index, ...c })) })
  },
  // Every file plays the fixture, from the page's own origin.
  "files.resolve": async (s, params, places) => {
    const permission: Permission = params.kind === "episode" ? "project:read" : "exports:read"
    if (!s.plugin.permissions.includes(permission)) {
      return refused("denied", `files.resolve needs the ${permission} permission for that file.`)
    }
    const id = projectOf(s, params)
    if (typeof id !== "string") return id
    const token = crypto.randomUUID().replaceAll("-", "")
    await tellPageServer({ session: s.session, token, fixture: places.fixture })
    return ok({ url: `${MOCK_PAGE_BASE}/${s.session}/~f/${token}`, path: null, mime: "video/mp4", size: 2_400_000 })
  },
  "net.fetch": (s, params) => {
    let host = ""
    try {
      host = new URL(String(params.url)).hostname
    } catch {
      return refused("invalid", "net.fetch takes an https:// address.")
    }
    if (!s.plugin.networkHosts.includes(host)) return refused("denied", `net.fetch can't reach ${host}: the plugin doesn't name it.`)
    return ok({ status: 200, url: String(params.url), headers: [["content-type", "application/json"]], body: '{"ok":true}', bodyBase64: null })
  },
  "state.get": (s, params) => ok({ value: pageState.get(s.pluginId)?.get(String(params.key)) ?? null }),
  "state.set": (s, params) => {
    const kept = pageState.get(s.pluginId) ?? new Map<string, unknown>()
    kept.set(String(params.key), params.value)
    pageState.set(s.pluginId, kept)
    return ok()
  },
  "state.delete": (s, params) => {
    pageState.get(s.pluginId)?.delete(String(params.key))
    return ok()
  },
  "state.keys": (s) => ok([...(pageState.get(s.pluginId)?.keys() ?? [])].sort()),
  "service.request": helloServiceRequest,
  // A run in Activity, as one from the export's menu or the palette: an export action on an export of the page's
  // project, else a command in that project.
  "actions.run": (s, params, places) => {
    const exportId = typeof params.exportId === "string" ? params.exportId : null
    if (exportId !== null) {
      if (!s.projectId) return refused("invalid", "This page isn't open in a project.")
      if (!places.exports(s.projectId).some((e) => e.id === exportId)) return refused("not_found", "This export no longer exists.")
    }
    const kind: ActionKind = exportId === null ? "command" : "exportAction"
    try {
      return ok({ jobId: places.run(s.pluginId, kind, String(params.action ?? ""), s.projectId, exportId, params.input ?? null) })
    } catch (e) {
      const { code, message } = e as { code: CallCode; message: string }
      return refused(code, message)
    }
  },
  "ui.toast": (s, params) => {
    const text = String(params.text ?? "").trim()
    if (text.length < 1 || text.length > 200) return refused("invalid", "A toast says 1 to 200 characters.")
    const now = Date.now()
    const last = (shown.get(s.session) ?? []).filter((t) => now - t < 60_000)
    if (last.length >= 2 || now - (last.at(-1) ?? -Infinity) < 2000) return ok({ shown: false })
    shown.set(s.session, [...last, now])
    const description = typeof params.description === "string" ? params.description : null
    const tone = params.tone === "error" ? "error" : "neutral"
    const title: Msg = { key: "plugin.pageToast", args: { name: s.plugin.name, text }, text: `Plugin “${s.plugin.name}”: ${text}` }
    return { kind: "effect", effect: { type: "toast", title, description, tone }, result: { shown: true } }
  },
  "ui.navigate": (_, params) => {
    const path = String(params.path ?? "")
    if (!insidePage(path)) return refused("invalid", "ui.navigate takes a path inside the page, like clips/3.")
    return { kind: "effect", effect: { type: "navigate", path, replace: params.replace === true }, result: null }
  },
  // The host answers `denied` itself while the frame doesn't have the focus.
  "ui.copyText": async (s, params) => {
    const now = Date.now()
    const last = (copies.get(s.session) ?? []).filter((t) => now - t < 60_000)
    if (last.length >= 10) return refused("busy", "A page copies at most 10 times a minute.")
    copies.set(s.session, [...last, now])
    await navigator.clipboard.writeText(String(params.text ?? ""))
    return ok()
  },
  // The browser's confirm stands in for the system's dialog.
  "ui.openExternal": (s, params) => {
    const url = String(params.url ?? "")
    if (url.length > 2000 || !/^https:\/\/[a-z0-9.-]+(?::\d+)?(?:[/?#][^\s\\]*)?$/i.test(url)) {
      return refused("invalid", "ui.openExternal opens only plain https:// links.")
    }
    if (!window.confirm(`Open this link in your browser?\n\nPlugin “${s.plugin.name}” asks to open:\n\n${url}`)) return ok({ opened: false })
    window.open(url, "_blank", "noopener")
    return ok({ opened: true })
  },
  // A download stands in for the save dialog.
  "files.save": (_, params) => {
    const name = String(params.name ?? "")
    if (!/^[A-Za-z0-9._@+-]{1,100}$/.test(name) || name === "." || name === "..") {
      return refused("invalid", "files.save takes a plain file name of up to 100 characters, like clips.txt.")
    }
    const kinds = Array.isArray(params.filters) ? (params.filters as { extensions?: unknown }[]) : []
    const offered = kinds.flatMap((f) => (Array.isArray(f.extensions) ? f.extensions.map(String) : []))
    if (runnableName(name) || offered.some(runnable)) return refused("invalid", "A page can't save a program, a script or a shortcut.")
    const text = typeof params.text === "string" ? params.text : null
    const data = typeof params.dataBase64 === "string" ? params.dataBase64 : null
    if ((text == null) === (data == null)) return refused("invalid", "files.save takes text or dataBase64, one of them.")
    const blob = new Blob([text ?? Uint8Array.from(atob(data ?? ""), (c) => c.charCodeAt(0))])
    const a = document.createElement("a")
    a.href = URL.createObjectURL(blob)
    a.download = name
    a.click()
    URL.revokeObjectURL(a.href)
    return ok({ saved: true })
  },
  // A file input made in code, never shown, stands in for the open dialog.
  "files.open": (_, params) =>
    new Promise<PageReply>((resolve) => {
      const most = Math.min(typeof params.maxBytes === "number" ? params.maxBytes : 16 << 20, 16 << 20)
      const input = document.createElement("input")
      input.type = "file"
      input.addEventListener("cancel", () => resolve(ok(null)))
      input.addEventListener("change", () => {
        const file = input.files?.[0]
        if (!file) return resolve(ok(null))
        if (file.size > most) return resolve(refused("invalid", `That file is larger than ${most} bytes.`))
        void file.arrayBuffer().then((buf) => {
          let bin = ""
          for (const b of new Uint8Array(buf)) bin += String.fromCharCode(b)
          resolve(ok({ name: file.name, size: file.size, dataBase64: btoa(bin) }))
        })
      })
      input.click()
    }),
}

/** The call of `method` of the page `session` with `params` (their JSON), as `plugin_page_call` answers it. */
export async function pageCall(session: string, method: string, params: string, places: PagePlaces): Promise<PageReply> {
  const open = sessions.get(session)
  if (!open) return refused("closed", "This page was closed.")
  if (!Object.hasOwn(pluginPageCall, method)) return refused("invalid", `AutoScene has no method ${method}.`)
  const m = method as HostMethod
  let read: unknown
  try {
    read = JSON.parse(params)
  } catch {
    return refused("invalid", `The params of ${m} aren't JSON.`)
  }
  const why = missing(m, open.plugin)
  if (why) return refused("denied", why)
  const args = typeof read === "object" && read !== null ? (read as Record<string, unknown>) : {}
  return pluginPageCall[m]({ session, ...open }, args, places)
}

/**
 * The run of `action` of `id`, as `queue_run` checks it: its plugin may run
 * now, an export action has an export, a command never runs on one, and one
 * that needs a project has it. Its title, its plugin's name and what it says
 * when it ends.
 */
export function pluginRun(
  id: string,
  kind: ActionKind,
  action: string,
  projectId: string | null,
  exportId: string | null,
  allow: boolean,
): { title: string; name: string; message: string } {
  const p = get(id)
  const shown = info(p, allow)
  if (!shown.active) throw { code: "invalid", message: p.broken?.text ?? p.withdrawn ?? shown.blockedReason?.text ?? `${p.name} is turned off.` }
  const refuse = (message: string) => {
    throw { code: "invalid", message }
  }
  const command = p.commands.find((c) => c.id === action)
  const exportAction = p.exportActions.find((a) => a.id === action)
  const spec = kind === "exportAction" ? exportAction : command
  // A page names an action without its kind: one of the other kind says what it needs.
  if (!spec && kind === "command" && exportAction) return refuse("Choose an export to run it on.")
  if (!spec && kind === "exportAction" && command) return refuse("A command doesn't run on an export.")
  if (!spec) return refuse(`${p.name} has no such action.`)
  if (kind === "exportAction" && (!exportId || !projectId)) refuse("Choose an export to run it on.")
  if (kind === "command" && exportId) refuse("A command doesn't run on an export.")
  if (kind === "command" && "needsProject" in spec && spec.needsProject && !projectId) refuse("Open a project to run it in.")
  return { title: spec.title, name: p.name, message: MESSAGES.get(`${id} ${action}`) ?? `${spec.title} is done.` }
}

/**
 * The pack action `action` of `id`, as `queue_pack_action` checks it: its
 * plugin may run now and has it. Its plugin's name, its title, and the name
 * the save dialog offers for `character`'s pack.
 */
export function packAction(
  id: string,
  action: string,
  character: string,
  allow: boolean,
): { name: string; title: string; file: string } {
  const p = get(id)
  const shown = info(p, allow)
  if (!shown.active) throw { code: "invalid", message: p.broken?.text ?? shown.blockedReason?.text ?? `${p.name} is turned off.` }
  const a = p.packActions.find((x) => x.id === action)
  if (!a) throw { code: "not_found", message: `${p.name} has no such pack action.` }
  return { name: p.name, title: a.title, file: a.file.name.replaceAll("{character}", character) }
}

/** The export actions the plugins that may run now turned on for after every export. */
export function afterExportRuns(allow: boolean): { pluginId: string; actionId: string }[] {
  return pluginsList(allow)
    .filter((p) => p.active)
    .flatMap((p) => p.exportActions.filter((a) => a.afterExport).map((a) => ({ pluginId: p.id, actionId: a.id })))
}
