import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { AccelInfo, ModelInfo, Platform } from "@/bindings"
import { commands } from "@/lib/ipc"
import { openExternalUrl } from "@/lib/os"
import { inLanguage } from "@/test/i18n"
import { AboutSection } from "./AboutSection"

const { machine } = vi.hoisted(() => ({
  machine: { platform: "windows" as Platform, accel: { installed: false } as Partial<AccelInfo> },
}))

const model = (id: string, purpose: string, license: string, nonCommercial: boolean, installed: boolean): ModelInfo => ({
  id,
  purpose,
  source: `${id} source`,
  license,
  nonCommercial,
  sizeBytes: 31_250_929,
  installed,
})

vi.mock("@/lib/ipc", () => ({
  commands: {
    appInfo: vi.fn(() => Promise.resolve({ version: "1.0.0", platform: machine.platform, arch: "x86_64" })),
    hardwareInfo: vi.fn(() =>
      Promise.resolve({
        runtime: machine.platform === "windows" ? "1.24.4" : "1.31.1",
        accel: { engine: "1.7.1", licence: "https://docs.nvidia.com/licence", ...machine.accel },
      }),
    ),
    accelNotices: vi.fn(() => Promise.resolve()),
    mediaTools: vi.fn(() => Promise.resolve({ ready: true, version: "8.0", path: "ffmpeg", error: null })),
    models: vi.fn(() =>
      Promise.resolve([
        model("transnetv2", "Shot boundaries", "MIT", false, true),
        model("arcface-glintr100", "Face identity embeddings", "InsightFace non-commercial research", true, false),
      ]),
    ),
  },
  errorMessage: (e: unknown) => String(e),
}))

vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ openLogs: vi.fn() }) }))
vi.mock("@/lib/os", () => ({ openExternalUrl: vi.fn(() => Promise.resolve()) }))

afterEach(cleanup)

const show = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AboutSection />
    </QueryClientProvider>,
  )

describe("AboutSection", () => {
  it("names the AI engine and what it computes through on each system", async () => {
    for (const [platform, line] of [
      ["windows", "ONNX Runtime 1.24.4 with DirectML"],
      ["macos", "ONNX Runtime 1.31.1 with Core ML"],
      ["linux", "ONNX Runtime 1.31.1"],
    ] as const) {
      machine.platform = platform
      show()
      expect(await screen.findByText(line)).toBeTruthy()
      cleanup()
    }
    machine.platform = "windows"
  })

  it("names NVIDIA's engine with its licence and notices once it is downloaded", async () => {
    const user = userEvent.setup()
    show()
    await screen.findByText("ONNX Runtime 1.24.4 with DirectML")
    expect(screen.queryByText(/TensorRT/)).toBeNull()
    cleanup()
    machine.accel = { installed: true }
    show()
    expect(await screen.findByText("NVIDIA TensorRT for RTX 1.7.1")).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "NVIDIA's licence" }))
    expect(openExternalUrl).toHaveBeenCalledWith("https://docs.nvidia.com/licence")
    await user.click(screen.getByRole("button", { name: "Acknowledgements" }))
    expect(commands.accelNotices).toHaveBeenCalledOnce()
    machine.accel = { installed: false }
  })

  it("lists the AI models with their licences and marks the non-commercial ones", async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <AboutSection />
      </QueryClientProvider>,
    )
    expect(await screen.findByText("Face identity embeddings")).toBeTruthy()
    expect(screen.getByText("InsightFace non-commercial research")).toBeTruthy()
    expect(screen.getByText("Shot boundaries")).toBeTruthy()
    // Only the restricted model carries the mark.
    expect(screen.getAllByText("Non-commercial")).toHaveLength(1)
    expect(screen.getByText(/Fast Scan uses none of them/)).toBeTruthy()
    expect(screen.getByText(/^Downloaded · 29\.8 MB/)).toBeTruthy()
    expect(screen.getByText(/^Not downloaded yet/)).toBeTruthy()
  })

  it("says why FFmpeg isn't there in the UI's language", async () => {
    await inLanguage("ro", { backend: { media: { toolsMissing: "FFmpeg lipsește." } } })
    const error = { key: "media.toolsMissing", args: {}, text: "FFmpeg is missing." }
    vi.mocked(commands.mediaTools).mockResolvedValueOnce({ ready: false, version: null, path: null, error })
    render(
      <QueryClientProvider client={new QueryClient()}>
        <AboutSection />
      </QueryClientProvider>,
    )
    expect(await screen.findByText("FFmpeg lipsește.")).toBeTruthy()
  })
})
