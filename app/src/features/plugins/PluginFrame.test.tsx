import type * as React from "react"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { PageMount, PluginPageEvent } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { events } from "@/lib/ipc"
import { i18n, LanguageRoot } from "@/lib/i18n"
import { inLanguage, plain } from "@/test/i18n"
import { replayMenu } from "./bridge"
import { PluginFrame } from "./PluginFrame"

const { pluginPageOpen, pluginPageClose } = vi.hoisted(() => ({
  pluginPageOpen: vi.fn(),
  pluginPageClose: vi.fn(async () => null),
}))

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => vi.fn(), useCanGoBack: () => false }))
vi.mock("@/lib/queries", () => ({ useAppInfo: () => ({ data: { version: "1.0.3-BETA" } }) }))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return {
    ...ipc,
    commands: { ...ipc.commands, pluginPageOpen, pluginPageClose },
    events: { ...ipc.events, pluginPageEvent: { listen: vi.fn(async () => () => {}) } },
  }
})

const SESSION = "0123456789abcdef0123456789abcdef"

function mount(o: Partial<PageMount> = {}): PageMount {
  return {
    session: SESSION,
    src: `http://asp.localhost/${SESSION}/ui/index.html`,
    plugin: { id: "dev.a.page", name: "Page", version: "1.0.0", trust: "reviewed", source: "package", icon: null },
    page: { id: "main", title: "Main", description: null, entry: "ui/index.html", scope: "global", rail: true },
    projectId: null,
    permissions: [],
    hosts: [],
    service: false,
    protocol: 2,
    ...o,
  }
}

function frame(path: string, menu: React.ReactNode = null) {
  return (
    <PluginFrame
      pluginId="dev.a.page"
      pageId="main"
      projectId={null}
      path={path}
      menu={menu}
      onReload={() => {}}
      onClose={() => {}}
    />
  )
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe("a plugin page's frame", () => {
  it("the frame is sandboxed to scripts and forms, nothing more", async () => {
    pluginPageOpen.mockResolvedValue(mount())
    render(frame(""))
    const iframe = await screen.findByLabelText("Main")
    expect(iframe.tagName).toBe("IFRAME")
    expect(iframe.getAttribute("sandbox")).toBe("allow-scripts allow-forms")
    expect(iframe.getAttribute("sandbox")).not.toContain("same-origin")
    expect(iframe.getAttribute("allow")).toBe("")
    expect(iframe.getAttribute("referrerpolicy")).toBe("no-referrer")
    // No title of its own: what names it is AutoScene's label, its page's title.
    expect(iframe.hasAttribute("title")).toBe(false)
    expect(pluginPageOpen).toHaveBeenCalledWith("dev.a.page", "main", null)
  })

  it("its src comes from the session, never from the splat", async () => {
    pluginPageOpen.mockResolvedValue(mount())
    const { rerender, unmount } = render(frame("../../_as/sdk.js"))
    const iframe = await screen.findByLabelText("Main")
    expect(iframe.getAttribute("src")).toBe(`http://asp.localhost/${SESSION}/ui/index.html`)
    // A move inside the page is no new frame, nor a new address.
    rerender(frame("clips/3?src=https://evil.example/"))
    expect(screen.getByLabelText("Main")).toBe(iframe)
    expect(iframe.getAttribute("src")).toBe(`http://asp.localhost/${SESSION}/ui/index.html`)
    expect(pluginPageOpen).toHaveBeenCalledTimes(1)
    // Gone, its session closes.
    unmount()
    await waitFor(() => expect(pluginPageClose).toHaveBeenCalledWith(SESSION))
  })

  it("the frame takes the focus as its page first loads, never later, nor from a field", async () => {
    pluginPageOpen.mockResolvedValue(mount())
    const { unmount } = render(
      <>
        <input aria-label="Search" />
        {frame("")}
      </>,
    )
    const iframe = await screen.findByLabelText("Main")
    const field = screen.getByLabelText("Search")
    fireEvent.load(iframe)
    expect(document.activeElement).toBe(iframe)
    // The page loads itself again while the person types elsewhere: the field keeps the focus.
    field.focus()
    fireEvent.load(iframe)
    expect(document.activeElement).toBe(field)
    // Nor does a later load take it back from where the person clicked.
    ;(document.activeElement as HTMLElement).blur()
    fireEvent.load(iframe)
    expect(document.activeElement).toBe(document.body)
    unmount()

    // A page that first loads while the person is in a field leaves them there.
    render(
      <>
        <input aria-label="Search" />
        {frame("")}
      </>,
    )
    const late = await screen.findByLabelText("Main")
    screen.getByLabelText("Search").focus()
    fireEvent.load(late)
    expect(document.activeElement).toBe(screen.getByLabelText("Search"))
  })

  it("a right-click the page hands over opens the window's menu with the page's entries", async () => {
    pluginPageOpen.mockResolvedValue(mount())
    render(
      <AppMenu>
        <div>{frame("", <ContextMenuItem>Reload the page</ContextMenuItem>)}</div>
      </AppMenu>,
    )
    replayMenu((await screen.findByLabelText("Main")) as HTMLIFrameElement, 12, 34)
    const menu = await screen.findByRole("menu")
    expect(within(menu).getByText("Reload the page")).toBeTruthy()
  })

  it("what Rust tells the page as it opens reaches it once it connects", async () => {
    let tell: ((e: { payload: PluginPageEvent }) => void) | null = null
    vi.mocked(events.pluginPageEvent.listen).mockImplementation(async (heard) => {
      tell = heard as typeof tell
      return () => {}
    })
    pluginPageOpen.mockImplementation(async () => {
      // Its service runs already: Rust says so before the open's answer is back.
      tell!({ payload: { session: SESSION, event: { type: "serviceState", state: "ready", message: null } } })
      tell!({ payload: { session: "another", event: { type: "serviceState", state: "stopped", message: plain("No.") } } })
      return mount({ service: true })
    })
    render(frame(""))
    const iframe = (await screen.findByLabelText("Main")) as HTMLIFrameElement
    let port: MessagePort | null = null
    vi.spyOn(iframe.contentWindow!, "postMessage").mockImplementation((_m: unknown, _o: unknown, transfer?: unknown) => {
      port = (transfer as MessagePort[])[0]
    })
    const ready = new MessageEvent("message", { data: { type: "autoscene:ready", v: 1 } })
    Object.defineProperty(ready, "source", { value: iframe.contentWindow })
    window.dispatchEvent(ready)
    expect(port).not.toBeNull()
    const heard: unknown[] = []
    port!.addEventListener("message", (e: MessageEvent<{ t: string }>) => void (e.data.t !== "ping" && heard.push(e.data)))
    port!.start()
    await waitFor(() => expect(heard).toEqual([{ t: "service", state: "ready" }]))
    port!.close()
  })

  it("the page is told AutoScene's language as it connects, and each switch after", async () => {
    await inLanguage("ro")
    pluginPageOpen.mockResolvedValue(mount())
    render(frame(""))
    const iframe = (await screen.findByLabelText("Main")) as HTMLIFrameElement
    let init: { init: { app: { locale: string } } } | null = null
    let port: MessagePort | null = null
    vi.spyOn(iframe.contentWindow!, "postMessage").mockImplementation((m: unknown, _o: unknown, transfer?: unknown) => {
      init = m as typeof init
      port = (transfer as MessagePort[])[0]
    })
    // Its connector listens once the frame's effects have run: the page says ready until it is heard (once counts).
    await waitFor(() => {
      const ready = new MessageEvent("message", { data: { type: "autoscene:ready", v: 1 } })
      Object.defineProperty(ready, "source", { value: iframe.contentWindow })
      window.dispatchEvent(ready)
      expect(init).not.toBeNull()
    })
    expect(init!.init.app.locale).toBe("ro")
    const heard: unknown[] = []
    port!.addEventListener("message", (e: MessageEvent<{ t: string }>) => void (e.data.t !== "ping" && heard.push(e.data)))
    port!.start()

    await act(() => inLanguage("en"))
    await waitFor(() => expect(heard).toEqual([{ t: "locale", locale: "en" }]))
    // Into the pseudo-locale: still English for a page, so nothing new to tell.
    await act(() => inLanguage("en-XA"))
    await act(() => inLanguage("ro"))
    await waitFor(() => expect(heard).toEqual([{ t: "locale", locale: "en" }, { t: "locale", locale: "ro" }]))
    port!.close()
  })

  it("a frame that a switch replaces is not told, and the new one starts in the new language", async () => {
    pluginPageOpen.mockResolvedValue(mount())
    render(<LanguageRoot>{frame("")}</LanguageRoot>)
    // The page's side of a connect: its init and its port, as the connector answers `ready`.
    const connect = async (iframe: HTMLIFrameElement) => {
      let init: { init: { app: { locale: string } } } | null = null
      let port: MessagePort | null = null
      vi.spyOn(iframe.contentWindow!, "postMessage").mockImplementation((m: unknown, _o: unknown, transfer?: unknown) => {
        init = m as typeof init
        port = (transfer as MessagePort[])[0]
      })
      await waitFor(() => {
        const ready = new MessageEvent("message", { data: { type: "autoscene:ready", v: 1 } })
        Object.defineProperty(ready, "source", { value: iframe.contentWindow })
        window.dispatchEvent(ready)
        expect(init).not.toBeNull()
      })
      const heard: unknown[] = []
      port!.addEventListener("message", (e: MessageEvent<{ t: string }>) => void (e.data.t !== "ping" && heard.push(e.data)))
      port!.start()
      return { init: init!, port: port!, heard }
    }
    const old = await connect((await screen.findByLabelText("Main")) as HTMLIFrameElement)
    expect(old.init.init.app.locale).toBe("en")

    await act(() => inLanguage("ro"))
    // The routed tree is keyed by the language: a new frame, a new session.
    await waitFor(() => expect(pluginPageOpen).toHaveBeenCalledTimes(2))
    let fresh!: HTMLIFrameElement
    await waitFor(() => {
      fresh = screen.getByLabelText("Main") as HTMLIFrameElement
      expect(fresh.isConnected).toBe(true)
    })
    const next = await connect(fresh)
    expect(next.init.init.app.locale).toBe("ro")
    // Neither frame hears a switch that was theirs to start over for.
    await new Promise((r) => setTimeout(r, 50))
    expect(old.heard).toEqual([])
    expect(next.heard).toEqual([])
    old.port.close()
    next.port.close()
  })

  it("a frame that is gone stops listening for the language", async () => {
    const listeners = () => (i18n as unknown as { observers: Record<string, Map<unknown, number>> }).observers.languageChanged?.size ?? 0
    const before = listeners()
    pluginPageOpen.mockResolvedValue(mount())
    const { unmount } = render(frame(""))
    await screen.findByLabelText("Main")
    // The frame's effects have run once its connector exists: its listener is one more.
    await waitFor(() => expect(listeners()).toBeGreaterThan(before))
    unmount()
    expect(listeners()).toBe(before)
  })

  it("a page that was closed says why in the UI's language", async () => {
    await inLanguage("ro", { backend: { plugin: { closedBecause: { updating: "Se actualizează." } } } })
    pluginPageOpen.mockResolvedValue(mount())
    render(frame(""))
    await waitFor(() => expect(events.pluginPageEvent.listen).toHaveBeenCalled())
    const heard = vi.mocked(events.pluginPageEvent.listen).mock.calls[0][0]
    const reason = { key: "plugin.closedBecause.updating", text: "It is being updated." }
    act(() => heard({ event: "plugin-page:event", id: 1, payload: { session: SESSION, event: { type: "closed", reason } } }))
    expect(within(await screen.findByRole("alert")).getByText("Se actualizează.")).toBeTruthy()
  })

  it("a cover says why aloud, takes the keyboard to its buttons, and puts the frame out of reach", async () => {
    pluginPageOpen.mockRejectedValue({ code: "not_ready", message: "Page is being updated." })
    render(frame(""))
    const cover = await screen.findByRole("alert")
    expect(within(cover).getByText("Couldn't open this page.")).toBeTruthy()
    await waitFor(() => expect(document.activeElement).toBe(within(cover).getByRole("button", { name: "Try again" })))
  })
})
