import { useEffect, useRef, useState } from "react"
import { useRouterState } from "@tanstack/react-router"
import { Dialog as D } from "radix-ui"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { Cpu, FastForward, GraphicsCard, UserFocus } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { NOTICE_FACTS, deviceLabel, noticeFoot, noticeLead } from "@/lib/fast-scan"
import { commands, errorMessage } from "@/lib/ipc"
import { layerOpen, useLayerOpen } from "@/lib/layers"
import { qk, queryClient, useAppInfo, useScanDefaults } from "@/lib/queries"
import { type NoticePhase, useFastScanNotice } from "@/state/fast-scan-notice"
import { useNewProject } from "@/state/new-project"
import { useUpdates } from "@/state/updates"

/*
 * Where new projects start with Fast Scan because of this computer's
 * graphics, AutoScene says so at the start (design-i52 §5.6): over the
 * library, at each start, until "Don't show this again" is ticked or a
 * default is chosen, here or in Settings.
 */

/** After the splash has faded (main.tsx: one frame, then 400 ms). */
export const AFTER_SPLASH_MS = 700
/** Shown in this run: a reload doesn't show it twice. */
export const SHOWN_KEY = "autoscene.fastScanNotice.shown"

function shownThisRun(): boolean {
  try {
    return sessionStorage.getItem(SHOWN_KEY) === "1"
  } catch {
    return false
  }
}

function markShown() {
  try {
    sessionStorage.setItem(SHOWN_KEY, "1")
  } catch {
    // Storage refused: shown once more after a reload, at worst.
  }
}

/** "Don't show this again": stored for the library; if that fails, it may show again next start. */
function storeDone() {
  void commands
    .scanNoticeDone()
    .catch((e) => console.warn("Couldn't store Don't show this again:", e))
    .finally(() => void queryClient.invalidateQueries({ queryKey: qk.scanDefaults }))
}

/** The answer of a choice made here, everywhere the default is shown. */
function chosen(d: Awaited<ReturnType<typeof commands.scanDefaultSet>>) {
  queryClient.setQueryData(qk.scanDefaults, d)
  void queryClient.invalidateQueries({ queryKey: qk.settings })
}

/** Its phase lives in `useFastScanNotice` (the update dialog reads it); this moves it on. */
const setPhase = (phase: NoticePhase) => useFastScanNotice.setState({ phase })

export function FastScanNotice() {
  const { t } = useTranslation(["shell", "common"])
  const { data, isError } = useScanDefaults()
  const { data: info } = useAppInfo()
  const onLibrary = useRouterState({ select: (s) => s.location.pathname === "/" })
  const newProject = useNewProject((s) => s.open)
  const dragging = useNewProject((s) => s.dragging)
  const updating = useUpdates((s) => s.dialogOpen)
  const phase = useFastScanNotice((s) => s.phase)
  const layer = useLayerOpen(phase === "due")
  const [settled, setSettled] = useState(false)
  const [never, setNever] = useState(false)
  // The tick, for the step aside above, which runs outside a render.
  const neverRef = useRef(false)
  const [saving, setSaving] = useState(false)
  const [quiet, setQuiet] = useState(true)
  const focusBack = useRef(true)
  const continueRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(true), AFTER_SPLASH_MS)
    return () => window.clearTimeout(timer)
  }, [])

  // Decide once, from the first answer after the splash. No answer: no notice.
  // Shown already in this run (a reload): not again.
  useEffect(() => {
    if (phase !== "waiting" || !settled) return
    if (isError) setPhase("done")
    else if (data) setPhase(data.notice && !shownThisRun() ? "due" : "done")
  }, [phase, settled, data, isError])

  // Due: open the first moment the library shows with nothing over it.
  // Leaving the library first (a drop that made a project) leaves it for the next start.
  useEffect(() => {
    if (phase !== "due") return
    if (!onLibrary) {
      setPhase("done")
      return
    }
    // `layer` re-runs this when a layer closes; `layerOpen()` also sees one opened before watching began.
    if (newProject || dragging || updating || layer || layerOpen() || !info) return
    markShown()
    setPhase("open")
  }, [phase, onLibrary, newProject, dragging, updating, layer, info])

  // New Project opened (a drop, the palette): one modal at a time, so the
  // notice steps aside as Continue would, and leaves the focus there. Closed
  // in the same update that opens New Project, so its title field gets the
  // focus and the notice's focus trap never holds it back.
  useEffect(
    () =>
      useNewProject.subscribe((s) => {
        if (!s.open || useFastScanNotice.getState().phase !== "open") return
        focusBack.current = false
        setPhase("done")
        if (neverRef.current) storeDone()
      }),
    [],
  )

  if (!data || !info) return null
  const c = data.computer
  const platform = info.platform

  const close = () => {
    setPhase("done")
    if (never) storeDone()
  }

  const turnOff = async () => {
    setSaving(true)
    try {
      chosen(await commands.scanDefaultSet("full"))
      setPhase("done")
      toast(t("shell:fastScan.off"), {
        description: t("shell:fastScan.offDescription"),
        action: {
          label: t("common:action.undo"),
          onClick: () =>
            void commands
              .scanDefaultSet("auto")
              .then(chosen)
              .catch((e) => toast.error(t("shell:fastScan.onFailed"), { description: errorMessage(e) })),
        },
      })
    } catch (e) {
      toast.error(t("shell:fastScan.offFailed"), { description: errorMessage(e) })
    } finally {
      setSaving(false)
    }
  }

  return (
    <D.Root open={phase === "open"} onOpenChange={(o) => !o && close()}>
      <D.Portal>
        <D.Overlay className="fixed inset-0 z-50 bg-scrim backdrop-blur-sm data-[state=open]:animate-fade-in" />
        <D.Content
          data-fast-scan-notice
          onOpenAutoFocus={(e) => {
            e.preventDefault()
            continueRef.current?.focus()
          }}
          onCloseAutoFocus={(e) => {
            if (!focusBack.current) e.preventDefault()
          }}
          className="glass-strong fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100dvh-32px)] w-[min(540px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-2xl focus:outline-none data-[state=open]:animate-pop-in"
        >
          <Art />
          <div className="min-h-0 overflow-y-auto px-7 pt-4 pb-6">
            <p
              aria-hidden
              className="eyebrow flex min-w-0 items-center gap-2 text-fg-2 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-fg-1"
            >
              {c.class === "processor" || platform === "macos" ? <Cpu /> : <GraphicsCard />}
              <span className="truncate">{deviceLabel(c, platform)}</span>
            </p>
            <D.Title className="display mt-1.5 text-3xl text-fg-1">{t("shell:fastScan.title")}</D.Title>
            <D.Description className="mt-2.5 text-lg text-fg-2">
              <span className="sr-only">{deviceLabel(c, platform)}. </span>
              {noticeLead(c, platform)}
            </D.Description>
            <ul className="mt-5 space-y-3">
              {NOTICE_FACTS.map((f) => (
                <li key={f.title} className="flex gap-3.5">
                  <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-[10px] bg-white/[0.06] text-fg-1 [&_svg]:size-[17px]">
                    {f.kind === "faster" ? <FastForward /> : <UserFocus />}
                  </span>
                  <span className="min-w-0 text-md leading-5">
                    <span className="block font-medium text-fg-1">{f.title}</span>
                    <span className="block text-fg-2">{f.body}</span>
                  </span>
                </li>
              ))}
            </ul>
            <p className="mt-5 text-sm text-fg-3">{noticeFoot(data)}</p>
            <div className="mt-6 flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
              <Checkbox label={t("shell:fastScan.never")} checked={never} onCheckedChange={(v) => {
                  neverRef.current = v === true
                  setNever(v === true)
                }} />
              <div className="ml-auto flex items-center gap-2">
                <Button variant="secondary" disabled={saving} onClick={() => void turnOff()}>
                  {t("shell:fastScan.turnOff")}
                </Button>
                <Button
                  ref={continueRef}
                  variant="cta"
                  disabled={saving}
                  // Focus starts here so Enter answers; its ring waits until the keyboard moves it.
                  data-quiet-focus={quiet || undefined}
                  className="data-[quiet-focus]:outline-none"
                  onBlur={() => setQuiet(false)}
                  onClick={close}
                >
                  {t("shell:fastScan.continue")}
                </Button>
              </div>
            </div>
          </div>
        </D.Content>
      </D.Portal>
    </D.Root>
  )
}

/**
 * Who is in `public/fast-scan-notice.webp`, in the dialog's own 540×180: the
 * boxes YuNet, Fast Scan's own detector, found in this picture, with a little
 * room around them. `public/fast-scan-notice-mesh.svg` follows the same faces.
 */
const FACES = [
  { name: "Thomas", x: 68, y: 27, w: 57, h: 71 },
  { name: "Newt", x: 241, y: 36, w: 58, h: 70 },
  { name: "Gally", x: 416, y: 19, w: 56, h: 79 },
]
/** A shot's frames along the picture's foot, and the few Fast Scan looks at (one a second): each sits under a face. */
const FRAMES = 65
const LOOKS = new Set([9, 32, 55])
/** The ruler's inset, in the picture's width. */
const RULER_INSET = 0.052
/** When the sweep passes a frame, as `--animate-scan-sweep` runs it: 200 ms in, 900 ms across. */
const sweepReaches = (frame: number) =>
  Math.round(200 + 900 * (RULER_INSET + ((1 - 2 * RULER_INSET) * frame) / (FRAMES - 1)))

/**
 * The picture: a still fading into the glass and, along its foot, a shot's
 * frames as a ruler, the ones Fast Scan looks at in gold. A line runs along the
 * shot once; behind it the faces it found come up, each over its gold frame.
 */
function Art() {
  return (
    <div aria-hidden className="relative aspect-[3/1] w-full shrink-0 overflow-hidden">
      <div className="absolute inset-0 [mask-image:linear-gradient(180deg,#000_45%,transparent_100%)]">
        <img src="/fast-scan-notice.webp" alt="" className="absolute inset-0 size-full object-cover" />
      </div>
      <div className="absolute inset-0 motion-safe:animate-scan-wipe">
        <img src="/fast-scan-notice-mesh.svg" alt="" className="absolute inset-0 size-full" />
        <svg viewBox="0 0 540 180" className="absolute inset-0 size-full">
          {FACES.map((f) => (
            <Corners key={f.name} {...f} />
          ))}
        </svg>
        {FACES.map((f) => (
          <span
            key={f.name}
            className="absolute -translate-x-1/2 rounded-[5px] bg-black/60 px-1.5 text-2xs font-medium text-white/90"
            style={{ left: `${((f.x + f.w / 2) / 540) * 100}%`, top: `${((f.y + f.h + 4) / 180) * 100}%` }}
          >
            {f.name}
          </span>
        ))}
      </div>
      <span className="absolute inset-y-0 w-px bg-gradient-to-b from-transparent via-accent to-accent opacity-0 shadow-[0_0_10px_rgb(224_180_92/0.7)] motion-safe:animate-scan-sweep" />
      {/* In percent, so a narrow window keeps each gold frame under its face. */}
      <div className="absolute inset-x-[5.2%] bottom-3 flex h-3 items-end justify-between">
        {Array.from({ length: FRAMES }, (_, i) =>
          LOOKS.has(i) ? (
            <span
              key={i}
              data-look
              className="h-3 w-[2px] rounded-full bg-accent shadow-[0_0_8px_rgb(224_180_92/0.7)] motion-safe:animate-[fade-in_400ms_var(--ease-out)_backwards]"
              style={{ animationDelay: `${sweepReaches(i)}ms` }}
            />
          ) : (
            <span key={i} className="h-1.5 w-px bg-white/25" />
          ),
        )}
      </div>
    </div>
  )
}

/** The app's way of marking a found face: gold corners. */
function Corners({ x, y, w, h }: { x: number; y: number; w: number; h: number }) {
  const l = 9
  const d = [
    `M${x} ${y + l}V${y}H${x + l}`,
    `M${x + w - l} ${y}H${x + w}V${y + l}`,
    `M${x + w} ${y + h - l}V${y + h}H${x + w - l}`,
    `M${x + l} ${y + h}H${x}V${y + h - l}`,
  ].join("")
  return (
    <path
      d={d}
      fill="none"
      stroke="var(--accent)"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{ filter: "drop-shadow(0 0 3px rgb(224 180 92 / 0.6))" }}
    />
  )
}
