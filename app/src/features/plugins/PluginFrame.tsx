import { useEffect, useRef, useState } from "react"
import { useNavigate } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import type { PageMount, PluginPageEvent } from "@/bindings"
import { AppWindow, Warning } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { ContextArea, type MenuContent } from "@/components/ui/context-area"
import { EmptyState } from "@/components/ui/empty-state"
import { commands, errorMessage, events } from "@/lib/ipc"
import { isMac, isTauri, isWindows } from "@/lib/platform"
import { useAppInfo } from "@/lib/queries"
import type { PageInit } from "@/plugin-kit/bridge-types"
import { FRAME, createConnector, inputElsewhere, pageCaller, pageMessage, replayKey, replayMenu, showToast } from "./bridge"

/** What covers the frame: the page hangs, it was closed, it sent too much, or it couldn't open. */
type Cover = { kind: "quiet" } | { kind: "closed"; reason: string } | { kind: "overflow" } | { kind: "failed"; message: string }

/**
 * A plugin's page in a sandboxed frame, as AutoScene's session for it
 * serves it: its address comes from the session, never from the route. The
 * page reaches AutoScene only through the bridge (`bridge.ts`), whose calls
 * name the session the app holds. `path` is the route's part after the
 * page, as the page set it: a change the page didn't make (Back, Forward)
 * is told to it. What Rust tells the page (its settings changed, its
 * plugin's service's events and state, its close) reaches it on its port,
 * also what it told before the frame was there: it is listened to before
 * the page opens.
 * The frame takes the focus once, as its page first loads,
 * unless the person is in a field or a dialog by then. On
 * macOS and Linux the app's page says every 2 s that it is still there, so
 * AutoScene can ask the person about a page that hung it. Mounted once per
 * session: a new plugin, page, project or version mounts a new one.
 */
export function PluginFrame({
  pluginId,
  pageId,
  projectId,
  path,
  menu,
  onReload,
  onClose,
}: {
  pluginId: string
  pageId: string
  projectId: string | null
  /** The path inside the page. */
  path: string
  /** What a right-click on the page offers. */
  menu: MenuContent
  onReload: () => void
  onClose: () => void
}) {
  const ref = useRef<HTMLIFrameElement>(null)
  const navigate = useNavigate()
  const { data: app } = useAppInfo()
  const [mount, setMount] = useState<PageMount | null>(null)
  const [cover, setCover] = useState<Cover | null>(null)
  const connector = useRef<ReturnType<typeof createConnector> | null>(null)
  // What Rust told before the page's connector was there (its service's state as it opened), and who hears it after.
  const early = useRef<PluginPageEvent[]>([])
  const tell = useRef<((e: PluginPageEvent) => void) | null>(null)
  // The path as the page last knew it: its own moves aren't told back to it.
  const known = useRef(path)
  // Its page loaded once already: loading itself again never takes the focus.
  const loaded = useRef(false)
  const version = useRef("")
  useEffect(() => {
    version.current = app?.version ?? ""
  }, [app?.version])

  // One session while mounted; one that opens after the frame went is closed
  // at once. Rust's events are heard before it opens: Rust tells the page its
  // service's state as it opens, before the open's answer is back.
  useEffect(() => {
    let live = true
    let session: string | null = null
    const listening = events.pluginPageEvent.listen(({ payload }) => {
      if (tell.current) tell.current(payload)
      else early.current = [...early.current.slice(1 - EARLY), payload]
    })
    void listening
      .catch(() => undefined)
      .then(() => {
        if (!live) return
        commands.pluginPageOpen(pluginId, pageId, projectId).then(
          (m) => {
            if (!live) return void commands.pluginPageClose(m.session).catch(() => undefined)
            session = m.session
            setMount(m)
          },
          (e: unknown) => live && setCover({ kind: "failed", message: errorMessage(e) }),
        )
      })
    return () => {
      live = false
      early.current = []
      void listening.then((un) => un()).catch(() => undefined)
      if (session) void commands.pluginPageClose(session).catch(() => undefined)
    }
  }, [pluginId, pageId, projectId])

  useEffect(() => {
    if (!mount) return
    const c = createConnector({
      frame: () => ref.current?.contentWindow,
      init: () => pageInit(mount, known.current, version.current),
      deps: {
        call: pageCaller(mount.session),
        effect: (e) => {
          if (e.type === "toast") return showToast(e)
          known.current = e.path
          // The splat as a param, which the router writes out as it is: in `to`, a `$` or `%` of the path would be the router's.
          void navigate({ to: ".", params: (p: Record<string, string>) => ({ ...p, _splat: e.path }), replace: e.replace })
        },
        // On macOS the menu bar owns ⌘ keys: the page's never reach it anyway.
        key: (k) => !isMac && replayKey(k, window),
        menu: (x, y) => ref.current && replayMenu(ref.current, x, y),
        unresponsive: (on) => setCover((c) => (on ? { kind: "quiet" } : c?.kind === "quiet" ? null : c)),
        overflow: () => setCover({ kind: "overflow" }),
        // A page can move the focus into its frame itself, and keeps it while
        // the person works in another app: the window must be in front too.
        focused: () => ref.current != null && document.activeElement === ref.current && document.hasFocus(),
        // A click or a key in the frame counts for the app's page too: so
        // Chromium (WebView2) has it. So does one in the app's page itself,
        // like the click that opened the page: none may have come just
        // before. WebKit's frames are only held to the focus.
        elsewhere: () => isWindows && clickedElsewhere(),
        activated: () => !isWindows || !navigator.userActivation || navigator.userActivation.isActive,
      },
    })
    connector.current = c
    const onMessage = (e: MessageEvent) => void c.message(e)
    window.addEventListener("message", onMessage)
    tell.current = (p) => {
      if (p.session !== mount.session) return
      c.send(pageMessage(p.event))
      if (p.event.type === "closed") setCover({ kind: "closed", reason: p.event.reason })
    }
    for (const p of early.current.splice(0)) tell.current(p)
    const beats =
      isTauri && !isWindows
        ? window.setInterval(() => void commands.pluginPageHeartbeat(mount.session).catch(() => undefined), 2000)
        : undefined
    return () => {
      window.removeEventListener("message", onMessage)
      tell.current = null
      window.clearInterval(beats)
      c.close()
      connector.current = null
    }
  }, [mount, navigate])

  // Back or Forward moved inside the page: it hears where to.
  useEffect(() => {
    if (path === known.current) return
    known.current = path
    connector.current?.send({ t: "route", path })
  }, [path])

  return (
    <ContextArea kind="page" menu={menu}>
      <div className="relative min-h-0 flex-1 bg-canvas">
        {mount && (
          <iframe
            ref={ref}
            src={mount.src}
            sandbox={FRAME.sandbox}
            allow={FRAME.allow}
            referrerPolicy={FRAME.referrerPolicy}
            aria-label={mount.page.title}
            // Covered, it is out of reach: the keyboard goes to the cover's buttons.
            inert={cover != null}
            className="block size-full border-0 bg-transparent"
            onLoad={() => {
              connector.current?.load()
              if (loaded.current) return
              loaded.current = true
              if (ref.current && mayTakeFocus(document.activeElement, ref.current)) ref.current.focus()
            }}
          />
        )}
        {cover && <Covered cover={cover} onReload={onReload} onClose={onClose} />}
      </div>
    </ContextArea>
  )
}

/** Whether the person just clicked or typed in AutoScene, outside a frame: heard from the start, so the click that opens a page counts. */
const clickedElsewhere = inputElsewhere(window)

/** How many of Rust's events are kept for a page whose connector isn't there yet. */
const EARLY = 64

/**
 * Whether a page that opened may take the focus from `at`: not from a field
 * the person types in, nor from a dialog or menu they opened meanwhile.
 */
function mayTakeFocus(at: Element | null, frame: HTMLIFrameElement): boolean {
  if (at == null || at === frame || at === document.body) return true
  return at.closest("input, textarea, select, [contenteditable], [role=dialog], [role=alertdialog], [role=menu]") == null
}

/** What a page is told as it connects. */
function pageInit(m: PageMount, path: string, version: string): PageInit {
  return {
    plugin: { id: m.plugin.id, name: m.plugin.name, version: m.plugin.version },
    page: { id: m.page.id, title: m.page.title, scope: m.page.scope },
    projectId: m.projectId,
    path,
    theme: { v: 1, scheme: "dark" },
    permissions: m.permissions,
    hosts: m.hosts,
    service: m.service,
    protocol: m.protocol,
    app: { version, platform: isMac ? "macos" : isWindows ? "windows" : "linux" },
  }
}

/**
 * Over the frame (never over AutoScene's bar): why the page can't be used,
 * and what to do. Said aloud as it comes, with the keyboard on its first
 * button: the frame under it is out of reach.
 */
function Covered({ cover, onReload, onClose }: { cover: Cover; onReload: () => void; onClose: () => void }) {
  const { t } = useTranslation(["plugins", "common"])
  const said = {
    quiet: { title: t("frame.quiet"), description: undefined },
    closed: { title: t("frame.closed"), description: cover.kind === "closed" ? cover.reason : undefined },
    overflow: { title: t("frame.overflow"), description: undefined },
    failed: { title: t("frame.failed"), description: cover.kind === "failed" ? cover.message : undefined },
  }[cover.kind]
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    // Not from where the person went meanwhile: only from the frame, or from nowhere.
    const at = document.activeElement
    if (at == null || at === document.body || at.tagName === "IFRAME") ref.current?.querySelector("button")?.focus()
  }, [cover.kind])
  return (
    <div ref={ref} role="alert" className="absolute inset-0 flex items-center justify-center bg-canvas/85 backdrop-blur-sm">
      <EmptyState
        icon={cover.kind === "failed" ? <Warning /> : <AppWindow />}
        tone={cover.kind === "failed" ? "danger" : "accent"}
        title={said.title}
        description={said.description}
        actions={
          <>
            <Button onClick={onReload}>{cover.kind === "failed" ? t("common:action.tryAgain") : t("frame.reload")}</Button>
            {(cover.kind === "quiet" || cover.kind === "failed") && (
              <Button variant="ghost" onClick={onClose}>
                {t("common:action.close")}
              </Button>
            )}
          </>
        }
      />
    </div>
  )
}
