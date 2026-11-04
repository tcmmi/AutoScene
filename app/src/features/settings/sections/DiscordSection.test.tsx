import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, DiscordLines, DiscordRpcSettings } from "@/bindings"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { DiscordSection } from "./DiscordSection"

const { state } = vi.hoisted(() => ({
  state: { settings: { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" } as AppSettings },
}))

vi.mock("@/lib/ipc", async (original) => ({
  ...(await original<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(async () => state.settings),
    settingsUpdate: vi.fn(async (next: AppSettings) => (state.settings = next)),
    // As `discord::preview` says it.
    discordPreview: vi.fn(async (show: DiscordRpcSettings) => [
      { details: show.showProject ? "Your project" : "Working on a project", state: "Reviewing an episode" },
      { details: show.showProject ? "Your project" : "Working on a project", state: "Making a scenepack" },
    ]),
  },
}))

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }))

const open = () =>
  render(
    <QueryClientProvider client={queryClient}>
      <DiscordSection />
    </QueryClientProvider>,
  )

const saved = () => state.settings.discordRpc

beforeEach(() => {
  state.settings = { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" }
  queryClient.clear()
  vi.clearAllMocks()
})
afterEach(cleanup)

describe("Settings → Discord", () => {
  it("can't be touched before the settings are here", async () => {
    let settingsHere!: (settings: AppSettings) => void
    vi.mocked(commands.settingsGet).mockReturnValueOnce(new Promise((here) => (settingsHere = here)))
    open()
    const main = screen.getByRole("switch", { name: "Show on Discord" })
    expect(main.hasAttribute("disabled")).toBe(true)
    await act(async () => settingsHere(state.settings))
    await waitFor(() => expect(main.hasAttribute("disabled")).toBe(false))
  })

  it("starts off, with one switch and what people would see", async () => {
    open()
    const main = await screen.findByRole("switch", { name: "Show on Discord" })
    expect(main.getAttribute("aria-checked")).toBe("false")
    expect(screen.getAllByRole("switch")).toHaveLength(1)
    expect(screen.getByText("What people would see")).toBeTruthy()
    expect(screen.queryByText("What people see")).toBeNull()
    expect(await screen.findByText("Reviewing an episode")).toBeTruthy()
    expect(screen.getByText("Making a scenepack")).toBeTruthy()
    expect(commands.settingsUpdate).not.toHaveBeenCalled()
  })

  it("turning it on lets out no name, and offers each with its own switch", async () => {
    const user = userEvent.setup()
    open()
    await waitFor(() => expect(screen.getByRole("switch", { name: "Show on Discord" }).hasAttribute("disabled")).toBe(false))
    await user.click(screen.getByRole("switch", { name: "Show on Discord" }))

    for (const name of ["Project", "Episode or file", "Character"]) {
      const row = await screen.findByRole("switch", { name })
      expect(row.getAttribute("aria-checked")).toBe("false")
    }
    expect(screen.getByText("What people see")).toBeTruthy()
    await waitFor(() => expect(saved()).toEqual({ enabled: true }))
  })

  it("a name's switch changes that name only, keeps the rest of the settings, and shows at once", async () => {
    state.settings = { ...state.settings, discordRpc: { enabled: true, showEpisode: true } }
    const user = userEvent.setup()
    open()
    await user.click(await screen.findByRole("switch", { name: "Project" }))

    await waitFor(() => expect(saved()).toEqual({ enabled: true, showEpisode: true, showProject: true }))
    expect(state.settings.libraryRoot).toBe("C:\\Library")
    expect((await screen.findAllByText("Your project")).length).toBe(2)
  })

  it("turning it off keeps a chosen name in reach: on again sends it at once", async () => {
    state.settings = { ...state.settings, discordRpc: { enabled: true, showProject: true } }
    const user = userEvent.setup()
    open()
    await screen.findByRole("switch", { name: "Project" })
    await user.click(screen.getByRole("switch", { name: "Show on Discord" }))
    await waitFor(() => expect(saved()).toEqual({ enabled: false, showProject: true }))

    await user.click(screen.getByRole("switch", { name: "Project" }))
    await waitFor(() => expect(saved()).toEqual({ enabled: false, showProject: false }))
  })

  it("turning it off with no name chosen leaves the one switch", async () => {
    state.settings = { ...state.settings, discordRpc: { enabled: true } }
    const user = userEvent.setup()
    open()
    await screen.findByRole("switch", { name: "Project" })
    await user.click(screen.getByRole("switch", { name: "Show on Discord" }))

    await waitFor(() => expect(screen.getAllByRole("switch")).toHaveLength(1))
    await waitFor(() => expect(saved()).toEqual({ enabled: false }))
  })

  it("two quick switches both count, while the first save is still on its way", async () => {
    state.settings = { ...state.settings, discordRpc: { enabled: true, showProject: true, showEpisode: true } }
    const saves: (() => void)[] = []
    vi.mocked(commands.settingsUpdate).mockImplementation(
      (next) =>
        new Promise((done) => {
          saves.push(() => done((state.settings = next)))
        }),
    )
    const user = userEvent.setup()
    open()
    const project = await screen.findByRole("switch", { name: "Project" })
    const episode = screen.getByRole("switch", { name: "Episode or file" })
    await user.click(project)
    await user.click(episode)

    // Both moved at once, and one save at a time goes out.
    expect(project.getAttribute("aria-checked")).toBe("false")
    expect(episode.getAttribute("aria-checked")).toBe("false")
    await waitFor(() => expect(saves).toHaveLength(1))
    await act(async () => saves[0]())
    await waitFor(() => expect(saves).toHaveLength(2))
    await act(async () => saves[1]())

    await waitFor(() => expect(saved()).toEqual({ enabled: true, showProject: false, showEpisode: false }))
    expect(vi.mocked(commands.settingsUpdate).mock.lastCall?.[0].discordRpc).toEqual(saved())
    expect(project.getAttribute("aria-checked")).toBe("false")
    expect(episode.getAttribute("aria-checked")).toBe("false")
  })

  it("a save that fails says so, and the switch goes back", async () => {
    state.settings = { ...state.settings, discordRpc: { enabled: true } }
    vi.mocked(commands.settingsUpdate).mockRejectedValueOnce(new Error("disk full"))
    const user = userEvent.setup()
    open()
    const project = await screen.findByRole("switch", { name: "Project" })
    await user.click(project)

    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't save the setting", expect.anything()))
    await waitFor(() => expect(screen.getByRole("switch", { name: "Project" }).getAttribute("aria-checked")).toBe("false"))
    expect(saved()).toEqual({ enabled: true })
  })

  it("keeps the last status on screen while a switch's answer is on its way", async () => {
    state.settings = { ...state.settings, discordRpc: { enabled: true } }
    const user = userEvent.setup()
    open()
    await screen.findByText("Reviewing an episode")
    let answer!: (lines: DiscordLines[]) => void
    vi.mocked(commands.discordPreview).mockReturnValueOnce(new Promise((here) => (answer = here)))
    await user.click(screen.getByRole("switch", { name: "Project" }))

    expect(screen.getByText("Reviewing an episode")).toBeTruthy()
    await act(async () => answer([{ details: "Your project", state: "Episode 1" }]))
    expect(await screen.findByText("Episode 1")).toBeTruthy()
  })
})
