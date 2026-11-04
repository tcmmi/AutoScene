import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PluginInfo, SettingSpec, SettingValue } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { pluginInfo } from "@/test/plugins"
import { PluginSettingsSection } from "./PluginSettingsSection"

const { state } = vi.hoisted(() => ({
  state: { plugin: null as PluginInfo | null, values: {} as Record<string, SettingValue> | Promise<Record<string, SettingValue>> },
}))

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(() => Promise.resolve({ allowUnreviewedPlugins: false })),
    pluginsList: vi.fn(() => Promise.resolve(state.plugin ? [state.plugin] : [])),
    pluginSettingsGet: vi.fn(() => Promise.resolve(state.values)),
    pluginSettingsSet: vi.fn(async (_: string, v: Record<string, SettingValue>) => ({ ...(await state.values), ...v })),
    pluginSetAfterExport: vi.fn(),
    pluginSetEvent: vi.fn(),
    pluginSecretSet: vi.fn(),
    pluginSecretClear: vi.fn(),
  },
}))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn() }) }))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
const { reveal } = vi.hoisted(() => ({ reveal: vi.fn() }))
vi.mock("@/lib/os", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/os")>()), reveal }))
const { platform } = vi.hoisted(() => ({ platform: { mac: false } }))
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  get isMac() {
    return platform.mac
  },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

const number = (key: string, title: string, o: { min?: number; max?: number; integer?: boolean }): SettingSpec => ({
  key,
  title,
  description: null,
  kind: { type: "number", default: null, min: o.min ?? null, max: o.max ?? null, step: null, integer: o.integer ?? false },
})

const select = (key: string, title: string, values: string[]): SettingSpec => ({
  key,
  title,
  description: null,
  kind: { type: "select", default: values[0], options: values.map((v) => ({ value: v, label: v })) },
})

function show(p: Partial<PluginInfo>, values: (typeof state)["values"] = {}) {
  state.plugin = pluginInfo({ id: "dev.a.hello", name: "Hello", ...p })
  state.values = values
  render(<PluginSettingsSection id="dev.a.hello" />, { wrapper })
}

const saved = () => vi.mocked(commands.pluginSettingsSet).mock.calls.map((c) => c[1])

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  platform.mac = false
})
afterEach(cleanup)

describe("a plugin's settings", () => {
  it("shows its folder and its log", async () => {
    show({})
    await screen.findByText("Its folder")
    for (const button of screen.getAllByRole("button", { name: "Reveal" })) fireEvent.click(button)
    expect(reveal.mock.calls).toEqual([[state.plugin!.folder], [state.plugin!.logPath]])
  })

  it("says what its downloaded files take, when it has some", async () => {
    show({ resourceBytes: 327_155_712 })
    await screen.findByText("Its folder")
    expect(screen.getByText("Downloaded files")).toBeTruthy()
    expect(screen.getByText("312 MB. AutoScene keeps them in its cache and removes them with the plugin.")).toBeTruthy()
    // Revealing is for its folder and logs: nothing more to reveal for the files.
    expect(screen.getAllByRole("button", { name: "Reveal" })).toHaveLength(2)
    cleanup()

    // None got yet, or it names none: no row.
    for (const resourceBytes of [0, null]) {
      queryClient.clear()
      show({ resourceBytes })
      await screen.findByText("Its folder")
      expect(screen.queryByText("Downloaded files")).toBeNull()
      cleanup()
    }
  })

  it("says AutoScene enforces what a sandboxed plugin may use", async () => {
    show({ runtime: "wasm", uvScript: false, sandbox: { memoryMb: 512, minutes: 30 }, permissions: ["exports:write", "network"], networkHosts: ["discord.com"] })
    expect(await screen.findByText("It runs in a sandbox. AutoScene enforces these.")).toBeTruthy()
    expect(screen.getByText("Reads, changes and deletes files in the export's folder")).toBeTruthy()
    expect(screen.getByText("Talks to these sites, over https only:")).toBeTruthy()
    expect(screen.getByText("discord.com")).toBeTruthy()
    expect(screen.queryByText(/user rights/)).toBeNull()
    cleanup()
    queryClient.clear()
    // A program's line stays as it is.
    show({ permissions: ["exports:read"] })
    expect(await screen.findByText(/^Its program runs with your user rights\./)).toBeTruthy()
    expect(screen.getByText("Reads the exported clips")).toBeTruthy()
  })

  it("says a broken WebAssembly plugin ran in the sandbox, in the sandbox's words", async () => {
    // As info() gives one whose plugin.json can't be read: the agreed runtime, no limits.
    show({ runtime: "wasm", uvScript: false, sandbox: null, broken: "Its files are missing. Install it again.", permissions: ["exports:write", "network"] })
    expect(await screen.findByText("It runs in a sandbox. AutoScene enforces these.")).toBeTruthy()
    expect(screen.getByText("Reads, changes and deletes files in the export's folder")).toBeTruthy()
    expect(screen.getByText("May use the network, but names no site: every request is refused.")).toBeTruthy()
    expect(screen.queryByText(/user rights/)).toBeNull()
    expect(screen.queryByText("Changes or adds files beside the exported clips")).toBeNull()
  })

  it("saves a number within its bounds", async () => {
    show({ settings: [number("clips", "Clips", { min: 1, integer: true }), number("pause", "Pause", { max: 2 })] }, { clips: 3 })
    const clips = (await screen.findByRole("textbox", { name: "Clips" })) as HTMLInputElement
    await waitFor(() => expect(clips.value).toBe("3"))
    // Typing alone saves nothing; leaving the field does, kept within bounds and whole.
    fireEvent.change(clips, { target: { value: "0" } })
    expect(commands.pluginSettingsSet).not.toHaveBeenCalled()
    fireEvent.blur(clips)
    await waitFor(() => expect(saved()).toEqual([{ clips: 1 }]))
    fireEvent.change(clips, { target: { value: "4,6" } })
    fireEvent.keyDown(clips, { key: "Enter" })
    fireEvent.blur(clips)
    await waitFor(() => expect(saved()).toEqual([{ clips: 1 }, { clips: 5 }]))
    const pause = screen.getByRole("textbox", { name: "Pause" })
    fireEvent.change(pause, { target: { value: "7.5" } })
    fireEvent.blur(pause)
    await waitFor(() => expect(saved().at(-1)).toEqual({ pause: 2 }))
    // What isn't a number is never saved.
    fireEvent.change(pause, { target: { value: "lots" } })
    fireEvent.blur(pause)
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't save the setting", { description: "Pause takes a number." }))
    expect(saved()).toHaveLength(3)
  })

  it("saves a slider once it rests", async () => {
    show({ settings: [{ ...number("level", "Level", { min: 0, max: 10, integer: true }) }] }, { level: 4 })
    const thumb = await screen.findByRole("slider", { name: "Level" })
    await waitFor(() => expect(thumb.getAttribute("aria-valuenow")).toBe("4"))
    fireEvent.keyDown(thumb, { key: "ArrowRight" })
    fireEvent.keyDown(thumb, { key: "ArrowRight" })
    expect(commands.pluginSettingsSet).not.toHaveBeenCalled()
    await waitFor(() => expect(saved()).toEqual([{ level: 6 }]))
  })

  it("saves a slider still moving when the page goes", async () => {
    show({ settings: [{ ...number("level", "Level", { min: 0, max: 10, integer: true }) }] }, { level: 4 })
    const thumb = await screen.findByRole("slider", { name: "Level" })
    await waitFor(() => expect(thumb.getAttribute("aria-valuenow")).toBe("4"))
    fireEvent.keyDown(thumb, { key: "ArrowRight" })
    cleanup()
    expect(saved()).toEqual([{ level: 5 }])
  })

  it("types a number that has no room for a slider", async () => {
    show({ settings: [number("fixed", "Fixed", { min: 3, max: 3 }), number("huge", "Huge", { min: -1e308, max: 1e308 })] }, { fixed: 3 })
    // Both bounds, but nothing between them, or more than a number holds.
    expect(await screen.findByRole("textbox", { name: "Fixed" })).toBeTruthy()
    expect(screen.getByRole("textbox", { name: "Huge" })).toBeTruthy()
    expect(screen.queryByRole("slider")).toBeNull()
  })

  it("saves each kind of setting", async () => {
    const user = userEvent.setup()
    const text: SettingSpec = {
      key: "greeting",
      title: "Greeting",
      description: null,
      kind: { type: "string", default: "Hi", placeholder: null, maxLength: 20 },
    }
    show({
      settings: [
        { key: "loud", title: "Loud", description: null, kind: { type: "boolean", default: false } },
        text,
        select("mood", "Mood", ["calm", "keen"]),
        select("colour", "Colour", ["red", "green", "blue", "gold"]),
      ],
    })
    const loud = (await screen.findByRole("switch", { name: "Loud" })) as HTMLButtonElement
    await waitFor(() => expect(loud.disabled).toBe(false))
    fireEvent.click(loud)
    await waitFor(() => expect(saved()).toEqual([{ loud: true }]))

    const greeting = screen.getByRole("textbox", { name: "Greeting" }) as HTMLInputElement
    expect(greeting.value).toBe("Hi")
    fireEvent.change(greeting, { target: { value: "Hello" } })
    fireEvent.blur(greeting)
    await waitFor(() => expect(saved().at(-1)).toEqual({ greeting: "Hello" }))

    // Two or three choices side by side, more in a list.
    fireEvent.click(within(screen.getByRole("radiogroup", { name: "Mood" })).getByRole("radio", { name: "keen" }))
    await waitFor(() => expect(saved().at(-1)).toEqual({ mood: "keen" }))
    screen.getByRole("combobox", { name: "Colour" }).focus()
    await user.keyboard("{Enter}")
    await user.click(await screen.findByRole("option", { name: "gold" }))
    await waitFor(() => expect(saved().at(-1)).toEqual({ colour: "gold" }))
    expect(saved()).toHaveLength(4)
  })

  it("waits for the stored values before a setting can change", async () => {
    let read: (values: Record<string, SettingValue>) => void = () => {}
    const stored = new Promise<Record<string, SettingValue>>((resolve) => (read = resolve))
    show({ settings: [number("level", "Level", { min: 0, max: 10, integer: true }), select("mood", "Mood", ["calm", "keen"])] }, stored)
    const thumb = await screen.findByRole("slider", { name: "Level" })
    const keen = within(screen.getByRole("radiogroup", { name: "Mood" })).getByRole("radio", { name: "keen" }) as HTMLButtonElement
    expect([thumb.hasAttribute("data-disabled"), keen.disabled]).toEqual([true, true])
    read({ level: 7, mood: "keen" })
    await waitFor(() => expect(thumb.getAttribute("aria-valuenow")).toBe("7"))
    expect([thumb.hasAttribute("data-disabled"), keen.disabled]).toEqual([false, false])
    expect(keen.getAttribute("aria-checked")).toBe("true")
  })

  it("says so when its settings can't be read", async () => {
    const full = { code: "io", message: "The disk is full." }
    // Tried twice.
    vi.mocked(commands.pluginSettingsGet).mockRejectedValueOnce(full).mockRejectedValueOnce(full)
    show({ settings: [select("mood", "Mood", ["calm", "keen"])] })
    expect(await screen.findByText("Couldn't read its settings: The disk is full.", undefined, { timeout: 5000 })).toBeTruthy()
    expect((screen.getByRole("radio", { name: "keen" }) as HTMLButtonElement).disabled).toBe(true)
  })

  it("reads only a setting's own value, never one its object inherits", async () => {
    const text = (key: string, title: string): SettingSpec => ({
      key,
      title,
      description: null,
      kind: { type: "string", default: null, placeholder: null, maxLength: 20 },
    })
    // A setting may be called constructor, or anything else an object inherits.
    const values = Object.create({ name: "inherited" }) as Record<string, SettingValue>
    show({ settings: [text("constructor", "Built by"), text("name", "Name")] }, values)
    const name = (await screen.findByRole("textbox", { name: "Name" })) as HTMLInputElement
    await waitFor(() => expect(name.disabled).toBe(false))
    expect(name.value).toBe("")
    expect((screen.getByRole("textbox", { name: "Built by" }) as HTMLInputElement).value).toBe("")
  })

  it("turns on Run after every export", async () => {
    const action = { id: "hello", title: "Say hello", description: null, afterExport: false, afterFailed: false }
    show({ exportActions: [action] })
    const on = await screen.findByRole("switch", { name: "Run Say hello after every export" })
    expect(on.getAttribute("aria-checked")).toBe("false")
    vi.mocked(commands.pluginSetAfterExport).mockResolvedValue({ ...state.plugin!, exportActions: [{ ...action, afterExport: true }] })
    fireEvent.click(on)
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Run Say hello after every export" }).getAttribute("aria-checked")).toBe("true"),
    )
    expect(commands.pluginSetAfterExport).toHaveBeenCalledExactlyOnceWith("dev.a.hello", "hello", true)
  })

  const analysisDone = {
    id: "done",
    title: "Post when an analysis finishes",
    description: null,
    on: "analysisFinished" as const,
    turnedOn: false,
  }

  it("an event is off until the person turns it on", async () => {
    show({ events: [analysisDone] })
    const name = "Run Post when an analysis finishes, when a project's episodes are all analysed"
    const on = await screen.findByRole("switch", { name })
    expect(on.getAttribute("aria-checked")).toBe("false")
    vi.mocked(commands.pluginSetEvent).mockResolvedValue({ ...state.plugin!, events: [{ ...analysisDone, turnedOn: true }] })
    fireEvent.click(on)
    await waitFor(() => expect(screen.getByRole("switch", { name }).getAttribute("aria-checked")).toBe("true"))
    expect(commands.pluginSetEvent).toHaveBeenCalledExactlyOnceWith("dev.a.hello", "done", true)
    expect(commands.pluginSetAfterExport).not.toHaveBeenCalled()
  })

  it("after-export actions and events share one group", async () => {
    const actions = [
      { id: "post", title: "Post to Discord", description: "Posts the clip list.", afterExport: true, afterFailed: true },
      { id: "tag", title: "Tag the clips", description: null, afterExport: false, afterFailed: false },
    ]
    show({ exportActions: actions, events: [analysisDone] })
    const group = (await screen.findByText("Runs by itself")).closest("section")!
    const rows = within(group).getAllByRole("switch").map((s) => s.getAttribute("aria-label"))
    expect(rows).toEqual([
      "Run Post to Discord after every export, also one that failed",
      "Run Tag the clips after every export",
      "Run Post when an analysis finishes, when a project's episodes are all analysed",
    ])
    // Each row says when it runs, and what it does when its plugin says so.
    expect(within(group).getByText("After every export, also one that failed")).toBeTruthy()
    expect(within(group).getByText("Posts the clip list.")).toBeTruthy()
    expect(within(group).getByText("After every export")).toBeTruthy()
    expect(within(group).getByText("When a project's episodes are all analysed")).toBeTruthy()
    expect(screen.queryByText("After every export", { selector: "h2, h3" })).toBeNull()
  })

  it("has no Runs by itself group without actions or events", async () => {
    show({ settings: [number("count", "Count", {})] })
    await screen.findByText("Settings")
    expect(screen.queryByText("Runs by itself")).toBeNull()
  })

  const secret = (key: string, title: string): SettingSpec => ({
    key,
    title,
    description: null,
    kind: { type: "secret", placeholder: "https://…" },
  })

  it("sets a secret without ever showing it", async () => {
    const user = userEvent.setup()
    show({ settings: [secret("token", "Token")] })
    expect(await screen.findByText("Not set")).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Remove Token" })).toBeNull()
    vi.mocked(commands.pluginSecretSet).mockImplementation(async () => ({ ...state.plugin!, secretsSet: ["token"] }))
    await user.click(screen.getByRole("button", { name: "Set Token" }))
    const field = (await screen.findByLabelText("Token", { selector: "input" })) as HTMLInputElement
    expect([field.type, field.value]).toEqual(["password", ""])
    // Nothing to save until something is typed.
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true)
    await user.type(field, "hunter2")
    await user.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(commands.pluginSecretSet).toHaveBeenCalledExactlyOnceWith("dev.a.hello", "token", "hunter2"))
    expect(await screen.findByText("Saved in your system's keychain")).toBeTruthy()
    // The dialog went, and what was typed with it; it never goes in as a setting's value.
    expect(screen.queryByLabelText("Token", { selector: "input" })).toBeNull()
    expect(document.body.innerHTML).not.toContain("hunter2")
    expect(commands.pluginSettingsSet).not.toHaveBeenCalled()
    // Set again, the field starts empty.
    await user.click(screen.getByRole("button", { name: "Set Token" }))
    expect(((await screen.findByLabelText("Token", { selector: "input" })) as HTMLInputElement).value).toBe("")
  })

  it("refuses a secret too long for the keychain, never cutting it short", async () => {
    const user = userEvent.setup()
    show({ settings: [secret("token", "Token")] })
    await user.click(await screen.findByRole("button", { name: "Set Token" }))
    const field = (await screen.findByLabelText("Token", { selector: "input" })) as HTMLInputElement
    await user.click(field)
    await user.paste("x".repeat(1025))
    // All of it stays in the field, and it says why it can't be saved.
    expect(field.value).toHaveLength(1025)
    expect(screen.getByText("A secret can be at most 1024 characters.")).toBeTruthy()
    const save = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement
    expect(save.disabled).toBe(true)
    fireEvent.submit(field.form!)
    expect(commands.pluginSecretSet).not.toHaveBeenCalled()
    // One shorter goes.
    await user.keyboard("{Backspace}")
    expect(screen.queryByText("A secret can be at most 1024 characters.")).toBeNull()
    expect(save.disabled).toBe(false)
  })

  it("says when a secret is read, and on macOS that the Keychain may ask again", async () => {
    show({ settings: [secret("token", "Token"), { ...secret("hook", "Hook"), description: "From Discord." }] })
    expect(await screen.findByText("Each run gets them as they are when it starts, and a secret each time it asks for it.")).toBeTruthy()
    expect(screen.queryByText(/macOS may ask/)).toBeNull()
    cleanup()
    platform.mac = true
    show({ settings: [secret("token", "Token"), { ...secret("hook", "Hook"), description: "From Discord." }] })
    expect(await screen.findAllByText("macOS may ask you to allow it again after AutoScene updates.")).toHaveLength(2)
    expect(screen.getByText("From Discord.")).toBeTruthy()
  })

  it("removes a secret once asked, and only once", async () => {
    const user = userEvent.setup()
    show({ settings: [secret("token", "Token"), secret("webhook", "Webhook")], secretsSet: ["token", "webhook"] })
    expect(await screen.findAllByText("Saved in your system's keychain")).toHaveLength(2)
    // Asked first; Cancel keeps it.
    await user.click(screen.getByRole("button", { name: "Remove Token" }))
    expect(await screen.findByText("Remove Token?")).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await waitFor(() => expect(screen.queryByText("Remove Token?")).toBeNull())
    expect(commands.pluginSecretClear).not.toHaveBeenCalled()
    // While it goes, Remove waits: a second click sends nothing.
    let done!: (p: PluginInfo) => void
    vi.mocked(commands.pluginSecretClear).mockReturnValueOnce(new Promise((resolve) => (done = resolve)))
    await user.click(screen.getByRole("button", { name: "Remove Token" }))
    const confirm = await screen.findByRole("button", { name: "Remove" })
    await user.click(confirm)
    expect((confirm as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(confirm)
    done({ ...state.plugin!, secretsSet: ["webhook"] })
    await waitFor(() => expect(screen.queryByText("Remove Token?")).toBeNull())
    expect(commands.pluginSecretClear).toHaveBeenCalledExactlyOnceWith("dev.a.hello", "token")
    expect(screen.queryByRole("button", { name: "Remove Token" })).toBeNull()
    expect(screen.getByText("Not set")).toBeTruthy()
    // One the keychain won't remove stays, and says why.
    const locked = "Your system's keychain can't be used: it is locked, or refused AutoScene."
    vi.mocked(commands.pluginSecretClear).mockRejectedValueOnce({ code: "not_ready", message: locked })
    await user.click(screen.getByRole("button", { name: "Remove Webhook" }))
    await user.click(await screen.findByRole("button", { name: "Remove" }))
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't remove the secret", { description: locked }))
    expect((screen.getByRole("button", { name: "Remove" }) as HTMLButtonElement).disabled).toBe(false)
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(await screen.findByRole("button", { name: "Remove Webhook" })).toBeTruthy()
  })

  it("says so for a plugin that isn't installed", async () => {
    state.plugin = null
    render(<PluginSettingsSection id="dev.a.gone" />, { wrapper })
    expect(await screen.findByText("This plugin isn't installed")).toBeTruthy()
  })
})

describe("a choice setting", () => {
  const withLabels = (labels: string[]): SettingSpec => ({
    key: "pick",
    title: "Pick",
    description: null,
    kind: {
      type: "select",
      default: labels[0].toLowerCase(),
      options: labels.map((l) => ({ value: l.toLowerCase(), label: l })),
    },
  })

  it("is a segmented control for three short choices", async () => {
    show({ settings: [withLabels(["Low", "Mid", "High"])] })
    const group = await screen.findByRole("radiogroup", { name: "Pick" })
    expect(within(group).getAllByRole("radio")).toHaveLength(3)
    expect(within(group).getByRole("radio", { name: "Low" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.queryByRole("combobox", { name: "Pick" })).toBeNull()
  })

  it("is a menu for three long choices", async () => {
    // 11 + 11 + 11 = 33 characters, one over what fits side by side.
    show({ settings: [withLabels(["Very quiet.", "Fairly loud", "Really loud"])] })
    await screen.findByRole("combobox", { name: "Pick" })
    expect(screen.queryByRole("radiogroup", { name: "Pick" })).toBeNull()
  })

  it("stays a segmented control at exactly 32 characters", async () => {
    show({ settings: [withLabels(["Very quiet", "Fairly loud", "Really loud"])] })
    await screen.findByRole("radiogroup", { name: "Pick" })
  })

  it("is a menu for one choice of 33 characters", async () => {
    show({ settings: [withLabels(["A label that runs to 33 character"])] })
    await screen.findByRole("combobox", { name: "Pick" })
  })

  it("is a menu for four choices", async () => {
    show({ settings: [withLabels(["A", "B", "C", "D"])] })
    await screen.findByRole("combobox", { name: "Pick" })
    expect(screen.queryByRole("radiogroup", { name: "Pick" })).toBeNull()
  })
})
