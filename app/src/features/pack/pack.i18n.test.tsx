import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { type Character, type ExportPreset, type MediaItem, type Msg, NAME_PARTS, type PackClip, type PackFilter, type PackNaming } from "@/bindings"
import { toast } from "sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { t } from "@/lib/i18n"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { inLanguage, plain } from "@/test/i18n"
import { pickFolder } from "@/lib/media"
import { PackCard } from "./PackCard"
import { PackExportDialog } from "./PackExportDialog"
import { PackFooter } from "./PackFooter"
import { checkingText, DEFAULT_SPEC, LENGTHS, lengthLabel, lengthOf, REASONS, saveHeading } from "./spec"
import type { Pack } from "./use-pack"

// A preset as the backend sends it: its English, and beside it a `Msg` said by key in the language in use.
const proresName: Msg = { key: "export.preset.proresHq.name", text: "ProRes 422 HQ" }
const prores = {
  id: "prores-hq",
  name: "ProRes 422 HQ",
  nameMsg: proresName,
  description: "Edit-grade intra codec.",
  container: "mov",
  video: { codec: "prores", profile: "hq" },
  audio: { codec: "pcm24" },
  maxHeight: null,
  hdr: "auto",
  stereo: false,
  builtin: true,
} as unknown as ExportPreset

vi.mock("@/lib/ipc", () => ({
  commands: {
    exportPresets: vi.fn(() => Promise.resolve([prores])),
    settingsGet: vi.fn(() => Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" })),
    settingsUpdate: vi.fn(),
    exportDefaultDir: vi.fn(() => Promise.resolve("C:\\Library\\Exports\\Show")),
    packExport: vi.fn(() => Promise.resolve({ id: "e1" })),
    projectGet: vi.fn(() => Promise.resolve({ id: "p", kind: "series" })),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/lib/media", async (original) => ({ ...(await original<object>()), pickFolder: vi.fn() }))

afterEach(() => {
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
  useActivity.setState({ jobs: {}, order: [] })
})

// What a Romanian catalog would hold for the words these tests touch. Romanian has a `few` form (2 to 19, and
// 101 to 119), which English lacks: a message that picked its form by `n === 1` would say these wrong.
const ro = {
  pack: {
    shots_one: "{{count}} cadru",
    shots_few: "{{count}} cadre",
    shots_other: "{{count}} de cadre",
    reason: { missing: "Fișier lipsă", plugin: "Filtru de plugin" },
    length: { any: "Orice durată", between: "{{min}} – {{max}} s", orLonger: "{{min}} s sau mai mult", upTo: "Până la {{max}} s" },
    check: {
      some_one: "Se verifică {{checked}} din {{total}} cadru",
      some_few: "Se verifică {{checked}} din {{total}} cadre",
      some_other: "Se verifică {{checked}} din {{total}} de cadre",
      all_one: "Se verifică {{total}} cadru",
      all_few: "Se verifică {{total}} cadre",
      all_other: "Se verifică {{total}} de cadre",
    },
    save: { heading: "Salvează cu un plugin" },
    card: {
      aria: { shot: "Cadru", shotAt: "Cadru la {{at}}", withoutFace: "fără fața lor", inPack: "în pachet", leftOut: "scos: {{why}}" },
    },
    footer: {
      picksByHand_one: "{{count}} alegere manuală",
      picksByHand_few: "{{count}} alegeri manuale",
      picksByHand_other: "{{count}} de alegeri manuale",
      exporting: "Se exportă… <x>{{percent}}</x>",
    },
    export: {
      title: "Exportă pachetul lui {{name}}",
      descriptionOneFile_few: "{{count}} cadre într-un singur video, fiecare tăiat exact și verificat față de sursă.",
      descriptionOneFile_other: "{{count}} de cadre într-un singur video, fiecare tăiat exact și verificat față de sursă.",
      submitOneFile_few: "Exportă {{count}} cadre într-un singur fișier",
      submitOneFile_other: "Exportă {{count}} de cadre într-un singur fișier",
      analysing: {
        episode_few: "{{count}} episoade se analizează încă. Cadrele cu {{name}} găsite în ele mai târziu nu vor intra în acest export.",
      },
      more_one: "și încă {{count}}",
      more_few: "și încă {{count}}",
      more_other: "și încă {{count}} de",
      oneFile: { label: "Toate cadrele într-un fișier" },
      folder: "Într-un dosar numit după {{name}}",
    },
  },
  export: { about: "Aproximativ {{size}} în total ca {{format}}.", field: { format: "Format" } },
  backend: { export: { preset: { proresHq: { name: "Editare ProRes 422 HQ" } } } },
}

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

describe("the pack's counts and checks", () => {
  it("say what they always said in English, a plural key each", () => {
    expect([1, 0, 2, 14_200].map((count) => t("pack:shots", { count }))).toEqual(["1 shot", "0 shots", "2 shots", "14,200 shots"])
    expect([1, 3].map((count) => t("pack:episodes", { count }))).toEqual(["1 episode", "3 episodes"])
    expect([1, 3].map((count) => t("pack:check.failed", { count }))).toEqual(["Couldn't check 1 shot", "Couldn't check 3 shots"])
    expect([1, 2].map((count) => t("pack:footer.picksByHand", { count }))).toEqual(["1 pick by hand", "2 picks by hand"])
    expect([1, 2].map((count) => t("pack:toast.handedBack", { count }))).toEqual([
      "Handed 1 shot back to the filters",
      "Handed 2 shots back to the filters",
    ])
    expect(checkingText(0, 1)).toBe("Checking 1 shot")
    expect(checkingText(0, 10)).toBe("Checking 10 shots")
    expect(checkingText(3, 10)).toBe("Checking 3 of 10 shots")
    // As this always read: no thousands separator in a check's count.
    expect(checkingText(300, 1200)).toBe("Checking 300 of 1200 shots")
  })

  it("name the menu's lengths, the header's counts and the saves' headings as they always read", () => {
    expect(LENGTHS.map((l) => lengthLabel(l.min, l.max))).toEqual([
      "Any length",
      "1 s or longer",
      "2 s or longer",
      "1 to 6 s",
      "2 to 10 s",
      "Up to 6 s",
    ])
    expect(t("pack:page.header", { shots: t("pack:shots", { count: 12 }), episodes: t("pack:episodes", { count: 3 }) })).toBe(
      "12 shots in 3 episodes · the pack takes the bright ones",
    )
    expect(t("pack:page.header", { shots: t("pack:shots", { count: 1 }), episodes: t("pack:episodes", { count: 1 }) })).toBe(
      "1 shot in 1 episode · the pack takes the bright ones",
    )
  })

  it("name a length the way the menu always did, a tenth at most", () => {
    expect(lengthOf({ ...DEFAULT_SPEC, minSec: 1, maxSec: 6 }).label).toBe("1 to 6 s")
    expect(lengthOf({ ...DEFAULT_SPEC, minSec: 2, maxSec: null }).label).toBe("2 s or longer")
    expect(lengthOf({ ...DEFAULT_SPEC, minSec: null, maxSec: 6 }).label).toBe("Up to 6 s")
    expect(lengthOf({ ...DEFAULT_SPEC, minSec: 1.26, maxSec: 8 }).label).toBe("1.3 to 8 s")
  })

  it("name every reason a shot is left out, as the card always said it", () => {
    expect(REASONS).toEqual({
      missing: "File missing",
      episode: "Other episode",
      tiny: "Flash cut",
      short: "Too short",
      long: "Too long",
      size: "Other size",
      not_solo: "Not alone",
      faceless: "No face",
      limit: "Past the limit",
      removed: "Taken out",
      plugin: "Plugin filter",
    })
    expect(t("pack:reason.other")).toBe("Left out")
  })

  it("name the shot sizes", () => {
    const sizes = ["extremeCloseUp", "closeUp", "mediumCloseUp", "medium", "wide"] as const
    expect(sizes.map((k) => t(`pack:size.${k}.label`))).toEqual(["Extreme close-up", "Close-up", "Medium close-up", "Medium shot", "Wide shot"])
    expect(sizes.map((k) => t(`pack:size.${k}.short`))).toEqual(["ECU", "CU", "MCU", "MS", "WS"])
  })

  it("name the name parts as the backend does, with a hint each", () => {
    for (const [part, label] of NAME_PARTS) expect(t(`pack:namePart.${part}.label`)).toBe(label)
    expect(NAME_PARTS.map(([part]) => t(`pack:namePart.${part}.hint`))).toEqual([
      "The project's title",
      "The character's name",
      "The episode, as S01E03, or its file's name",
      "The shot's place in the pack: 001, 002, …",
      "Where the shot starts, as 00-12-03-05",
      "The frame the shot starts on, as 17357",
    ])
  })

  it("follow the language's plural forms and digits: Romanian has a few", async () => {
    await act(() => inLanguage("ro", ro))
    expect([1, 3, 19, 20, 101, 120].map((count) => t("pack:shots", { count }))).toEqual([
      "1 cadru",
      "3 cadre",
      "19 cadre",
      "20 de cadre",
      "101 cadre",
      "120 de cadre",
    ])
    expect(checkingText(0, 3)).toBe("Se verifică 3 cadre")
    expect(checkingText(2, 25)).toBe("Se verifică 2 din 25 de cadre")
    // A decimal in the language's own digits, and the reasons and headings read when asked, not when first imported.
    expect(lengthOf({ ...DEFAULT_SPEC, minSec: 1.5, maxSec: 8 }).label).toBe("1,5 – 8 s")
    expect(lengthOf(DEFAULT_SPEC).label).toBe("Orice durată")
    expect(REASONS.missing).toBe("Fișier lipsă")
    expect(saveHeading({ taken: 1, current: true, check: null })).toBe("Salvează cu un plugin")
    // A word the language lacks stays English.
    expect(REASONS.tiny).toBe("Flash cut")
  })
})

describe("a shot's card", () => {
  const speaks: PackFilter = {
    key: "dev.example.speech-filter/speaks",
    title: "Speaks",
    description: null,
    reason: "No speech",
    plugin: "Speech Filter",
    checked: 3,
    total: 3,
    state: "ready",
  }
  const clip = (more: Partial<PackClip>, shot: Partial<PackClip["shot"]> = {}): PackClip => ({
    shot: { mediaId: "m", start: 0, end: 48, sprite: null, cells: 0, trackId: 1, face: "", faceHeight: 0.3, ...shot },
    start: 0,
    end: 48,
    size: "close_up",
    quality: 1,
    coverage: 1,
    others: 0,
    score: 1,
    included: false,
    byRules: false,
    reason: "plugin",
    manual: null,
    filter: 0,
    ...more,
  })
  const episode = { id: "m", label: "S01E01", fps: { num: 24, den: 1 } } as MediaItem
  const card = (c: PackClip, media?: MediaItem, filter?: PackFilter) =>
    render(<PackCard clip={c} index={0} media={media} filter={filter} onToggle={() => {}} onOpen={() => {}} />, { wrapper })
  /** The card's button: its screen reader name is the sentence under test. */
  const shotName = () => screen.getAllByRole("button").find((b) => b.hasAttribute("aria-pressed"))!.getAttribute("aria-label")

  it("is named by facts joined with commas, as it always was, in English", () => {
    card(clip({}), episode, speaks)
    expect(shotName()).toBe("Shot at S01E01 · 00:00:00:00, 2.0 s, left out: No speech")
    cleanup()
    card(clip({ included: true, reason: null }, { faceless: true }))
    expect(shotName()).toBe("Shot, without their face, in the pack")
    cleanup()
    card(clip({ filter: null }))
    expect(shotName()).toBe("Shot, left out: Plugin filter")
  })

  it("is named in the language's words, its own facts staying as they are", async () => {
    await act(() => inLanguage("ro", ro))
    card(clip({}), episode, speaks)
    // The plugin's own reason ("No speech") and the episode's name stay as they came.
    expect(shotName()).toBe("Cadru la S01E01 · 00:00:00:00, 2,0 s, scos: No speech")
    cleanup()
    card(clip({ filter: null }))
    expect(shotName()).toBe("Cadru, scos: Filtru de plugin")
  })

  it("says in its tooltip what a click does, and its play button where it is", () => {
    const tooltip = () => {
      fireEvent.focus(screen.getAllByRole("button").find((b) => b.hasAttribute("aria-pressed"))!)
      return screen.getAllByText(/Click to/)[0].textContent
    }
    card(clip({}), episode, speaks)
    expect(tooltip()).toBe("Close-up · Click to take it in")
    expect(screen.getByRole("button", { name: "Watch the shot at S01E01 · 00:00:00:00" })).toBeTruthy()
    cleanup()
    card(clip({ included: true, manual: true, reason: null }, { faceless: true }), episode, speaks)
    expect(tooltip()).toBe("Close-up · their face doesn't show · “Speaks” would leave it out · Click to leave it out")
    cleanup()
    card(clip({}))
    expect(screen.getByRole("button", { name: "Watch the shot" })).toBeTruthy()
  })

  it("asks whose pack it is only when it knows: Not them, Not Walter", () => {
    expect(t("pack:card.notThem", { name: "Walter" })).toBe("Not Walter")
    expect(t("pack:card.notThemAnon")).toBe("Not them")
    expect(t("pack:toast.notThem", { name: "Walter" })).toBe("Not Walter: out of the pack")
    expect(t("pack:toast.notThemAnon")).toBe("Not them: out of the pack")
  })
})

describe("the footer", () => {
  const footer = (props: Partial<React.ComponentProps<typeof PackFooter>> = {}) =>
    render(
      <PackFooter
        totals={{ clips: 3, frames: 72, seconds: 3 }}
        bytes={3e8}
        format="ProRes 422 HQ"
        picks={5}
        canExport
        onClearPicks={() => {}}
        onExport={() => {}}
        onShowExport={() => {}}
        {...props}
      />,
      { wrapper },
    )

  it("says Exporting… with its percent standing out, in the order the language puts them", async () => {
    await act(() => inLanguage("ro", ro))
    const { container } = footer({ exporting: { state: "running", progress: 0.4 } })
    expect(screen.getByRole("button", { name: "Se exportă… 40%" })).toBeTruthy()
    expect(container.querySelector(".timecode")?.textContent).toBe("40%")
  })

  it("counts its shots and its picks in the language's forms", async () => {
    footer()
    expect(screen.getByText("3 shots")).toBeTruthy()
    expect(screen.getByText("5 picks by hand")).toBeTruthy()
    cleanup()
    await act(() => inLanguage("ro", ro))
    footer()
    expect(screen.getByText("3 cadre")).toBeTruthy()
    expect(screen.getByText("5 alegeri manuale")).toBeTruthy()
    // A line the catalog has no Romanian for yet reads in English: nothing is missing, nothing breaks.
    expect(screen.getByText("0:03 · about 286,1 MB (ProRes 422 HQ)")).toBeTruthy()
  })
})

describe("the pack's export dialog", () => {
  const walter = { id: "c", name: "Walter", number: 1 } as Character
  const naming: PackNaming = { template: "{series} {character} {tc}", folder: true, merge: true, file: "{series} {character} scenepack" }
  const pack = (clips: number): Pack =>
    ({
      state: { spec: DEFAULT_SPEC, naming },
      preview: { data: { clips: [], totals: { clips, frames: 72, seconds: 3 }, stamp: "s" } },
      plan: {
        data: {
          names: ["Show_Walter_00-00-00-00", "Show_Walter_00-00-02-02", "Show_Walter_00-00-12-02"],
          sizes: [{ presetId: "prores-hq", bytes: 3e8 }],
          folder: null,
          file: "Show_Walter_scenepack",
        },
        isError: false,
        error: null,
      },
      setNaming: vi.fn(),
      current: true,
    }) as unknown as Pack
  const dialog = (clips: number) =>
    render(<PackExportDialog pid="p" character={walter} pack={pack(clips)} open onOpenChange={() => {}} />, { wrapper })

  it("says how many more names there are with a plural key, and its preset by key", async () => {
    dialog(15)
    expect(await screen.findByText("and 12 more")).toBeTruthy()
    // A preset's name sent as a `Msg` shows its English where the UI has no words for the key.
    await waitFor(() => expect(screen.getByText(/About 286\.1 MB in all as ProRes 422 HQ\./)).toBeTruthy())
    expect(screen.getByRole("button", { name: "Export 15 shots into one file" })).toBeTruthy()
    expect(vi.mocked(commands.exportPresets)).toHaveBeenCalled()
  })

  it("reads as it always did in English: the hint under the format, the folder switch, the toast", async () => {
    const { unmount } = dialog(3)
    const hint = await waitFor(() => {
      const found = screen.getByText(/Edit-grade intra codec./).closest("p")
      expect(found).toBeTruthy()
      return found!
    })
    expect(hint.textContent).toBe(
      "Edit-grade intra codec. About 286.1 MB in all as ProRes 422 HQ. The shots are rendered first, so it needs extra free space for a while.",
    )
    fireEvent.click(screen.getByRole("button", { name: "Export 3 shots into one file" }))
    await waitFor(() => expect(commands.packExport).toHaveBeenCalled())
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Exporting 3 shots of Walter into one file"))
    unmount()
    // A file per shot: the folder switch names the character, the toast and the button count the shot.
    const files = pack(1)
    files.state = { ...files.state!, naming: { ...naming, merge: false } }
    render(<PackExportDialog pid="p" character={walter} pack={files} open onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByText("In a folder named after Walter")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Export 1 shot" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Exporting 1 shot of Walter"))
  })

  it("follows the language: forms of few and other, the preset said by its key, the character a value", async () => {
    await act(() => inLanguage("ro", ro))
    dialog(15)
    // 12 more is few in Romanian ("și încă 12"), and 120 more is other ("și încă 120 de").
    expect(await screen.findByText("și încă 12")).toBeTruthy()
    expect(screen.getByRole("dialog", { name: "Exportă pachetul lui Walter" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Exportă 15 cadre într-un singur fișier" })).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/Aproximativ 286,1 MB în total ca Editare ProRes 422 HQ\./)).toBeTruthy())
    expect(screen.getByText("Toate cadrele într-un fișier")).toBeTruthy()
    cleanup()
    dialog(123)
    expect(await screen.findByText("și încă 120 de")).toBeTruthy()
    const description = document.getElementById(screen.getByRole("dialog").getAttribute("aria-describedby") ?? "")?.textContent
    expect(description).toBe("123 de cadre într-un singur video, fiecare tăiat exact și verificat față de sursă.")
    expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Exportă 123 de cadre într-un singur fișier" })).toBeTruthy()
  })

  it("names a character nobody named in the language: its title, the folder switch, the toast", async () => {
    const export_ = { ...ro.pack.export, started_one: "Se exportă {{count}} plan cu {{name}}" }
    await act(() => inLanguage("ro", { ...ro, pack: { ...ro.pack, export: export_ }, cast: { unnamed: "Personajul {{n}}" } }))
    const files = pack(1)
    files.state = { ...files.state!, naming: { ...naming, merge: false } }
    render(<PackExportDialog pid="p" character={{ id: "c", name: null, number: 3 } as Character} pack={files} open onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByRole("dialog", { name: "Exportă pachetul lui Personajul 3" })).toBeTruthy()
    expect(screen.getByText("Într-un dosar numit după Personajul 3")).toBeTruthy()
    await screen.findByText(/Edit-grade intra codec./)
    fireEvent.click(screen.getByRole("button", { name: "Export 1 shot" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Se exportă 1 plan cu Personajul 3"))
  })

  it("counts the episodes still being analysed with the language's plural, the character's name a value", async () => {
    const analysing = (id: string) =>
      ({ id, kind: "analyze", title: plain("Analysing E01.mkv"), subtitle: null, projectId: "p", mediaId: id, exportId: null, state: "running", progress: 0.4, stage: plain("Finding characters"), etaMs: null, error: null, createdAt: 1, finishedAt: null, rev: 1 }) as const
    useActivity.getState().replaceAll([analysing("a"), analysing("b"), analysing("c")])
    dialog(15)
    const said = (text: string) => async () => {
      const id = screen.getByRole("dialog").getAttribute("aria-describedby") ?? ""
      expect(document.getElementById(id)?.textContent).toContain(text)
    }
    await waitFor(said("3 episodes are still being analysed. Shots of Walter found in them later won't be in this export."))
    cleanup()
    await act(() => inLanguage("ro", ro))
    dialog(15)
    await waitFor(said("3 episoade se analizează încă. Cadrele cu Walter găsite în ele mai târziu nu vor intra în acest export."))
  })

  it("says one episode, or one or two files of a film, in the singular and plural of each", async () => {
    const analysing = (id: string) =>
      ({ id, kind: "analyze", title: plain("Analysing E01.mkv"), subtitle: null, projectId: "p", mediaId: id, exportId: null, state: "running", progress: 0.4, stage: plain("Finding characters"), etaMs: null, error: null, createdAt: 1, finishedAt: null, rev: 1 }) as const
    const said = async (text: string) =>
      waitFor(() => {
        const id = screen.getByRole("dialog").getAttribute("aria-describedby") ?? ""
        expect(document.getElementById(id)?.textContent).toContain(text)
      })
    useActivity.getState().replaceAll([analysing("a")])
    dialog(15)
    await said("1 episode is still being analysed. Shots of Walter found in it later won't be in this export.")
    cleanup()
    queryClient.clear()
    vi.mocked(commands.projectGet).mockResolvedValue({ id: "p", kind: "movie" } as never)
    try {
      dialog(15)
      await said("1 file is still being analysed. Shots of Walter found in it later won't be in this export.")
      cleanup()
      useActivity.getState().replaceAll([analysing("a"), analysing("b")])
      dialog(15)
      await said("2 files are still being analysed. Shots of Walter found in them later won't be in this export.")
    } finally {
      vi.mocked(commands.projectGet).mockResolvedValue({ id: "p", kind: "series" } as never)
    }
  })

  it("asks the OS for a folder under the title Export to", async () => {
    dialog(3)
    vi.mocked(pickFolder).mockResolvedValue(null)
    fireEvent.click(await screen.findByRole("button", { name: "Change…" }))
    expect(pickFolder).toHaveBeenCalledWith("Export to")
  })
})
