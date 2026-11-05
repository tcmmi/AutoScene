import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { cleanup, render, screen } from "@testing-library/react"
import type * as React from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PluginReview } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { fmt } from "@/lib/i18n"
import { queryClient } from "@/lib/queries"
import { catalogProblems } from "@/locales/check"
import plugins from "@/locales/en/plugins"
import { usePluginReview } from "@/state/plugin-review"
import { inLanguage } from "@/test/i18n"
import { catalogPlugin, pluginInfo } from "@/test/plugins"
import { CatalogCard } from "./CatalogCard"
import { catalogDay, catalogLine } from "./catalog-items"
import { InstallReviewDialog } from "./InstallReviewDialog"
import { pageLabel, railTooltip } from "./page-items"
import { authorLine, limitsLine, permissionLabel, resourceLine, trustLabel, trustLine, withdrawnLine } from "./plugin-items"

// The words of the plugin screens come from the catalog (`plugins`), and a plugin's own words stay as it wrote
// them inside AutoScene's sentences. Romanian is a fixture here; the real catalog comes later.

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(() => Promise.resolve({ allowUnreviewedPlugins: false })),
    pluginsList: vi.fn(() => Promise.resolve([])),
    catalogGet: vi.fn(),
    pluginInstall: vi.fn(),
    pluginDiscard: vi.fn(() => Promise.resolve(null)),
    pluginCompileCancel: vi.fn(() => Promise.resolve(null)),
    catalogCancel: vi.fn(() => Promise.resolve(null)),
    pluginsAllowUnreviewed: vi.fn(),
  },
}))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goPlugin: vi.fn(), goPlugins: vi.fn() }) }))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

beforeEach(() => {
  queryClient.clear()
  usePluginReview.setState({ review: null, loading: false, queue: [], getting: null, compiling: null })
})
afterEach(cleanup)

/** Every message of a namespace, by its key. */
function messages(bundle: unknown, prefix = ""): [string, string][] {
  if (typeof bundle === "string") return [[prefix, bundle]]
  if (bundle && typeof bundle === "object") {
    return Object.entries(bundle).flatMap(([k, v]) => messages(v, prefix ? `${prefix}.${k}` : k))
  }
  return []
}

describe("the plugins catalog", () => {
  it("renders: only tags Trans can fill, and no empty text", () => {
    expect(catalogProblems(plugins)).toEqual([])
  })

  it("starts every message about a plugin with a word the plugin can't choose", () => {
    const all = messages(plugins)
    const toasts = all.filter(([k]) => k.startsWith("toast.") && /\.(named|anonymous)$/.test(k))
    expect(toasts.length).toBeGreaterThan(10)
    for (const [key, text] of toasts) {
      if (key.endsWith(".named")) expect(text, key).toMatch(/^[^{]+{{name}}/)
      else expect(text, key).toMatch(/^[^{<]+/)
      // The plugin's own words come after the fixed ones: no variable before `{{name}}`.
      expect(text.indexOf("{{") < 0 || text.indexOf("{{name}}") <= text.indexOf("{{"), key).toBe(true)
    }
    for (const key of ["page.label", "page.railTooltip"]) {
      expect(all.find(([k]) => k === key)?.[1], key).toMatch(/^Plugin “{{name}}”/)
    }
  })
})

describe("the same English as before", () => {
  it("says a catalog's day the way a person does", () => {
    expect(catalogDay(Date.UTC(2026, 10, 3, 12))).toBe("3 November 2026")
    expect(fmt.longDate(Date.UTC(2026, 0, 9, 12))).toBe("9 January 2026")
  })

  it("names a plugin's page after the fixed word", () => {
    expect(pageLabel({ pluginName: "AutoScene", title: "Settings" })).toBe("Plugin “AutoScene” · Settings")
    expect(railTooltip({ pluginName: "AutoScene", title: "Settings" })).toBe("Plugin “AutoScene”: Settings")
  })
})

describe("a plugin's words in a translated sentence", () => {
  it("keeps its name, author and titles as it wrote them", async () => {
    await inLanguage("ro", {
      plugins: {
        by: "De {{author}}",
        saysBy: "Spune că e de {{author}}",
        page: { label: "Pluginul „{{name}}” · {{title}}", railTooltip: "Pluginul „{{name}}”: {{title}}" },
        trust: { reviewed: "Verificat de AutoScene", withdrawn: "Retras" },
        withdrawn: { line: "{{reason}} E oprit." },
      },
    })
    expect(pageLabel({ pluginName: "AutoScene", title: "Setări <b>x</b>" })).toBe("Pluginul „AutoScene” · Setări <b>x</b>")
    expect(railTooltip({ pluginName: "AutoScene", title: "Setări" })).toBe("Pluginul „AutoScene”: Setări")
    const p = { trust: "reviewed", source: "package", author: "Example Dev" } as const
    expect(authorLine(pluginInfo({ id: "dev.a.one", name: "One", ...p }))).toBe("De Example Dev")
    expect(authorLine(pluginInfo({ id: "dev.a.one", name: "One", ...p, trust: "unreviewed" }))).toBe("Spune că e de Example Dev")
    expect(trustLabel({ ...p }).label).toBe("Verificat de AutoScene")
    expect(trustLabel({ ...p, withdrawn: "x" }).label).toBe("Retras")
    expect(withdrawnLine("A trimis căi.")).toBe("A trimis căi. E oprit.")
  })

  it("says counts with the language's plural forms, and lists and days its own way", async () => {
    await inLanguage("ro", {
      plugins: {
        limits: {
          minutes_one: "Memorie {{memory}} MB. Se oprește după {{n}} minut.",
          minutes_few: "Memorie {{memory}} MB. Se oprește după {{n}} minute.",
          minutes_other: "Memorie {{memory}} MB. Se oprește după {{n}} de minute.",
          hours_one: "Memorie {{memory}} MB. Se oprește după {{n}} oră.",
          hours_few: "Memorie {{memory}} MB. Se oprește după {{n}} ore.",
          hours_other: "Memorie {{memory}} MB. Se oprește după {{n}} de ore.",
        },
        resource: { firstRunFrom: "Descarcă {{size}} din {{hosts}} la prima rulare.", firstRun: "Descarcă {{size}}." },
        catalog: {
          of: "Catalog din {{date}}.",
          checkedOn: "Verificat la {{date}}.",
        },
      },
    })
    expect(limitsLine({ memoryMb: 512, minutes: 1 })).toBe("Memorie 512 MB. Se oprește după 1 minut.")
    expect(limitsLine({ memoryMb: 512, minutes: 3 })).toBe("Memorie 512 MB. Se oprește după 3 minute.")
    expect(limitsLine({ memoryMb: 512, minutes: 25 })).toBe("Memorie 512 MB. Se oprește după 25 de minute.")
    expect(limitsLine({ memoryMb: 512, minutes: 120 })).toBe("Memorie 512 MB. Se oprește după 2 ore.")
    // A size's digits and a list of sites follow the language: "1,5 GB", "a, b și c".
    expect(resourceLine({ resourceBytes: 1.5 * 1024 ** 3, resourceHosts: ["a.example", "b.example", "c.example"] }, "process")).toBe(
      "Descarcă 1,5 GB din a.example, b.example și c.example la prima rulare.",
    )
    expect(resourceLine({ resourceBytes: 3 * 1024 ** 3, resourceHosts: [] }, "process")).toBe("Descarcă 3 GB.")
    const status = { ready: true, daily: true, published: Date.UTC(2026, 10, 3, 12), expires: null, fetchedAt: null, checkedAt: Date.UTC(2026, 9, 20, 12), outOfDate: false, error: null, needsNewerApp: false }
    expect(catalogLine(status, Date.UTC(2026, 10, 5, 12))).toBe("Catalog din 3 noiembrie 2026. Verificat la 20 octombrie 2026.")
  })

  it("keeps a sandbox's permissions and the trust line whole, and a missing translation reads in English", async () => {
    await inLanguage("ro", { plugins: { permission: { walled: { exportsRead: "Citește fișierele din dosarul exportului" } } } })
    expect(permissionLabel("exports:read", { hosts: [] })).toBe("Citește fișierele din dosarul exportului")
    expect(permissionLabel("exports:write", { hosts: [] })).toBe("Reads, changes and deletes files in the export's folder")
    expect(trustLine({ trust: "reviewed", source: "package" })).toBe("AutoScene reviewed this version.")
  })

  it("shows a catalog card in the language, with the plugin's summary as written", async () => {
    await inLanguage("ro", {
      plugins: {
        by: "De {{author}}",
        version: "versiunea {{version}}",
        trust: { reviewed: "Verificat de AutoScene" },
        sandboxed: "În sandbox",
        catalog: { install: "Instalează…", installNamed: "Instalează {{name}}…", downloadsRun: "Descarcă {{size}} la prima rulare" },
        options: "Opțiunile {{name}}",
      },
    })
    const p = catalogPlugin({ id: "dev.example.big-model", name: "Big Model", author: "Example Dev", summary: "Finds the <b>big</b> shots.", runtime: "wasm", resourceBytes: 327_155_712 })
    const root = createRootRoute({ component: () => <CatalogCard p={p} /> })
    const plugin = createRoute({ getParentRoute: () => root, path: "/plugins/$id", component: () => null })
    const router = createRouter({ routeTree: root.addChildren([plugin]), history: createMemoryHistory({ initialEntries: ["/"] }) })
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>{children}</TooltipProvider>
      </QueryClientProvider>
    )
    render(<RouterProvider router={router} />, { wrapper })
    expect(await screen.findByText("Verificat de AutoScene")).toBeTruthy()
    expect(screen.getByText("În sandbox")).toBeTruthy()
    expect(screen.getByText("De Example Dev · versiunea 1.0.0")).toBeTruthy()
    expect(screen.getByText("Descarcă 312 MB la prima rulare")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Instalează…" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Opțiunile Big Model" })).toBeTruthy()
    // The summary is the plugin's: its markup is text.
    expect(screen.getByText("Finds the <b>big</b> shots.")).toBeTruthy()
  })
})

describe("the install review in another language", () => {
  const discord = pluginInfo({
    id: "dev.example.discord-notify",
    name: "Discord <i>Notify</i>",
    version: "0.3.1",
    author: "Example Dev",
    trust: "unreviewed",
    permissions: ["network"],
    networkHosts: ["discord.com"],
    exportActions: [{ id: "post", title: "Post <b>to</b> Discord", description: null, afterExport: false, afterFailed: true }],
    pages: [{ id: "board", title: "Board", scope: "global", rail: true, description: null, entry: "board.html" }],
  })
  const review: PluginReview = {
    ticketId: "t1",
    plugin: discord,
    update: null,
    needsUnreviewed: true,
    allowed: true,
    uvMissing: false,
    uvBytes: 15236317,
    resourceBytes: 0,
    resourceHosts: [],
    catalog: null,
    componentWarnings: [],
  }

  it("shows a plugin's own {{braces}} as written, in the name to type and in what it adds", async () => {
    await inLanguage("en")
    const odd = pluginInfo({
      ...discord,
      name: "a{{name}}b{{max}}",
      exportActions: [{ id: "post", title: "Post {{title}} {{max}}", description: null, afterExport: false, afterFailed: true }],
      pages: [{ id: "board", title: "{{what}}", scope: "global", rail: true, description: null, entry: "board.html" }],
    })
    usePluginReview.setState({ review: { ...review, plugin: odd } })
    render(<InstallReviewDialog />)
    // The label says exactly the name the confirm check compares with.
    expect(screen.getByLabelText("Type a{{name}}b{{max}} to install it")).toBeTruthy()
    expect(screen.getAllByText("Post {{title}} {{max}}", { exact: false }).length).toBe(2)
    expect(screen.getByText("{{what}}", { exact: false })).toBeTruthy()
  })

  it("says the verb, the question and what it adds as whole sentences, around the plugin's own words", async () => {
    await inLanguage("ro", {
      plugins: {
        review: {
          install: "Instalezi {{name}}?",
          installButton: "Instalează",
          typeToInstall: "Scrie <x/> ca să-l instalezi",
          adds: {
            title: "Ce adaugă",
            pageRail: "<x>O pagină:</x> <what/><x>, în bară (are loc pentru {{max}} pagini)</x>",
            afterFailed: "<x>Rulează după fiecare export, și după unul eșuat:</x> <what/><x> (îl pornești tu)</x>",
          },
        },
      },
    })
    usePluginReview.setState({ review })
    render(<InstallReviewDialog />)
    expect(screen.getByRole("heading", { name: "Instalezi Discord <i>Notify</i>?" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Instalează" })).toBeTruthy()
    // The name to type is the plugin's, as it wrote it, with its markup left as text.
    expect(screen.getByLabelText("Scrie Discord <i>Notify</i> ca să-l instalezi")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "Ce adaugă" })).toBeTruthy()
    expect(screen.getByText("O pagină:")).toBeTruthy()
    expect(screen.getByText(", în bară (are loc pentru 4 pagini)")).toBeTruthy()
    expect(screen.getAllByText("Post <b>to</b> Discord", { exact: false }).length).toBe(2)
    expect(screen.getByText("Rulează după fiecare export, și după unul eșuat:")).toBeTruthy()
  })
})
