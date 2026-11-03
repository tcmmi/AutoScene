import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { type Character, type ExportPreset, FILE_PARTS, NAME_PARTS, type PackNaming } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { PackExportDialog } from "./PackExportDialog"
import { DEFAULT_SPEC } from "./spec"
import type { Pack } from "./use-pack"

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
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@/lib/media", () => ({ pickFolder: vi.fn() }))

const walter = { id: "c", label: "Walter" } as Character

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
            apart: naming.merge ? "Bit-Perfect makes a file per shot here: their episodes' pictures differ in size or format." : null,
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
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
  vi.restoreAllMocks()
  Reflect.deleteProperty(document, "execCommand")
})

/** The name-part button `label` under the field labelled `field`. */
const chip = (field: string, label: string) =>
  within(screen.getByLabelText(field).parentElement!).getByRole("button", { name: label })

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
      within(screen.getByLabelText(field).parentElement!)
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
