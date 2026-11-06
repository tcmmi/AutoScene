import type * as React from "react"
import { useState } from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { type Character, type ExportPreset, FILE_PARTS, type JobInfo, NAME_PARTS, type PackNaming, type ProjectSummary } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { PackExportDialog } from "./PackExportDialog"
import { DEFAULT_SPEC } from "./spec"
import type { Pack } from "./use-pack"
import { inLanguage, plain } from "@/test/i18n"

const prores: ExportPreset = {
  id: "prores-hq",
  name: "ProRes 422 HQ",
  description: "Edit-grade intra codec.",
  container: "mov",
  video: { codec: "prores", profile: "hq" },
  audio: { codec: "pcm24" },
  maxHeight: null,
  hdr: "auto",
  stereo: false,
  builtin: true,
}
const bitPerfect: ExportPreset = {
  ...prores,
  id: "bit-perfect",
  name: "Bit-Perfect (lossless)",
  description: "Every frame exactly as in the episode.",
  video: { codec: "lossless" },
  audio: { codec: "lossless" },
}

vi.mock("@/lib/ipc", () => ({
  commands: {
    exportPresets: vi.fn(() => Promise.resolve([prores, bitPerfect])),
    settingsGet: vi.fn(() =>
      Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" }),
    ),
    settingsUpdate: vi.fn(),
    exportDefaultDir: vi.fn(() => Promise.resolve("C:\\Library\\Exports\\Show")),
    packExport: vi.fn(() => Promise.resolve({ id: "e1" })),
    projectGet: vi.fn(() => Promise.resolve({ id: "p", kind: "series" })),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@/lib/media", () => ({ pickFolder: vi.fn() }))

const walter = { id: "c", name: "Walter", number: 1 } as Character

/** A pack of three shots, as `usePack` hands it over. */
function pack(naming: PackNaming): Pack {
  return {
    state: { spec: DEFAULT_SPEC, naming },
    preview: { data: { clips: [], totals: { clips: 3, frames: 72, seconds: 3 }, stamp: "s" } },
    plan: {
      data: {
        names: ["Show_Walter_E01_00-00-00-00", "Show_Walter_E01_00-00-02-02", "Show_Walter_E01_00-00-12-02"],
        sizes: [
          { presetId: "prores-hq", bytes: 3e8 },
          {
            presetId: "bit-perfect",
            bytes: 9e8,
            apart: naming.merge
              ? {
                  key: "export.lossless.apart",
                  args: { why: { key: "export.lossless.pictures", args: {}, text: "their episodes' pictures differ in size or format" } },
                  text: "Bit-Perfect makes a file per shot here: their episodes' pictures differ in size or format.",
                }
              : null,
          },
        ],
        folder: naming.folder ? "Walter" : null,
        file: naming.merge ? "Show_Walter_scenepack" : null,
      },
      isError: false,
      error: null,
    },
    setNaming: vi.fn(),
    current: true,
  } as unknown as Pack
}

/** The pack once the backend refused its plan with `message`, as `usePack` hands it over then. */
const refused = (p: Pack, message: string): Pack => ({ ...p, plan: { data: undefined, isError: true, error: message } }) as unknown as Pack

// The backend's sentences (as_project::naming).
const unknownEp = "“{ep}” isn't a name part. Use {series}, {character}, {episode}, {index}, {tc} or {frame}."
const unclosed = "A “{” isn't closed with “}”."

const naming = (merge: boolean): PackNaming => ({
  template: "{series} {character} {episode} {tc}",
  folder: true,
  merge,
  file: "{series} {character} scenepack",
})

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

afterEach(() => {
  vi.useRealTimers()
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
  vi.restoreAllMocks()
  Reflect.deleteProperty(document, "execCommand")
  useActivity.setState({ jobs: {}, order: [] })
})

/** A job of the project "p": the analysis of the episode "m" unless told otherwise. */
const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "j",
  kind: "analyze",
  title: plain("Analysing E01.mkv"),
  subtitle: null,
  projectId: "p",
  mediaId: "m",
  exportId: null,
  state: "running",
  progress: 0.4,
  stage: plain("Finding characters"),
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
  ...o,
})

/** The dialog's description, as a screen reader hears it. */
const description = () => {
  const id = screen.getByRole("dialog").getAttribute("aria-describedby")
  return document.getElementById(id ?? "")?.textContent ?? ""
}

/** The field's whole box: label, field, buttons, problem and names (the field sits in its marks' box). */
const box = (field: HTMLElement) => field.parentElement!.parentElement!

/** The name-part button `label` under the field labelled `field`. */
const chip = (field: string, label: string) => within(box(screen.getByLabelText(field))).getByRole("button", { name: label })

/**
 * The dialog over a pack that takes what the dialog sets, as `usePack` does.
 * `refuse` is the backend: why it refuses the plan of a naming, if it does.
 */
function LiveDialog({ initial, refuse }: { initial: PackNaming; refuse?: (naming: PackNaming) => string | null }) {
  const [current, setCurrent] = useState(initial)
  const p = { ...pack(current), setNaming: setCurrent }
  const problem = refuse?.(current)
  return <PackExportDialog pid="p" character={walter} pack={problem ? refused(p, problem) : p} open onOpenChange={() => {}} />
}

/** The field's problem line: always there, read out when it changes. */
const problemLine = (field: HTMLElement) => box(field).querySelector<HTMLElement>('[aria-live="polite"]')!

/** Is `field` marked wrong, with its problem as its description? */
function marked(field: HTMLElement): string | null {
  if (field.getAttribute("aria-invalid") !== "true") {
    expect(field.hasAttribute("aria-describedby")).toBe(false)
    return null
  }
  return document.getElementById(field.getAttribute("aria-describedby")!)!.textContent
}

/** The name-part toolbar under the field labelled `field`. */
const toolbar = (field: string) => within(box(screen.getByLabelText(field))).getByRole("toolbar", { name: "Name parts" })

/** The marks behind `field`: its parts as they stand out, and a mistyped one in red. */
const marks = (field: HTMLElement) => field.parentElement!.querySelector<HTMLElement>('[aria-hidden="true"]')!

/** The engine's own typing command, stubbed: jsdom has none. */
function stubExec() {
  const exec = vi.fn<(...args: unknown[]) => boolean>(() => true)
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true })
  return exec
}

describe("PackExportDialog", () => {
  it("exports the pack into one file, with the shots' names as chapters", async () => {
    const p = pack(naming(true))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText("Show_Walter_scenepack.mov")).toBeTruthy())
    expect(screen.getByLabelText("File name")).toBeTruthy()
    expect(screen.getByLabelText("Chapter names")).toBeTruthy()
    // One file has no folder of its own.
    expect(screen.queryByText(/In a folder named after/)).toBeNull()
    expect(screen.getByText("C:\\Library\\Exports\\Show")).toBeTruthy()
    // About how big, in the format chosen; making it takes room for the
    // shots while they are joined.
    expect(screen.getByText(/About 286\.1 MB in all as ProRes 422 HQ\./)).toBeTruthy()
    expect(screen.getByText(/needs extra free space/)).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Export 3 shots into one file" }))
    await waitFor(() => expect(commands.packExport).toHaveBeenCalled())
    expect(vi.mocked(commands.packExport).mock.calls[0][2]).toMatchObject({ naming: { merge: true }, presetId: "prores-hq" })
  })

  it("switches to a file per shot", async () => {
    const p = pack(naming(true))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    fireEvent.click(await screen.findByRole("switch"))
    expect(p.setNaming).toHaveBeenCalledWith({ ...naming(true), merge: false })
  })

  it("says why Bit-Perfect makes a file per shot of episodes that differ", async () => {
    vi.mocked(commands.settingsGet).mockResolvedValueOnce({
      libraryRoot: "C:\\Library",
      exportPresetId: "bit-perfect",
      audioLanguage: "eng",
    })
    const p = pack(naming(true))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText(/makes a file per shot here: their episodes' pictures differ/)).toBeTruthy())
    expect(screen.queryByText(/needs extra free space/)).toBeNull()
    // Named, foldered and exported like a file per shot.
    expect(screen.queryByLabelText("File name")).toBeNull()
    expect(screen.getByLabelText("File names")).toBeTruthy()
    expect(screen.getByText("C:\\Library\\Exports\\Show\\Walter")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Export 3 shots" })).toBeTruthy()
  })

  it("the buttons are the backend's name parts, in its order", async () => {
    const p = pack(naming(true))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const buttons = (field: string) =>
      within(box(screen.getByLabelText(field)))
        .getAllByRole("button")
        .map((b) => b.textContent)
    const label = (part: string) => NAME_PARTS.find(([p]) => p === part)?.[1]
    expect(buttons("Chapter names")).toEqual(NAME_PARTS.map(([, l]) => l))
    expect(buttons("File name")).toEqual(FILE_PARTS.map(label))
    // Each button puts in its own part: "Number" is {index}.
    fireEvent.click(chip("Chapter names", "Number"))
    expect(p.setNaming).toHaveBeenLastCalledWith({ ...naming(true), template: `${naming(true).template} {index}` })
    fireEvent.click(chip("File name", "Character"))
    expect(p.setNaming).toHaveBeenLastCalledWith({ ...naming(true), file: `${naming(true).file}{character}` })
  })

  it("a name-part button types its part at the cursor", async () => {
    const user = userEvent.setup()
    const exec = stubExec()
    const select = vi.spyOn(HTMLInputElement.prototype, "setSelectionRange")
    const p = pack(naming(false))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const field = (await screen.findByLabelText("File names")) as HTMLInputElement
    await user.click(field)
    // "{character}" selected: Episode takes its place.
    field.setSelectionRange(9, 20)
    select.mockClear()
    await user.click(chip("File names", "Episode"))
    expect(exec).toHaveBeenCalledWith("insertText", false, "{episode}")
    expect(select).toHaveBeenCalledWith(9, 20, undefined)
    expect(select.mock.invocationCallOrder[0]).toBeLessThan(exec.mock.invocationCallOrder[0])
    // Without the engine's command the field is changed directly, and the dialog hears it.
    Reflect.deleteProperty(document, "execCommand")
    field.setSelectionRange(9, 20)
    await user.click(chip("File names", "Episode"))
    expect(p.setNaming).toHaveBeenLastCalledWith({ ...naming(false), template: "{series} {episode} {episode} {tc}" })
  })

  it("pressing a name-part button leaves the focus in the field", async () => {
    const user = userEvent.setup()
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    const field = await screen.findByLabelText("File names")
    await user.click(field)
    const blurred = vi.fn()
    field.addEventListener("blur", blurred)
    await user.click(chip("File names", "Frame"))
    expect(document.activeElement).toBe(field)
    // Nor between the buttons.
    await user.click(toolbar("File names"))
    expect(document.activeElement).toBe(field)
    // Not even for a moment: the button never took the focus.
    expect(blurred).not.toHaveBeenCalled()
  })

  it("Tab into the field and a button by keyboard keep the template", async () => {
    const user = userEvent.setup()
    const p = pack(naming(false))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const field = (await screen.findByLabelText("File names")) as HTMLInputElement
    // From the switch above it, Tab selects all of the field, as the webview does.
    screen.getAllByRole("switch")[0].focus()
    await user.tab()
    expect(document.activeElement).toBe(field)
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, field.value.length])
    await user.tab()
    expect(document.activeElement).toBe(chip("File names", "Series"))
    await user.keyboard("{Enter}")
    // The part goes at the end; the template stays.
    expect(p.setNaming).toHaveBeenLastCalledWith({ ...naming(false), template: `${naming(false).template} {series}` })
    expect(document.activeElement).toBe(field)
  })

  it("a button fills its own field, not the one with the focus", async () => {
    const user = userEvent.setup()
    const p = pack(naming(true))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const file = await screen.findByLabelText("File name")
    await user.click(file)
    await user.click(chip("Chapter names", "Frame"))
    expect(p.setNaming).toHaveBeenCalledTimes(1)
    expect(p.setNaming).toHaveBeenLastCalledWith({ ...naming(true), template: `${naming(true).template} {frame}` })
    expect(document.activeElement).toBe(screen.getByLabelText("Chapter names"))
  })

  it("each press is its own undo step", async () => {
    const user = userEvent.setup()
    const exec = stubExec()
    const select = vi.spyOn(HTMLInputElement.prototype, "setSelectionRange")
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    const field = await screen.findByLabelText("File names")
    await user.click(field)
    select.mockClear()
    for (const label of ["Frame", "Number", "Timecode"]) await user.click(chip("File names", label))
    const inserts = exec.mock.invocationCallOrder.filter((_, i) => exec.mock.calls[i][0] === "insertText")
    expect(inserts).toHaveLength(3)
    // Before each one the selection is set again: that ends the engine's
    // typing command, so Ctrl+Z takes back one part at a time.
    const selects = select.mock.invocationCallOrder
    inserts.forEach((at, i) => {
      expect(selects.filter((s) => s < at && (i === 0 || s > inserts[i - 1]))).toHaveLength(1)
    })
  })

  it("the name parts are a toolbar", async () => {
    const user = userEvent.setup()
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    const field = await screen.findByLabelText("File names")
    const bar = screen.getByRole("toolbar", { name: "Name parts" })
    // It says which field its parts go into.
    expect(bar.getAttribute("aria-controls")).toBe(field.id)
    const buttons = within(bar).getAllByRole("button")
    const stops = () => buttons.map((b) => b.tabIndex)
    const focused = (label: string) => waitFor(() => expect(document.activeElement).toBe(within(bar).getByRole("button", { name: label })))
    field.focus()
    await user.tab()
    await focused("Series")
    expect(stops()).toEqual([0, -1, -1, -1, -1, -1])
    // The arrows move the focus, and the one stop goes with it.
    await user.keyboard("{ArrowRight}")
    await focused("Character")
    expect(stops()).toEqual([-1, 0, -1, -1, -1, -1])
    await user.keyboard("{End}")
    await focused("Frame")
    // Past the last, the first.
    await user.keyboard("{ArrowRight}")
    await focused("Series")
    await user.keyboard("{ArrowLeft}")
    await focused("Frame")
    await user.keyboard("{Home}")
    await focused("Series")
    expect(stops()).toEqual([0, -1, -1, -1, -1, -1])
  })

  it("a focused button says its part and what it gives", async () => {
    const user = userEvent.setup()
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    const field = await screen.findByLabelText("File names")
    field.focus()
    await user.tab()
    await user.keyboard("{End}")
    await waitFor(() => expect(document.activeElement).toBe(chip("File names", "Frame")))
    await user.keyboard("{ArrowLeft}")
    const timecode = chip("File names", "Timecode")
    await waitFor(() => expect(document.activeElement).toBe(timecode))
    // The spelling to type by hand, in mono, and what it gives; read out with the button.
    const tip = await screen.findByRole("tooltip")
    expect(tip.textContent).toBe("{tc}Where the shot starts, as 00-12-03-05")
    expect(within(tip).getByText("{tc}").className).toContain("font-mono")
    expect(timecode.getAttribute("aria-describedby")).toBe(tip.id)
    // Every part has its own.
    await user.keyboard("{Home}")
    await waitFor(() => expect(screen.getByRole("tooltip").textContent).toBe("{series}The project's title"))
  })

  it("Enter on a button puts the part in and returns to the field", async () => {
    const user = userEvent.setup()
    render(<LiveDialog initial={naming(false)} />, { wrapper })
    const field = (await screen.findByLabelText("File names")) as HTMLInputElement
    await user.click(field)
    // The caret after "{series}"; then over to Episode by keyboard.
    field.setSelectionRange(8, 8)
    await user.tab()
    await user.keyboard("{ArrowRight}{ArrowRight}")
    await waitFor(() => expect(document.activeElement).toBe(chip("File names", "Episode")))
    expect(screen.getByRole("tooltip").textContent).toContain("{episode}")
    await user.keyboard("{Enter}")
    expect(field.value).toBe("{series} {episode} {character} {episode} {tc}")
    expect(document.activeElement).toBe(field)
    expect([field.selectionStart, field.selectionEnd]).toEqual([18, 18])
    // The tooltip closes, so the names below show again.
    await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull())
    // Tab goes back to the button last used; Space works like Enter.
    await user.tab()
    expect(document.activeElement).toBe(chip("File names", "Episode"))
    await user.keyboard(" ")
    expect(field.value).toBe("{series} {episode} {episode} {character} {episode} {tc}")
    expect(document.activeElement).toBe(field)
  })

  it("the buttons are one Tab stop", async () => {
    const user = userEvent.setup()
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(true))} open onOpenChange={() => {}} />, { wrapper })
    const file = await screen.findByLabelText("File name")
    const chapters = screen.getByLabelText("Chapter names")
    const format = screen.getByRole("combobox", { name: "Export format" })
    file.focus()
    // Field, its parts, the next field, its parts, Format: five stops for two fields and eight buttons.
    const order: Element[] = [file, chip("File name", "Series"), chapters, chip("Chapter names", "Series"), format]
    for (const expected of order.slice(1)) {
      await user.tab()
      expect(document.activeElement).toBe(expected)
    }
    // And back the same way: the parts are one stop that way too.
    for (const expected of order.slice(0, -1).reverse()) {
      await user.tab({ shift: true })
      expect(document.activeElement).toBe(expected)
    }
  })

  it("says why Bit-Perfect makes a file per shot in the UI's language, its reason included", async () => {
    await inLanguage("ro", {
      backend: { export: { lossless: { apart: "Bit-Perfect face câte un fișier: {{why}}.", pictures: "imaginile diferă" } } },
    })
    vi.mocked(commands.settingsGet).mockResolvedValueOnce({
      libraryRoot: "C:\\Library",
      exportPresetId: "bit-perfect",
      audioLanguage: "eng",
    })
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(true))} open onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByText("Bit-Perfect face câte un fișier: imaginile diferă.")).toBeTruthy()
  })

  it.each([
    ["allFfv1", "These pictures can't be stored exactly as HEVC.", "Aceste imagini rămân FFV1."],
    ["someFfv1", "Some of these pictures can't be stored exactly as HEVC.", "Unele imagini rămân FFV1."],
  ])("says the format's note in the UI's language: %s", async (key, text, romanian) => {
    await inLanguage("ro", { backend: { export: { lossless: { [key]: romanian } } } })
    vi.mocked(commands.settingsGet).mockResolvedValueOnce({
      libraryRoot: "C:\\Library",
      exportPresetId: "bit-perfect",
      audioLanguage: "eng",
    })
    const p = pack(naming(false))
    const data = p.plan.data as NonNullable<Pack["plan"]["data"]>
    const note = { key: `export.lossless.${key}`, args: {}, text }
    const noted = { ...p, plan: { ...p.plan, data: { ...data, sizes: data.sizes.map((s) => ({ ...s, note })) } } } as Pack
    render(<PackExportDialog pid="p" character={walter} pack={noted} open onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByText(romanian)).toBeTruthy()
    expect(document.body.textContent).not.toContain("[object Object]")
  })

  it("says the format and what it is for in the UI's language", async () => {
    await inLanguage("ro", { backend: { fixture: { name: "ProRes românesc (montaj)", description: "Pentru montaj." } } })
    vi.mocked(commands.exportPresets).mockResolvedValueOnce([
      {
        ...prores,
        nameMsg: { key: "fixture.name", text: prores.name },
        descriptionMsg: { key: "fixture.description", text: prores.description },
      },
    ])
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByText(/^Pentru montaj\./)).toBeTruthy()
    expect(await screen.findByText(/in all as ProRes românesc\.$/)).toBeTruthy()
    expect(screen.getByRole("combobox", { name: "Export format" }).textContent).toContain("ProRes românesc (montaj)")
  })

  it("a broken template keeps the dialog as it was", async () => {
    vi.mocked(commands.settingsGet).mockResolvedValueOnce({
      libraryRoot: "C:\\Library",
      exportPresetId: "bit-perfect",
      audioLanguage: "eng",
    })
    // Bit-Perfect over episodes that differ: a file per shot, though "one file" is on.
    const typo = { ...naming(true), template: "{series} {ep}" }
    const p = pack(typo)
    const view = render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const apart = /makes a file per shot here: their episodes' pictures differ/
    await waitFor(() => expect(screen.getByText(apart)).toBeTruthy())
    const field = screen.getByLabelText("File names")
    field.focus()
    // The backend refuses the template: the plan has nothing to say any more.
    view.rerender(<PackExportDialog pid="p" character={walter} pack={refused(p, unknownEp)} open onOpenChange={() => {}} />)
    expect(marked(field)).toBe(unknownEp)
    // Still a file per shot, with the same notes; no "File name" field takes the focus.
    expect(screen.queryByLabelText("File name")).toBeNull()
    expect(screen.getByLabelText("File names")).toBe(field)
    expect(screen.getByText(apart)).toBeTruthy()
    expect(screen.getByText(/in all as Bit-Perfect/)).toBeTruthy()
    expect(screen.getByText("C:\\Library\\Exports\\Show\\Walter")).toBeTruthy()
    expect(document.activeElement).toBe(field)
    expect(screen.getByRole("button", { name: "Export 3 shots" })).toHaveProperty("disabled", true)
  })

  it("a file name refused while Bit-Perfect makes a file per shot keeps its field", async () => {
    vi.mocked(commands.settingsGet).mockResolvedValueOnce({
      libraryRoot: "C:\\Library",
      exportPresetId: "bit-perfect",
      audioLanguage: "eng",
    })
    const view = render(<PackExportDialog pid="p" character={walter} pack={pack(naming(true))} open onOpenChange={() => {}} />, {
      wrapper,
    })
    await waitFor(() => expect(screen.getByText(/makes a file per shot here/)).toBeTruthy())
    expect(screen.queryByLabelText("File name")).toBeNull()
    // Typed under another preset: the backend checks the one file's name even so.
    const tc = "“{tc}” names a shot, not the file. The file's name can use {series} and {character}."
    const bad = pack({ ...naming(true), file: "{series} {tc}" })
    view.rerender(<PackExportDialog pid="p" character={walter} pack={refused(bad, tc)} open onOpenChange={() => {}} />)
    // Its field is there to mend it, with the reason under it, said once.
    const file = screen.getByLabelText("File name")
    expect(marked(file)).toBe(tc)
    expect(screen.getAllByText(tc)).toHaveLength(1)
    expect(marked(screen.getByLabelText("File names"))).toBeNull()
    expect(screen.getByText(/makes a file per shot here/)).toBeTruthy()
    expect(screen.getByRole("button", { name: "Export 3 shots" })).toHaveProperty("disabled", true)
    // Mended there, it stays under the keys: the next one still goes into it.
    file.focus()
    fireEvent.change(file, { target: { value: naming(true).file } })
    expect(bad.setNaming).toHaveBeenLastCalledWith(naming(true))
    view.rerender(<PackExportDialog pid="p" character={walter} pack={pack(naming(true))} open onOpenChange={() => {}} />)
    expect(screen.getByLabelText("File name")).toBe(file)
    expect(document.activeElement).toBe(file)
    expect(marked(file)).toBeNull()
    expect(screen.getByRole("button", { name: "Export 3 shots" })).toHaveProperty("disabled", false)
    // Nor does it go when the focus leaves it: nothing below moves under the pointer.
    act(() => screen.getByLabelText("File names").focus())
    expect(screen.getByLabelText("File name")).toBe(file)
    expect(document.activeElement).toBe(screen.getByLabelText("File names"))
  })

  it("switching to one file leaves the focus on the switch", async () => {
    const user = userEvent.setup()
    // Opened on a file per shot: its field has the focus.
    render(<LiveDialog initial={naming(false)} />, { wrapper })
    const names = await screen.findByLabelText("File names")
    await waitFor(() => expect(document.activeElement).toBe(names))
    const toggle = screen.getAllByRole("switch")[0]
    await user.click(toggle)
    expect(screen.getByLabelText("File name")).toBeTruthy()
    expect(document.activeElement).toBe(toggle)
    cleanup()
    // Opened on one file: off and on again, the field that shows up again doesn't take it either.
    render(<LiveDialog initial={naming(true)} />, { wrapper })
    const file = await screen.findByLabelText("File name")
    await waitFor(() => expect(document.activeElement).toBe(file))
    const again = screen.getAllByRole("switch")[0]
    await user.click(again)
    expect(screen.queryByLabelText("File name")).toBeNull()
    await user.click(again)
    expect(screen.getByLabelText("File name")).toBeTruthy()
    expect(document.activeElement).toBe(again)
  })

  it("only the field the problem is in turns red", async () => {
    const p = refused(pack({ ...naming(true), template: "{series} {ep}" }), unknownEp)
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const chapters = await screen.findByLabelText("Chapter names")
    expect(marked(chapters)).toBe(unknownEp)
    expect(marked(screen.getByLabelText("File name"))).toBeNull()
    // Said once, under its field; nothing above the buttons.
    expect(screen.getAllByText(unknownEp)).toHaveLength(1)
    expect(problemLine(chapters).textContent).toBe(unknownEp)
    expect(screen.getByRole("button", { name: "Export 3 shots into one file" })).toHaveProperty("disabled", true)
  })

  it("a file name with a shot's part shows its problem under the file name", async () => {
    const tc = "“{tc}” names a shot, not the file. The file's name can use {series} and {character}."
    // Both are wrong: the backend checks the one file's name first.
    const p = refused(pack({ ...naming(true), file: "{series} {tc}", template: "{series} {ep}" }), tc)
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const file = await screen.findByLabelText("File name")
    expect(marked(file)).toBe(tc)
    expect(marked(screen.getByLabelText("Chapter names"))).toBeNull()
    cleanup()
    // "{ser {tc}" is one unknown part to the backend (a "{" runs to the first "}"): still the file's.
    const ser = "“{ser {tc}}” isn't a name part. Use {series}, {character}, {episode}, {index}, {tc} or {frame}."
    render(
      <PackExportDialog pid="p" character={walter} pack={refused(pack({ ...naming(true), file: "{ser {tc}" }), ser)} open onOpenChange={() => {}} />,
      { wrapper },
    )
    expect(marked(await screen.findByLabelText("File name"))).toBe(ser)
    expect(marked(screen.getByLabelText("Chapter names"))).toBeNull()
  })

  it("a problem that isn't the template's shows above the buttons", async () => {
    const p = refused(pack(naming(true)), "That character is gone.")
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const line = (await screen.findByText("That character is gone.")).closest<HTMLElement>('[aria-live="polite"]')!
    expect(line).toBeTruthy()
    expect(marked(screen.getByLabelText("File name"))).toBeNull()
    expect(marked(screen.getByLabelText("Chapter names"))).toBeNull()
    const exportButton = screen.getByRole("button", { name: "Export 3 shots into one file" })
    expect(line.compareDocumentPosition(exportButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(exportButton).toHaveProperty("disabled", true)
  })

  it("a part still being typed isn't an error yet", async () => {
    // The backend: an unclosed part, else {ep}.
    const refuse = ({ template }: PackNaming) => (/\{[^}]*$/.test(template) ? unclosed : template.includes("{ep}") ? unknownEp : null)
    render(<LiveDialog initial={naming(false)} refuse={refuse} />, { wrapper })
    const field = (await screen.findByLabelText("File names")) as HTMLInputElement
    await waitFor(() => expect(document.activeElement).toBe(field))
    // The clock is the test's. (user-event's wrapper waits on a timer of its
    // own, so the keys go in as change events; the caret ends after them.)
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] })
    const type = (value: string) => fireEvent.change(field, { target: { value } })
    const before = naming(false).template
    const quiet = () => {
      expect(marked(field)).toBeNull()
      expect(screen.queryByText(unclosed)).toBeNull()
      expect(screen.queryByText(unknownEp)).toBeNull()
    }
    // "{ep", the caret in it: no red yet, and the names stay.
    type(`${before} {ep`)
    expect(field.selectionStart).toBe(field.value.length)
    quiet()
    expect(screen.getByText("Show_Walter_E01_00-00-00-00")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Export 3 shots" })).toHaveProperty("disabled", true)
    // A second after the last key it is.
    act(() => vi.advanceTimersByTime(999))
    quiet()
    act(() => vi.advanceTimersByTime(1))
    expect(marked(field)).toBe(unclosed)
    expect(screen.queryByText("Show_Walter_E01_00-00-00-00")).toBeNull()
    // Closed, the part is done: wrong at once.
    type(`${before} {ep}`)
    expect(marked(field)).toBe(unknownEp)
    // Open again and typed in: quiet; on leaving the field, red.
    type(`${before} {ep`)
    quiet()
    act(() => field.blur())
    expect(marked(field)).toBe(unclosed)
    // Typed in again, then the caret leaves the part: red.
    act(() => field.focus())
    type(`${before} {epi`)
    quiet()
    field.setSelectionRange(0, 0)
    fireEvent.keyUp(field, { key: "Home" })
    expect(marked(field)).toBe(unclosed)
  })

  it("the message is announced", async () => {
    const p = pack(naming(false))
    const view = render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    const field = await screen.findByLabelText("File names")
    // There before the message, empty, and not the field's description: the names aren't one.
    const line = problemLine(field)
    expect(line.textContent).toBe("")
    expect(field.hasAttribute("aria-describedby")).toBe(false)
    expect(screen.getByText("Show_Walter_E01_00-00-00-00")).toBeTruthy()
    view.rerender(
      <PackExportDialog
        pid="p"
        character={walter}
        pack={refused(pack({ ...naming(false), template: "{series} {ep}" }), unknownEp)}
        open
        onOpenChange={() => {}}
      />,
    )
    // The same line now says it, and describes the field.
    expect(problemLine(field)).toBe(line)
    expect(line.textContent).toBe(unknownEp)
    expect(field.getAttribute("aria-describedby")).toBe(line.id)
    // The dialog's own line is live too, and stays empty.
    const lines = screen.getByRole("dialog").querySelectorAll('[aria-live="polite"]')
    expect([...lines].map((l) => l.textContent)).toEqual([unknownEp, ""])
  })

  it("the parts stand out in the field", async () => {
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(true))} open onOpenChange={() => {}} />, { wrapper })
    for (const [label, value, parts] of [
      ["Chapter names", "{series} {character} {episode} {tc}", ["{series}", "{character}", "{episode}", "{tc}"]],
      ["File name", "{series} {character} scenepack", ["{series}", "{character}"]],
    ] as const) {
      const layer = marks(await screen.findByLabelText(label))
      // One inline span holds the pieces, as one line of text: laid out as
      // flex items, the spaces between parts would drop out and the marks shift.
      expect(layer.childNodes).toHaveLength(1)
      expect((layer.firstChild as HTMLElement).tagName).toBe("SPAN")
      expect([...layer.querySelectorAll('[data-part="part"]')].map((m) => m.textContent)).toEqual(parts)
      expect(layer.querySelectorAll("[data-bad]")).toHaveLength(0)
      // Under the field's own text, letter for letter.
      expect(layer.textContent).toBe(value)
      expect(layer.compareDocumentPosition(screen.getByLabelText(label)) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    }
  })

  it("a mistyped part is marked where it is", async () => {
    // The backend names the first wrong part: here "{ep}" comes before any unclosed one.
    const refuse = ({ template }: PackNaming) => (template.includes("{ep}") ? unknownEp : /\{[^}]*$/.test(template) ? unclosed : null)
    const before = "{series} {ep} {tc}"
    render(<LiveDialog initial={{ ...naming(false), template: before }} refuse={refuse} />, { wrapper })
    const field = (await screen.findByLabelText("File names")) as HTMLInputElement
    const layer = marks(field)
    const bad = () => [...layer.querySelectorAll("[data-bad]")].map((m) => [m.getAttribute("data-part"), m.textContent])
    // Red where it is; the parts around it stay as they are.
    expect(bad()).toEqual([["unknown", "{ep}"]])
    expect([...layer.querySelectorAll('[data-part="part"]')].map((m) => m.textContent)).toEqual(["{series}", "{tc}"])
    expect(layer.querySelector("[data-bad]")!.className).toMatch(/decoration-wavy/)
    // A part still being typed isn't marked: "{x" with the caret in it. "{ep}" still is.
    await waitFor(() => expect(document.activeElement).toBe(field))
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] })
    fireEvent.change(field, { target: { value: `${before} {x` } })
    expect(bad()).toEqual([["unknown", "{ep}"]])
    expect(layer.querySelector('[data-part="unclosed"]')!.textContent).toBe("{x")
    // A second after the last key it is.
    act(() => vi.advanceTimersByTime(1000))
    expect(bad()).toEqual([
      ["unknown", "{ep}"],
      ["unclosed", "{x"],
    ])
    // The only wrong part, being typed: neither it nor the field is red yet.
    fireEvent.change(field, { target: { value: "{series} {x" } })
    expect(bad()).toEqual([])
    expect(field.getAttribute("aria-invalid")).toBeNull()
    // Mended: nothing red.
    fireEvent.change(field, { target: { value: "{series} {episode} {tc}" } })
    expect(bad()).toEqual([])
  })

  it("the marks follow the field's scroll", async () => {
    render(<LiveDialog initial={naming(false)} />, { wrapper })
    const field = await screen.findByLabelText("File names")
    const layer = marks(field)
    const scrolled = vi.fn()
    Object.defineProperty(layer, "scrollLeft", { configurable: true, get: () => 0, set: scrolled })
    Object.defineProperty(field, "scrollLeft", { configurable: true, get: () => 40 })
    fireEvent.scroll(field)
    expect(scrolled).toHaveBeenLastCalledWith(40)
    // And after a change of the value, before it paints.
    Object.defineProperty(field, "scrollLeft", { configurable: true, get: () => 72 })
    fireEvent.change(field, { target: { value: `${naming(false).template} {frame}` } })
    expect(scrolled).toHaveBeenLastCalledWith(72)
  })

  it("says episodes still being analysed aren't in it", async () => {
    useActivity.getState().replaceAll([
      // One episode started over: its old job still stops while the new one
      // waits. It is one episode.
      job({ id: "old" }),
      job({ id: "new", state: "queued", progress: null, createdAt: 2 }),
      job({ id: "other", mediaId: "n", createdAt: 3 }),
      // One still being indexed: its analysis starts once that is done.
      job({ id: "index", kind: "ingest", mediaId: "y", createdAt: 6 }),
      // None of these: done, another project's, not an analysis.
      job({ id: "done", mediaId: "o", state: "done", progress: 1, finishedAt: 4, createdAt: 4 }),
      job({ id: "elsewhere", projectId: "q", mediaId: "x", createdAt: 5 }),
      job({ id: "export", kind: "export", mediaId: null, exportId: "e", createdAt: 7 }),
    ])
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(true))} open onOpenChange={() => {}} />, { wrapper })
    const missing = "3 episodes are still being analysed. Shots of Walter found in them later won't be in this export."
    await waitFor(() => expect(description()).toContain(missing))
    expect(description()).toBe(`3 shots in one video, each cut frame-exact and checked against the source. ${missing}`)
    // Said as it opened: an analysis ending meanwhile doesn't move the dialog.
    act(() => useActivity.getState().upsert(job({ id: "other", mediaId: "n", createdAt: 3, state: "done", finishedAt: 8, rev: 2 })))
    expect(description()).toContain(missing)
    // And not a word of it while nothing is analysed.
    cleanup()
    useActivity.setState({ jobs: {}, order: [] })
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByRole("button", { name: "Export 3 shots" })).toBeTruthy())
    expect(description()).toBe("3 shots, each cut frame-exact and checked against the source.")
    // As tall as it gets, it scrolls inside: at the smallest window its
    // buttons stay in the window.
    expect(screen.getByRole("dialog").className).toMatch(/max-h-\[calc\(100vh-48px\)\].*overflow-y-auto/)
  })

  it("counts only the episodes the pack takes", async () => {
    useActivity.getState().replaceAll([
      job({ id: "a", mediaId: "n" }),
      job({ id: "b", kind: "ingest", mediaId: "y", createdAt: 2 }),
    ])
    const only = (media: string[]): Pack => {
      const p = pack(naming(false))
      return { ...p, state: { spec: { ...DEFAULT_SPEC, media }, naming: naming(false) } }
    }
    // Filtered to a finished episode: the others aren't in this export anyway.
    render(<PackExportDialog pid="p" character={walter} pack={only(["o"])} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByRole("button", { name: "Export 3 shots" })).toBeTruthy())
    expect(description()).toBe("3 shots, each cut frame-exact and checked against the source.")
    cleanup()
    // Filtered to one still being indexed and a finished one: that one.
    render(<PackExportDialog pid="p" character={walter} pack={only(["o", "y"])} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() =>
      expect(description()).toBe(
        "3 shots, each cut frame-exact and checked against the source. 1 episode is still being analysed. Shots of Walter found in it later won't be in this export.",
      ),
    )
  })

  it("says files for a movie project", async () => {
    vi.mocked(commands.projectGet).mockResolvedValueOnce({ id: "p", kind: "movie" } as ProjectSummary)
    useActivity.getState().replaceAll([job({ id: "film" })])
    render(<PackExportDialog pid="p" character={walter} pack={pack(naming(false))} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() =>
      expect(description()).toBe(
        "3 shots, each cut frame-exact and checked against the source. 1 file is still being analysed. Shots of Walter found in it later won't be in this export.",
      ),
    )
  })

  it("names a file per shot, in the character's folder", async () => {
    const p = pack(naming(false))
    render(<PackExportDialog pid="p" character={walter} pack={p} open onOpenChange={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText("C:\\Library\\Exports\\Show\\Walter")).toBeTruthy())
    expect(screen.queryByLabelText("File name")).toBeNull()
    expect(screen.getByLabelText("File names")).toBeTruthy()
    expect(screen.getByText(/In a folder named after Walter/)).toBeTruthy()
    expect(screen.getByRole("button", { name: "Export 3 shots" })).toBeTruthy()
  })
})
