import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { ExportItem, ExportSummary, JobInfo, MediaItem } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { useRelink } from "@/state/relink"
import { inLanguage } from "@/test/i18n"
import { AnalysisStatus } from "./AnalysisStatus"
import { CharacterCrumb, MediaCrumb, PackCrumb } from "./crumbs"
import { ExportsPage } from "./ExportsPage"
import { MissingBanner } from "./MissingBanner"
import { RelinkDialog } from "./RelinkDialog"
import { folderResult } from "./relink"
import en from "@/locales/en/project"
import { StatusCell } from "./StatusCell"

// A project's words in a language that isn't English, from fixture catalogs: Romanian has three plural forms,
// and a sentence that English builds from one word order is whole in the catalog, so the language is free to
// reorder it. The English of these screens is pinned by each screen's own test.

const { pickSaveFile, exportSaveList } = vi.hoisted(() => ({ pickSaveFile: vi.fn(), exportSaveList: vi.fn() }))
vi.mock("@tanstack/react-router", () => ({
  useParams: () => ({ pid: "p" }),
  useNavigate: () => vi.fn(),
  useCanGoBack: () => false,
}))
vi.mock("@/lib/media", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/media")>()), pickSaveFile }))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return {
    ...ipc,
    commands: {
      ...ipc.commands,
      exportSaveList,
      settingsGet: vi.fn(() => Promise.resolve({ findBodies: false })),
      projectGet: vi.fn(() => Promise.resolve({ id: "p", fastScan: false })),
    },
  }
})
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/components/ui/page", () => ({ Page: ({ children }: { children: React.ReactNode }) => <main>{children}</main> }))

const exportsList = vi.hoisted(() => ({ exports: [] as unknown[], media: [] as unknown[], cast: { characters: [] as unknown[] } }))
vi.mock("@/lib/queries", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/queries")>()),
  useProject: () => ({ data: { title: "Show" } }),
  useExports: () => ({ data: exportsList.exports }),
  useMediaList: () => ({ data: exportsList.media }),
  useCast: () => ({ data: exportsList.cast }),
  usePlugins: () => ({ data: [] }),
  firstFailure: () => null,
}))

beforeEach(() => {
  vi.clearAllMocks()
  exportsList.exports = []
  exportsList.media = []
  exportsList.cast = { characters: [] }
  useActivity.setState({ jobs: {}, order: [] })
})
afterEach(() => {
  cleanup()
  queryClient.clear()
  useRelink.setState({ question: null })
})

/** Romanian fixture for the `project` namespace: only what a test looks at. */
const ro = (words: Record<string, unknown>) => inLanguage("ro", { project: words, common: { action: { cancel: "Anulează", retry: "Reîncearcă" } } })

const clip = (id: number): ExportItem =>
  ({ id, mediaId: "m", start: id * 100, end: id * 100 + 24, name: `Clip_${id}`, status: "done", path: null, verified: true, error: null }) as ExportItem
const done = (items: ExportItem[], merged: ExportSummary["merged"] = null): ExportSummary =>
  ({ id: "e1", projectId: "p", createdAt: 1, preset: { name: "ProRes 422 HQ", container: "mov" }, outDir: "C:\\Out", status: "done", items, label: null, merged }) as ExportSummary

function showExports(ex: ExportSummary) {
  exportsList.exports = [ex]
  render(
    <AppMenu>
      <div>
        <ExportsPage />
      </div>
    </AppMenu>,
    { wrapper: TooltipProvider },
  )
}

describe("the exports page in another language", () => {
  it("says an export's count with the language's plural form, whole, and its buttons and badges from the catalog", async () => {
    await ro({
      exports: {
        title: "Exporturi",
        clipsDone_one: "{{done}} din {{count}} clip",
        clipsDone_few: "{{done}} din {{count}} clipuri",
        clipsDone_other: "{{done}} din {{count}} de clipuri",
        options: "Opțiuni pentru {{name}}",
        showClips_few: "Arată {{count}} clipuri",
        showClips_other: "Arată {{count}} de clipuri",
        hideClips: "Ascunde clipurile",
      },
      menu: { showFolder: "Arată dosarul" },
    })
    // Three clips: the few form; twenty: the other form.
    showExports(done([clip(1), clip(2), clip(3)]))
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Exporturi")
    expect(screen.getByText(/3 din 3 clipuri/)).toBeTruthy()
    expect(screen.getByRole("button", { name: "Opțiuni pentru ProRes 422 HQ" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Arată dosarul" })).toBeTruthy()
    cleanup()
    showExports(done(Array.from({ length: 20 }, (_, i) => clip(i + 1))))
    expect(screen.getByText(/20 din 20 de clipuri/)).toBeTruthy()
    // Folded, the list says how many it hides, in the plural form of the language.
    expect(screen.getByRole("button", { name: /Arată 20/ }).textContent).toBe("Arată 20 de clipuri")
  })

  it("picks the plural form by the export's total, not by how many are done, and groups both numbers", async () => {
    await ro({
      exports: {
        clipsDone_one: "{{done, number}} din {{count}} clip",
        clipsDone_few: "{{done, number}} din {{count}} clipuri",
        clipsDone_other: "{{done, number}} din {{count}} de clipuri",
      },
    })
    const some = (n: number, total: number) =>
      done(Array.from({ length: total }, (_, i) => ({ ...clip(i + 1), status: i < n ? "done" : "failed" }) as ExportItem))
    // 2 of 20: done would be "few", the total is "other". 1 of 3: done is "one", the total is "few".
    showExports(some(2, 20))
    expect(screen.getByText(/2 din 20 de clipuri/)).toBeTruthy()
    cleanup()
    showExports(some(1, 3))
    expect(screen.getByText(/1 din 3 clipuri/)).toBeTruthy()
    cleanup()
    showExports(some(1200, 1300))
    expect(screen.getByText(/1\.200 din 1\.300 de clipuri/)).toBeTruthy()
  })

  it("groups the done number in English too", () => {
    showExports(done(Array.from({ length: 1300 }, (_, i) => clip(i + 1))))
    expect(screen.getByText(/1,300 of 1,300 clips/)).toBeTruthy()
    cleanup()
    showExports(done(Array.from({ length: 1300 }, (_, i) => ({ ...clip(i + 1), status: i < 1200 ? "done" : "failed" }) as ExportItem)))
    expect(screen.getByText(/1,200 of 1,300 clips/)).toBeTruthy()
  })

  it("says the one file's count and its menu in the language, and a save's toast", async () => {
    await ro({
      exports: {
        clipsDoneInOneFile_few: "{{done}} din {{count}} clipuri într-un singur fișier",
        file: { chapters_few: "{{count}} capitole" },
        saveEdl: "Salvează EDL…",
        savedEdl: "EDL salvat",
        saveEdlTitle: "Salvează EDL",
        leftOut_one: "{{count}} clip a fost lăsat deoparte: episodul lui a fost șters.",
        leftOut_few: "{{count}} clipuri au fost lăsate deoparte: episoadele lor au fost șterse.",
      },
      menu: { showInFolder: "Arată în dosar" },
    })
    pickSaveFile.mockResolvedValue("D:\\Edit\\Walter.edl")
    exportSaveList.mockResolvedValue({ path: "D:\\Edit\\Walter.edl", leftOut: 3 })
    const merged = { name: "pack", path: "C:\\Out\\pack.mov", verified: true, bytes: 1e6, error: null }
    showExports({ ...done([clip(1), clip(2), clip(3)].map((c) => ({ ...c, path: merged.path })), merged), label: "Walter · scenepack" })
    expect(screen.getByText(/3 din 3 clipuri într-un singur fișier/)).toBeTruthy()
    expect(screen.getByText(/3 capitole/)).toBeTruthy()
    fireEvent.contextMenu(screen.getByRole("heading", { level: 2 }), { clientX: 40, clientY: 40 })
    fireEvent.click(await screen.findByRole("menuitem", { name: "Salvează EDL…" }))
    await waitFor(() => expect(toast).toHaveBeenCalledOnce())
    // The dialog's title, the toast, its note and its button all come from the catalog.
    expect(pickSaveFile).toHaveBeenCalledWith("Salvează EDL", expect.any(String), { name: "EDL", extensions: ["edl"] })
    const [title, options] = vi.mocked(toast).mock.calls[0] as unknown as [string, { description?: string; action: { label: string } }]
    expect([title, options.description, options.action.label]).toEqual([
      "EDL salvat",
      "3 clipuri au fost lăsate deoparte: episoadele lor au fost șterse.",
      "Arată în dosar",
    ])
    // One left out is the singular message; none left out has no note.
    for (const [leftOut, note] of [[1, "1 clip a fost lăsat deoparte: episodul lui a fost șters."], [0, undefined]] as const) {
      vi.mocked(toast).mockClear()
      exportSaveList.mockResolvedValue({ path: "D:\\Edit\\Walter.edl", leftOut })
      fireEvent.contextMenu(screen.getByRole("heading", { level: 2 }), { clientX: 40, clientY: 40 })
      fireEvent.click(await screen.findByRole("menuitem", { name: "Salvează EDL…" }))
      await waitFor(() => expect(toast).toHaveBeenCalledOnce())
      expect((vi.mocked(toast).mock.calls[0] as unknown as [string, { description?: string }])[1].description).toBe(note)
    }
  })
})

describe("an episode's state and the project's analysis in another language", () => {
  const job = (o: Partial<JobInfo>): JobInfo =>
    ({ id: "a", kind: "analyze", title: "t", projectId: "p", mediaId: "m", state: "queued", progress: null, stage: null, etaMs: null, rev: 1, createdAt: 1, ...o }) as JobInfo

  it("counts the episodes waiting with the language's plural forms", async () => {
    await ro({
      analysis: { waiting: "Așteaptă analiza", moreQueued_one: "{{count}} episod în așteptare", moreQueued_few: "{{count}} episoade în așteptare", moreQueued_other: "{{count}} de episoade în așteptare" },
    })
    const counts = async (n: number) => {
      useActivity.getState().replaceAll(Array.from({ length: n + 1 }, (_, i) => job({ id: `a${i}`, mediaId: `m${i}`, state: i === 0 ? "running" : "queued", stage: "Găsește cadre" })))
      const { unmount } = render(
        <QueryClientProvider client={queryClient}>
          <AnalysisStatus pid="p" />
        </QueryClientProvider>,
      )
      const text = screen.getByText(/în așteptare/).textContent
      unmount()
      return text
    }
    expect([await counts(1), await counts(3), await counts(20)]).toEqual([
      "1 episod în așteptare",
      "3 episoade în așteptare",
      "20 de episoade în așteptare",
    ])
  })

  it("says how many files can't be found by a series' or a movie's own words", async () => {
    await ro({
      missing: {
        episodes_few: "{{count}} episoade nu se găsesc",
        files_one: "{{count}} fișier nu se găsește",
        locateFolder: "Caută dosarul…",
      },
    })
    const lost = (n: number) => Array.from({ length: n }, () => ({ id: "m", status: "missing" }) as MediaItem)
    const { unmount } = render(<MissingBanner pid="p" media={lost(3)} />)
    expect(screen.getByText("3 episoade nu se găsesc")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Caută dosarul…" })).toBeTruthy()
    unmount()
    render(<MissingBanner pid="p" media={lost(1)} movie />)
    expect(screen.getByText("1 fișier nu se găsește")).toBeTruthy()
  })

  it("keeps the count set in the timecode face inside a translated sentence, grouped the language's way", async () => {
    await ro({ status: { shots_one: "<mono>{{count}}</mono> cadru", shots_few: "<mono>{{count}}</mono> cadre", shots_other: "<mono>{{count}}</mono> de cadre", faces_other: "<mono>{{count}}</mono> de fețe" } })
    const m = { id: "m", status: "ready", shots: "ready", shotCount: 14_200, faces: "ready", faceCount: 31 } as MediaItem
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <StatusCell media={m} projectId="p" />
        </TooltipProvider>
      </QueryClientProvider>,
    )
    expect(container.textContent).toContain("14.200 de cadre")
    expect(container.textContent).toContain("31 de fețe")
    // The number itself keeps its face.
    expect([...container.querySelectorAll(".timecode")].map((e) => e.textContent)).toEqual(["14.200", "31"])
  })
})

describe("relinking in another language", () => {
  const media = { id: "m", fileName: "E01.mkv", label: "S01E01" } as MediaItem

  it("asks in the language, with the file's and the episode's names and the reason as they are", async () => {
    await ro({
      relink: {
        questionTitle: "Folosești „{{file}}” pentru {{episode}}?",
        questionDescription: "Nu pare același episod. {{reason}} Dacă îl folosești oricum, e analizat din nou.",
        useAnyway: "Folosește oricum",
      },
    })
    render(<RelinkDialog />)
    act(() => useRelink.getState().ask({ pid: "p", media, path: "D:/x/other.mkv", reason: "It is shorter." }))
    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByText("Folosești „other.mkv” pentru S01E01 · E01?")).toBeTruthy()
    expect(within(dialog).getByText("Nu pare același episod. It is shorter. Dacă îl folosești oricum, e analizat din nou.")).toBeTruthy()
    expect(within(dialog).getByRole("button", { name: "Folosește oricum" })).toBeTruthy()
    expect(within(dialog).getByRole("button", { name: "Anulează" })).toBeTruthy()
  })

  it("says what looking in a folder found as whole messages: the plural form, the movie's own words", async () => {
    await ro({
      relink: {
        folder: {
          episodes: {
            foundAll_one: "Am găsit episodul",
            foundAll_few: "Am găsit toate cele {{count}} episoade",
            foundSome_few: "Am găsit {{found}} din {{count}} episoade",
            stillMissing_other: "{{count}} de episoade nu se găsesc încă.",
          },
          files: { noneTitle: "Niciunul nu e în acest dosar", noneDescription: "Alege dosarul în care sunt fișierele acum." },
          copies_few: "{{count}} sunt alte copii, indexate din nou.",
        },
      },
    })
    expect(folderResult({ same: 1, reindex: 0, missing: 0 }, 1)).toEqual({ title: "Am găsit episodul" })
    expect(folderResult({ same: 1, reindex: 2, missing: 0 }, 3)).toEqual({
      title: "Am găsit toate cele 3 episoade",
      description: "2 sunt alte copii, indexate din nou.",
    })
    expect(folderResult({ same: 3, reindex: 0, missing: 20 }, 23).description).toBe("20 de episoade nu se găsesc încă.")
    expect(folderResult({ same: 2, reindex: 0, missing: 1 }, 3).title).toBe("Am găsit 2 din 3 episoade")
    expect(folderResult({ same: 0, reindex: 0, missing: 2 }, 2, true)).toEqual({
      title: "Niciunul nu e în acest dosar",
      description: "Alege dosarul în care sunt fișierele acum.",
    })
  })
})

describe("the window title's crumbs in another language", () => {
  it("follow the language, with the character's name as a value", async () => {
    await ro({ crumbs: { episode: "Episod", character: "Personaj", pack: "{{name}} · Pachet de scene", everyoneElse: "Distribuție · Ceilalți", cast: "Distribuție · {{name}}" } })
    exportsList.cast = { characters: [{ id: "c1", label: "Walter" }] }
    const { container } = render(
      <>
        <MediaCrumb pid="p" mid="gone" />|<PackCrumb pid="p" cid="c1" />|<PackCrumb pid="p" cid="nobody" />|
        <CharacterCrumb pid="p" cid="c1" />|<CharacterCrumb pid="p" cid="unassigned" />
      </>,
    )
    expect(container.textContent).toBe(
      "Episod|Walter · Pachet de scene|Personaj · Pachet de scene|Distribuție · Walter|Distribuție · Ceilalți",
    )
  })
})

describe("the relink catalog", () => {
  it("has the same keys for episodes and for files: relink.ts picks them with a template, no key scan sees them", () => {
    const { episodes, files } = en.relink.folder
    expect(Object.keys(files).sort()).toEqual(Object.keys(episodes).sort())
  })
})
