import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { GraphicsClass, Platform, ScanChoice, ScanDefaults } from "@/bindings"
import { Dialog, DialogContent } from "@/components/ui/dialog"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { noticeOpen, useFastScanNotice } from "@/state/fast-scan-notice"
import { useNewProject } from "@/state/new-project"
import { useUpdates } from "@/state/updates"
import { AFTER_SPLASH_MS, FastScanNotice, SHOWN_KEY } from "./FastScanNotice"

const { state, route } = vi.hoisted(() => {
  let path = "/"
  const subs = new Set<() => void>()
  return {
    state: {
      cls: "integrated" as GraphicsClass,
      name: "Intel Iris Xe Graphics",
      platform: "windows" as Platform,
      choice: "auto" as ScanChoice,
      findBodies: false,
      done: false,
    },
    route: {
      get: () => path,
      go(p: string) {
        path = p
        subs.forEach((f) => f())
      },
      subscribe(f: () => void) {
        subs.add(f)
        return () => void subs.delete(f)
      },
    },
  }
})

/** What the library answers, like `as_project::scan::defaults`. */
function defaults(): ScanDefaults {
  const { cls, choice, findBodies, done } = state
  const here = cls === "notForAi" || cls === "integrated" || cls === "processor"
  const fast = (content: string) =>
    choice === "auto" ? here && content !== "animation" && (content === "anime" || !findBodies) : choice === "fast"
  return {
    computer: { class: cls, name: state.name },
    choice,
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({ content, fast: fast(content) })),
    notice: choice === "auto" && here && !done,
  }
}

vi.mock("@tanstack/react-router", async () => {
  const { useSyncExternalStore } = await import("react")
  return {
    useRouterState: ({ select }: { select: (s: { location: { pathname: string } }) => unknown }) =>
      select({ location: { pathname: useSyncExternalStore(route.subscribe, route.get) } }),
  }
})
vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    appInfo: vi.fn(() => Promise.resolve({ version: "1.1.0", platform: state.platform, arch: "x86_64", dataDir: "C:\\Data" })),
    scanDefaultsGet: vi.fn(() => Promise.resolve(defaults())),
    scanDefaultSet: vi.fn((choice: ScanChoice) => {
      state.choice = choice
      state.done = true
      return Promise.resolve(defaults())
    }),
    scanNoticeDone: vi.fn(() => {
      state.done = true
      return Promise.resolve(null)
    }),
  },
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
)

/** The New Project dialog, as far as the notice is concerned: a modal that `useNewProject` opens. */
function NewProject() {
  const open = useNewProject((s) => s.open)
  return (
    <Dialog open={open} onOpenChange={(o) => useNewProject.getState().setOpen(o)}>
      <DialogContent title="New project">
        {/* As NewProjectDialog's title field. */}
        <input aria-label="Title" autoFocus />
      </DialogContent>
    </Dialog>
  )
}

function Shell({ menu = false }: { menu?: boolean }) {
  return (
    <>
      {menu && (
        <DropdownMenu>
          <DropdownMenuTrigger>Sort</DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuItem>By name</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <NewProject />
      <FastScanNotice />
    </>
  )
}

const show = (menu = false) => render(<Shell menu={menu} />, { wrapper })
const notice = () => screen.findByRole("dialog", { name: "Fast Scan is on" }, { timeout: 4000 })
const noNotice = () => expect(screen.queryByRole("dialog", { name: "Fast Scan is on" })).toBeNull()
/** Past the splash, with every answer in. */
const settle = () => act(() => new Promise((r) => setTimeout(r, AFTER_SPLASH_MS + 150)))
const button = (name: string) => screen.getByRole("button", { name })

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  Object.assign(state, {
    cls: "integrated",
    name: "Intel Iris Xe Graphics",
    platform: "windows",
    choice: "auto",
    findBodies: false,
    done: false,
  })
  route.go("/")
  sessionStorage.clear()
  useNewProject.setState({ open: false, dragging: false, seed: [] })
  useUpdates.setState({ dialogOpen: false })
  useFastScanNotice.setState({ phase: "waiting" })
})
afterEach(() => {
  cleanup()
  document.body.style.pointerEvents = ""
})

// Each case waits out the splash (and some several times), on a machine that may be busy.
describe("FastScanNotice", { timeout: 30_000 }, () => {
  it("shows after the splash over the library when due", async () => {
    show()
    await waitFor(() => expect(commands.scanDefaultsGet).toHaveBeenCalled())
    await act(() => new Promise((r) => setTimeout(r, AFTER_SPLASH_MS / 2)))
    noNotice()
    await notice()
    expect(noticeOpen(useFastScanNotice.getState())).toBe(true)
    expect(sessionStorage.getItem(SHOWN_KEY)).toBe("1")
  })

  it("waits while the New Project dialog, a drop, the update dialog or a menu is open, and opens the moment it closes", async () => {
    const blockers: [string, () => void, () => void][] = [
      ["New Project", () => useNewProject.setState({ open: true }), () => useNewProject.setState({ open: false })],
      ["a drop", () => useNewProject.setState({ dragging: true }), () => useNewProject.setState({ dragging: false })],
      ["the update dialog", () => useUpdates.setState({ dialogOpen: true }), () => useUpdates.setState({ dialogOpen: false })],
    ]
    for (const [what, open, close] of blockers) {
      act(open)
      show()
      await settle()
      expect(screen.queryByRole("dialog", { name: "Fast Scan is on" }), what).toBeNull()
      act(close)
      await notice()
      cleanup()
      sessionStorage.clear()
      useFastScanNotice.setState({ phase: "waiting" })
    }

    // A Radix menu, followed through its `data-state`.
    const user = userEvent.setup()
    show(true)
    await user.click(button("Sort"))
    await screen.findByRole("menu")
    await settle()
    noNotice()
    await user.keyboard("{Escape}")
    await notice()
  })

  it("decided once: not due at the first answer, it doesn't open later in the run when the answer changes", async () => {
    state.cls = "forAi"
    show()
    await settle()
    noNotice()
    state.cls = "integrated"
    await act(() => queryClient.invalidateQueries({ queryKey: qk.scanDefaults }))
    await waitFor(() => expect(queryClient.getQueryData<ScanDefaults>(qk.scanDefaults)?.notice).toBe(true))
    await settle()
    noNotice()
  })

  it("leaving the library before it opens leaves it for the next start", async () => {
    act(() => useNewProject.setState({ open: true }))
    show()
    await settle()
    // The drop made a project, and the app went to it.
    act(() => {
      useNewProject.setState({ open: false })
      route.go("/project/7")
    })
    await settle()
    act(() => route.go("/"))
    await settle()
    noNotice()
    expect(sessionStorage.getItem(SHOWN_KEY)).toBeNull()
    expect(commands.scanNoticeDone).not.toHaveBeenCalled()
  })

  it("Continue without the tick stores nothing and doesn't show twice in a run or after a reload", async () => {
    const user = userEvent.setup()
    show()
    await notice()
    await user.click(button("Continue"))
    await waitFor(noNotice)
    expect(commands.scanNoticeDone).not.toHaveBeenCalled()
    expect(noticeOpen(useFastScanNotice.getState())).toBe(false)
    // Back on the library later in the run.
    act(() => route.go("/settings"))
    act(() => route.go("/"))
    await settle()
    noNotice()
    // A reload in the same run: everything starts afresh but this run's storage.
    cleanup()
    queryClient.clear()
    useFastScanNotice.setState({ phase: "waiting" })
    show()
    await settle()
    noNotice()
  })

  it("Continue or Escape with the tick calls scanNoticeDone", async () => {
    const user = userEvent.setup()
    for (const answer of ["Continue", "Escape"]) {
      show()
      await notice()
      await user.click(screen.getByRole("checkbox", { name: "Don't show this again" }))
      if (answer === "Continue") await user.click(button("Continue"))
      else await user.keyboard("{Escape}")
      await waitFor(noNotice)
      expect(commands.scanNoticeDone, answer).toHaveBeenCalledTimes(1)
      cleanup()
      vi.clearAllMocks()
      queryClient.clear()
      sessionStorage.clear()
      useFastScanNotice.setState({ phase: "waiting" })
      state.done = false
    }
  })

  it("Turn off Fast Scan calls scanDefaultSet('full') and toasts; Undo calls scanDefaultSet('auto'); a failed Undo says so", async () => {
    const user = userEvent.setup()
    show()
    await notice()
    await user.click(button("Turn off Fast Scan"))
    await waitFor(noNotice)
    expect(commands.scanDefaultSet).toHaveBeenCalledWith("full")
    expect(queryClient.getQueryData<ScanDefaults>(qk.scanDefaults)?.choice).toBe("full")
    expect(toast).toHaveBeenCalledWith(
      "Fast Scan is off",
      expect.objectContaining({ description: "New projects start with the full scan." }),
    )
    expect(commands.scanNoticeDone).not.toHaveBeenCalled()

    const undo = vi.mocked(toast).mock.calls[0][1]?.action as { label: string; onClick: () => void }
    expect(undo.label).toBe("Undo")
    undo.onClick()
    await waitFor(() => expect(commands.scanDefaultSet).toHaveBeenLastCalledWith("auto"))
    await waitFor(() => expect(queryClient.getQueryData<ScanDefaults>(qk.scanDefaults)?.choice).toBe("auto"))

    vi.mocked(commands.scanDefaultSet).mockRejectedValueOnce({ code: "internal", message: "The disk is full." })
    undo.onClick()
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't turn Fast Scan back on", { description: "The disk is full." }),
    )
    noNotice()
  })

  it("a failed Turn off keeps the notice open", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.scanDefaultSet).mockRejectedValueOnce({ code: "internal", message: "The disk is full." })
    show()
    await notice()
    await user.click(button("Turn off Fast Scan"))
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't turn off Fast Scan", { description: "The disk is full." }))
    expect(screen.getByRole("dialog", { name: "Fast Scan is on" })).toBeTruthy()
    expect(button("Continue").hasAttribute("disabled")).toBe(false)
  })

  it("New Project opening while it is open closes it as Continue (honouring the tick); only New Project is open and has the focus", async () => {
    const user = userEvent.setup()
    for (const ticked of [false, true]) {
      show()
      await notice()
      if (ticked) await user.click(screen.getByRole("checkbox", { name: "Don't show this again" }))
      act(() => useNewProject.getState().show(["D:\\Films\\Tears of Steel.mkv"]))
      await waitFor(noNotice)
      const dialogs = screen.getAllByRole("dialog")
      expect(dialogs.map((d) => d.getAttribute("aria-labelledby") && document.getElementById(d.getAttribute("aria-labelledby")!)?.textContent)).toEqual([
        "New project",
      ])
      // Its title field, as when it opens alone: the notice's focus trap doesn't hold it back.
      await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Title" })))
      expect(commands.scanNoticeDone).toHaveBeenCalledTimes(ticked ? 1 : 0)
      cleanup()
      vi.clearAllMocks()
      queryClient.clear()
      sessionStorage.clear()
      useFastScanNotice.setState({ phase: "waiting" })
      state.done = false
      useNewProject.setState({ open: false, seed: [] })
    }
  })

  it("focus starts on Continue without a ring, which shows once Tab moves it; labelled by its title and described by the device and the reason, with no Radix warning", async () => {
    const warn = vi.spyOn(console, "warn")
    const error = vi.spyOn(console, "error")
    const user = userEvent.setup()
    show()
    const dialog = await notice()
    const go = button("Continue")
    await waitFor(() => expect(document.activeElement).toBe(go))
    expect(go.hasAttribute("data-quiet-focus")).toBe(true)
    expect(dialog.getAttribute("aria-describedby")).toBeTruthy()
    expect(document.getElementById(dialog.getAttribute("aria-describedby")!)?.textContent).toBe(
      "Intel Iris Xe Graphics. This computer analyses video on its built-in graphics. A full scan is heavy work for them, so new projects start with Fast Scan.",
    )

    // Tab stays inside: the tick box, Turn off Fast Scan, Continue.
    await user.tab()
    expect(document.activeElement).toBe(screen.getByRole("checkbox", { name: "Don't show this again" }))
    expect(go.hasAttribute("data-quiet-focus")).toBe(false)
    await user.keyboard(" ")
    expect(screen.getByRole("checkbox").getAttribute("aria-checked")).toBe("true")
    await user.tab()
    expect(document.activeElement).toBe(button("Turn off Fast Scan"))
    await user.tab()
    expect(document.activeElement).toBe(go)
    await user.tab()
    expect(document.activeElement).toBe(screen.getByRole("checkbox"))

    const radix = (spy: typeof warn) => spy.mock.calls.filter((c) => String(c[0]).includes("DialogContent"))
    expect(radix(warn)).toEqual([])
    expect(radix(error)).toEqual([])
    warn.mockRestore()
    error.mockRestore()
  })

  it("the processor, Linux and Mac wording; the foot with faceless people on", async () => {
    const cases: [Partial<typeof state>, string, string][] = [
      [
        { cls: "processor", name: "", platform: "windows" },
        "Processor",
        "This computer has no graphics card AutoScene can use. A full scan is heavy work for a processor, so new projects start with Fast Scan.",
      ],
      [
        { cls: "processor", name: "", platform: "linux" },
        "Processor",
        "On Linux, AutoScene analyses video on the processor. A full scan is heavy work for a processor, so new projects start with Fast Scan.",
      ],
      [
        { cls: "integrated", name: "Apple M3 Max", platform: "macos" },
        "Apple M3 Max",
        "This Mac analyses video on its built-in graphics. A full scan is heavy work for them, so new projects start with Fast Scan.",
      ],
      [
        { cls: "notForAi", name: "NVIDIA GeForce GTX 1060 6GB", platform: "windows" },
        "NVIDIA GeForce GTX 1060 6GB",
        "This graphics card has no hardware made for AI work. A full scan is heavy work for it, so new projects start with Fast Scan.",
      ],
    ]
    for (const [machine, device, lead] of cases) {
      Object.assign(state, machine)
      show()
      const dialog = await notice()
      expect(document.getElementById(dialog.getAttribute("aria-describedby")!)?.textContent).toBe(`${device}. ${lead}`)
      expect(dialog.textContent).toContain(device)
      expect(dialog.textContent).not.toContain("CC BY")
      expect(dialog.querySelector("img")?.getAttribute("src")).toBe("/fast-scan-notice.webp")
      expect(dialog.textContent).toContain("3D animation keeps the full scan. You can change the default any time in Settings.")
      cleanup()
      queryClient.clear()
      sessionStorage.clear()
      useFastScanNotice.setState({ phase: "waiting" })
    }

    state.findBodies = true
    show()
    const dialog = await notice()
    expect(dialog.textContent).toContain(
      "3D animation keeps the full scan, and so does live action while characters without a visible face are found. You can change the default any time in Settings.",
    )
  })
})
