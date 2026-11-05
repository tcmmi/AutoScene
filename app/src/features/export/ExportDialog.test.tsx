import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { ExportPreset } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { queryClient } from "@/lib/queries"
import { inLanguage } from "@/test/i18n"
import { ExportDialog } from "./ExportDialog"

const bitPerfect = {
  id: "bit-perfect",
  name: "Bit-Perfect (lossless)",
  description: "Every frame exactly as in the episode.",
} as ExportPreset

vi.mock("@/lib/ipc", () => ({
  commands: {
    exportPresets: vi.fn(() => Promise.resolve([bitPerfect])),
    settingsGet: vi.fn(() => Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "bit-perfect", audioLanguage: "eng" })),
    exportDefaultDir: vi.fn(() => Promise.resolve("C:\\Library\\Exports\\Show")),
    exportPlan: vi.fn(() =>
      Promise.resolve({
        names: ["Show_E01"],
        sizes: [
          {
            presetId: "bit-perfect",
            bytes: 9e8,
            note: { key: "export.lossless.allFfv1", args: {}, text: "These pictures can't be stored exactly as HEVC." },
          },
        ],
      }),
    ),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@/lib/media", () => ({ pickFolder: vi.fn() }))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

afterEach(() => {
  cleanup()
  queryClient.clear()
})

describe("ExportDialog", () => {
  it("says the format's note in the UI's language", async () => {
    await inLanguage("ro", { backend: { export: { lossless: { allFfv1: "Aceste imagini rămân FFV1." } } } })
    const clips = [{ mediaId: "m", start: 0, end: 24, name: null, summary: "E01" }]
    render(<ExportDialog projectId="p" clips={clips} open onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByText("Aceste imagini rămân FFV1.")).toBeTruthy()
    expect(document.body.textContent).not.toContain("[object Object]")
  })
})
