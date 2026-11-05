import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { ClipRequest, ExportPreset, Msg } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { pickFolder } from "@/lib/media"
import { queryClient } from "@/lib/queries"
import { inLanguage } from "@/test/i18n"
import { ExportDialog, type ExportClip } from "./ExportDialog"

// A preset as the backend may soon send it: its name is a `Msg`, said by key in the language in use.
const prores = {
  id: "prores-hq",
  name: { key: "export.preset.proresHq", text: "ProRes 422 HQ" } satisfies Msg,
  description: "Edit-grade intra codec.",
  container: "mov",
  video: { codec: "prores", profile: "hq" },
  audio: { codec: "pcm24" },
  maxHeight: null,
  hdr: "auto",
  stereo: false,
  builtin: true,
} as unknown as ExportPreset

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/lib/ipc", () => ({
  commands: {
    exportPresets: vi.fn(() => Promise.resolve([prores])),
    settingsGet: vi.fn(() => Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" })),
    settingsUpdate: vi.fn((s: unknown) => Promise.resolve(s)),
    exportDefaultDir: vi.fn(() => Promise.resolve("C:\\Library\\Exports\\Show")),
    exportPlan: vi.fn(() => Promise.resolve({ names: ["S01E01_00-00-00-00"], sizes: [{ presetId: "prores-hq", bytes: 3e8, note: null }] })),
    exportStart: vi.fn(() => Promise.resolve({ id: "e1" })),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@/lib/media", async (original) => ({ ...(await original<object>()), pickFolder: vi.fn() }))

afterEach(() => {
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
})

// A Romanian catalog would hold: titles with a few form (2 to 19), which English lacks.
const ro = {
  export: {
    title: "Exportă clipul",
    titleMany_one: "Exportă {{total}} clip",
    titleMany_few: "Exportă {{total}} clipuri",
    titleMany_other: "Exportă {{total}} de clipuri",
    description: "Fiecare clip este tăiat exact și verificat față de sursă.",
    about: "Aproximativ {{size}} în total ca {{format}}.",
    startedMany_few: "Se exportă {{total}} clipuri",
    startedMany_other: "Se exportă {{total}} de clipuri",
    started: "Se exportă „{{name}}”",
    theClip: "clipul",
    show: "Arată",
    submit: "Exportă",
    field: { fileName: "Numele fișierului" },
  },
  common: { action: { cancel: "Anulează" } },
  backend: { export: { preset: { proresHq: "Editare ProRes 422 HQ" } } },
}

const clip = (n: number): ExportClip => ({
  mediaId: "m",
  start: n * 100,
  end: n * 100 + 48,
  name: `Clip ${n}`,
  summary: "S01E03 · 00:12:03:05 – 00:12:07:11",
}) as ExportClip & ClipRequest
const clips = (n: number) => Array.from({ length: n }, (_, i) => clip(i))
const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)
const dialog = (n: number) => render(<ExportDialog projectId="p" clips={clips(n)} open onOpenChange={() => {}} />, { wrapper })

describe("the export dialog", () => {
  it("titles and starts an export as it always did, in English", async () => {
    dialog(1)
    expect(await screen.findByRole("dialog", { name: "Export clip" })).toBeTruthy()
    // The clip's own summary stays under the title; a preset sent as a Msg shows its English.
    expect(screen.getByText("S01E03 · 00:12:03:05 – 00:12:07:11")).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/About 286\.1 MB in all as ProRes 422 HQ\./)).toBeTruthy())
    fireEvent.click(screen.getByRole("button", { name: "Export" }))
    await waitFor(() => expect(commands.exportStart).toHaveBeenCalled())
    // No file name typed: the toast names the file the backend suggests.
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Exporting “S01E01_00-00-00-00”"))
    cleanup()
    dialog(3)
    expect(await screen.findByRole("dialog", { name: "Export 3 clips" })).toBeTruthy()
    expect(screen.getByText("Each clip is cut frame-exact and checked against the source.")).toBeTruthy()
    fireEvent.click(await screen.findByRole("button", { name: "Export" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Exporting 3 clips"))
  })

  it("asks the OS for a folder under the title Export to", async () => {
    dialog(2)
    vi.mocked(pickFolder).mockResolvedValue(null)
    fireEvent.click(await screen.findByRole("button", { name: "Change…" }))
    expect(pickFolder).toHaveBeenCalledWith("Export to")
  })

  it("writes a count of a thousand or more as it always did, without a separator", async () => {
    dialog(1200)
    expect(await screen.findByRole("dialog", { name: "Export 1200 clips" })).toBeTruthy()
    fireEvent.click(await screen.findByRole("button", { name: "Export" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Exporting 1200 clips"))
  })

  it("follows the language: the few form for 3 clips, the other for 25, a preset by its key, the file's name a value", async () => {
    await act(() => inLanguage("ro", ro))
    dialog(3)
    expect(await screen.findByRole("dialog", { name: "Exportă 3 clipuri" })).toBeTruthy()
    expect(screen.getByText("Fiecare clip este tăiat exact și verificat față de sursă.")).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/Aproximativ 286,1 MB în total ca Editare ProRes 422 HQ\./)).toBeTruthy())
    // A key the catalog lacks reads in English: the buttons that have no Romanian yet.
    expect(screen.getByRole("button", { name: "Anulează" })).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Exportă" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Se exportă 3 clipuri"))
    cleanup()
    dialog(25)
    expect(await screen.findByRole("dialog", { name: "Exportă 25 de clipuri" })).toBeTruthy()
    cleanup()
    // One clip: its name, typed, stays as typed inside the translated sentence.
    dialog(1)
    const user = userEvent.setup()
    await user.type(await screen.findByLabelText("Numele fișierului"), "Walter la birou")
    await user.click(screen.getByRole("button", { name: "Exportă" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Se exportă „Walter la birou”"))
  })
})
