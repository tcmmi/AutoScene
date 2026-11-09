import { useEffect, useMemo, useRef, useState } from "react"
import { useNavigate } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { CircleNotch, FilePlus, FilmSlate, FilmStrip, FolderPlus, LinkSimple, Television, Warning, X } from "@/components/icons"
import { toast } from "sonner"
import type { ContentKind, ImportCandidate, LinkItem, ProjectKind } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Tooltip } from "@/components/ui/tooltip"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Field, Input } from "@/components/ui/input"
import { Segmented } from "@/components/ui/segmented"
import { Switch } from "@/components/ui/switch"
import { newProjectHint, startsFast } from "@/lib/fast-scan"
import { footageBlurb } from "@/lib/footage"
import { formatBytes } from "@/lib/format"
import { say, t as translate } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { fileStem, pickFolder, pickVideoFiles } from "@/lib/media"
import { usePlugins, useScanDefaults, useSettings } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { useNewProject } from "@/state/new-project"
import { sourceItems } from "@/features/plugins/plugin-items"
import { FootagePicker } from "./FootagePicker"
import { LinkSourcesPanel } from "./LinkSourcesPanel"
import { queueLinks } from "./queue-links"
import { suggestTitle } from "./suggest-title"

export function NewProjectDialog() {
  const open = useNewProject((s) => s.open)
  const setOpen = useNewProject((s) => s.setOpen)
  const seed = useNewProject((s) => s.seed)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {open && <NewProjectForm seed={seed} onDone={() => setOpen(false)} />}
    </Dialog>
  )
}

function NewProjectForm({ seed, onDone }: { seed: string[]; onDone: () => void }) {
  const { t } = useTranslation(["library", "common"])
  const navigate = useNavigate()
  const [title, setTitle] = useState("")
  const titleTouched = useRef(false)
  const [kind, setKind] = useState<ProjectKind>("series")
  const kindTouched = useRef(false)
  const [content, setContent] = useState<ContentKind>("live")
  // Fast Scan follows the footage until the person flips it.
  const [touched, setTouched] = useState<boolean | null>(null)
  const { data: scanDefaults, isError: scanUnknown } = useScanDefaults()
  const { data: settings } = useSettings()
  const findBodies = settings?.findBodies ?? false
  // Without the defaults (they failed), the library's own rule as far as the
  // settings tell it: 3D animation keeps the full scan.
  const fastScan =
    touched ?? (scanDefaults ? startsFast(scanDefaults, content) : content !== "animation" && (settings?.fastScan ?? false))
  const scanKnown = scanDefaults != null || scanUnknown
  // Dropped files (`seed`, which grows when more are dropped on the open
  // dialog) plus picked ones; nothing typed is lost when more arrive.
  const [picked, setPicked] = useState<string[]>([])
  const sources = useMemo(() => [...new Set([...seed, ...picked])], [seed, picked])
  const [scan, setScan] = useState<{ key: string; found: ImportCandidate[] }>({ key: "", found: [] })
  const [removed, setRemoved] = useState<Set<string>>(new Set())
  const [creating, setCreating] = useState(false)
  // Links a plugin's source downloads once the project exists, and whether their field is open.
  const { data: plugins } = usePlugins()
  const linkSources = useMemo(() => sourceItems(plugins), [plugins])
  const [links, setLinks] = useState<LinkItem[]>([])
  const [linking, setLinking] = useState(false)
  // Files dragged over the window land here: the list says so while they hover.
  const dragging = useNewProject((s) => s.dragging)
  const scanKey = JSON.stringify([kind, sources])
  const scanning = sources.length > 0 && scan.key !== scanKey
  const candidates = useMemo(() => (sources.length > 0 ? scan.found : []), [sources, scan])

  // Re-scan whenever sources or the project type change (type decides episode parsing).
  useEffect(() => {
    if (sources.length === 0) return
    let stale = false
    commands
      .importScan(sources, kind, null)
      .then((found) => {
        if (stale) return
        setScan({ key: JSON.stringify([kind, sources]), found })
        const usable = found.filter((c) => !c.refused)
        if (!kindTouched.current) {
          const numbered = usable.some((c) => c.episode != null)
          const next: ProjectKind = numbered || usable.length > 1 ? "series" : "movie"
          if (next !== kind) setKind(next)
        }
        if (!titleTouched.current && usable.length > 0) {
          const first = sources[0]
          const isFile = usable.some((c) => c.path === first)
          setTitle(suggestTitle(first, isFile) || fileStem(usable[0].fileName))
        }
      })
      .catch((e) => {
        if (stale) return
        setScan({ key: JSON.stringify([kind, sources]), found: [] })
        toast.error(translate("library:newProject.readFailed"), { description: errorMessage(e) })
      })
    return () => {
      stale = true
    }
  }, [sources, kind])

  // A file that can't be imported is listed with why, never added.
  const chosen = useMemo(() => candidates.filter((c) => !c.refused && !removed.has(c.path)), [candidates, removed])
  const totalBytes = chosen.reduce((n, c) => n + (c.sizeBytes ?? 0), 0)
  const addSources = (paths: string[]) => paths.length && setPicked((s) => [...new Set([...s, ...paths])])
  const addLinks = (items: LinkItem[]) => {
    setLinks((had) => {
      const next = [...had, ...items.filter((i) => !had.some((h) => h.link === i.link))]
      // Without files, links guess the type as files do: more than one, a series.
      // i18n-ignore: project kinds, not words
      const guess: ProjectKind = next.length > 1 ? "series" : "movie"
      if (!kindTouched.current && candidates.length === 0) setKind(guess)
      return next
    })
    setLinking(false)
  }
  const linksButton = (small: boolean) =>
    linkSources.length > 0 && (
      <Button size={small ? "sm" : undefined} variant="ghost" onClick={() => setLinking(true)}>
        <LinkSimple /> {small ? t("newProject.links") : t("newProject.addLinks")}
      </Button>
    )
  const linksPanel = linking && (
    <LinkSourcesPanel sources={linkSources} onAdd={addLinks} onCancel={() => setLinking(false)} />
  )

  const create = async () => {
    setCreating(true)
    try {
      const req = { title: title.trim(), kind, content, files: chosen.map((c) => c.path) }
      // What the switch shows; `null` (the library's own default) only while it waits for it.
      const p = await commands.projectCreate(req, scanKnown ? fastScan : null)
      if (links.length > 0) await queueLinks(p.id, links)
      onDone()
      void navigate({ to: "/p/$pid/episodes", params: { pid: p.id } })
    } catch (e) {
      toast.error(t("newProject.createFailed"), { description: errorMessage(e) })
    } finally {
      setCreating(false)
    }
  }

  return (
    <DialogContent
      title={t("newProject.title")}
      description={t("newProject.description")}
      className="w-[min(760px,calc(100vw-32px))]"
    >
      <div className="grid grid-cols-[1fr_minmax(15rem,auto)] items-start gap-4">
        <Field label={t("newProject.titleLabel")} htmlFor="np-title">
          <Input
            id="np-title"
            autoFocus
            value={title}
            placeholder={t("newProject.titlePlaceholder")}
            onChange={(e) => {
              titleTouched.current = true
              setTitle(e.target.value)
            }}
          />
        </Field>
        <Field label={t("newProject.typeLabel")} className="items-start">
          <Segmented<ProjectKind>
            aria-label={t("newProject.typeAria")}
            value={kind}
            onChange={(v) => {
              kindTouched.current = true
              setKind(v)
            }}
            options={[
              { value: "series", label: <><Television /> {t("newProject.series")}</> },
              { value: "movie", label: <><FilmStrip /> {t("newProject.movie")}</> },
            ]}
          />
        </Field>
        <Field label={t("footage.label")} hint={footageBlurb(content)} className="items-start">
          <FootagePicker value={content} onChange={setContent} />
        </Field>
        <Field
          label={t("newProject.fastScan")}
          htmlFor="np-fast-scan"
          hint={scanKnown ? newProjectHint(content, findBodies, scanDefaults) : undefined}
          className="items-start"
        >
          {/* As tall as the footage picker beside it, so their lines start level. */}
          <div className="flex h-9 items-center">
            <Switch id="np-fast-scan" checked={fastScan} disabled={!scanKnown} onCheckedChange={setTouched} />
          </div>
        </Field>
      </div>

      <div
        className={cn(
          "relative mt-6 overflow-hidden rounded-2xl border border-white/[0.08] bg-black/25 transition-colors duration-[var(--dur-fast)]",
          dragging && "border-accent-line",
        )}
      >
        {dragging && (
          <div className="pointer-events-none absolute inset-2 z-10 flex animate-fade-in flex-col items-center justify-center rounded-xl border-2 border-dashed border-accent-line bg-black/70 text-center backdrop-blur-md">
            <span className="flex size-12 items-center justify-center rounded-full bg-accent-subtle text-accent [&_svg]:size-6">
              <FilePlus />
            </span>
            <p className="mt-3 text-lg font-medium text-fg-1">{t("newProject.dropToAdd")}</p>
            <p className="mt-1 text-sm text-fg-3">{t("newProject.dropToAddKinds")}</p>
          </div>
        )}
        {candidates.length === 0 && links.length === 0 && linking ? (
          linksPanel
        ) : candidates.length === 0 && links.length === 0 ? (
          // Lower on short windows, so the dialog keeps a margin round it.
          <div className="m-2 flex flex-col items-center rounded-xl border-2 border-dashed border-white/[0.08] px-6 py-12 text-center [@media(max-height:720px)]:py-8">
            {scanning ? (
              <CircleNotch className="size-6 animate-spin text-accent" />
            ) : (
              <>
                <div className="flex size-14 items-center justify-center rounded-2xl bg-white/[0.06] text-fg-1 [&_svg]:size-7">
                  <FilmSlate />
                </div>
                <p className="mt-4 text-lg font-medium text-fg-1">{t("newProject.dropHere")}</p>
                <p className="mt-1 text-sm text-fg-3">{t("newProject.dropFormats")}</p>
              </>
            )}
            <div className="mt-6 flex gap-2.5">
              <Button onClick={() => void pickFolder(t("newProject.chooseFolder")).then((p) => p && addSources([p]))}>
                <FolderPlus /> {t("newProject.addFolder")}
              </Button>
              <Button variant="ghost" onClick={() => void pickVideoFiles().then(addSources)}>
                <FilePlus /> {t("newProject.addFiles")}
              </Button>
              {linksButton(false)}
            </div>
          </div>
        ) : (
          <>
            <div className="max-h-[clamp(112px,calc(100dvh-456px),320px)] overflow-y-auto p-1.5">
              <table className="w-full text-md">
                <tbody>
                  {candidates.map((c, i) => {
                    const off = removed.has(c.path)
                    return (
                      // Two names that aren't Unicode can read the same.
                      <tr key={c.refused ? `${i}:${c.path}` : c.path} className={cn("group", off && "opacity-40")}>
                        <td className="w-24 whitespace-nowrap rounded-l-xl py-2 pl-3 group-hover:bg-white/[0.04]">
                          <span className={cn("timecode text-sm", c.label ? "text-fg-1" : "text-fg-3")}>
                            {c.label || "—"}
                          </span>
                        </td>
                        <td className="max-w-0 py-2 group-hover:bg-white/[0.04]">
                          <Tooltip label={c.path} side="top" delay={600}>
                            <div className={cn("truncate", c.refused ? "text-fg-3" : "text-fg-2")}>{c.fileName}</div>
                          </Tooltip>
                          {c.refused && (
                            <div className="mt-0.5 flex items-start gap-1.5 text-sm text-danger">
                              <Warning className="mt-0.5 size-3.5 shrink-0" />
                              <span>{say(c.refused)}</span>
                            </div>
                          )}
                        </td>
                        <td className="timecode w-24 whitespace-nowrap py-2 pr-2 text-right text-sm text-fg-3 group-hover:bg-white/[0.04]">
                          {formatBytes(c.sizeBytes ?? 0)}
                        </td>
                        <td className="w-10 rounded-r-xl py-1 pr-1.5 group-hover:bg-white/[0.04]">
                          {!c.refused && (
                            <Tooltip label={t(off ? "newProject.include" : "newProject.skip")} side="left">
                              <button
                                aria-label={t(off ? "newProject.includeFile" : "newProject.skipFile", { name: c.fileName })}
                                onClick={() =>
                                  setRemoved((r) => {
                                    const n = new Set(r)
                                    if (n.has(c.path)) n.delete(c.path)
                                    else n.add(c.path)
                                    return n
                                  })
                                }
                                className="flex size-7 items-center justify-center rounded-full text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-white/[0.1] hover:text-fg-1 focus-visible:opacity-100"
                              >
                                <X className="size-3.5" />
                              </button>
                            </Tooltip>
                          )}
                        </td>
                      </tr>
                    )
                  })}
                  {links.map((l) => (
                    <tr key={l.link} className="group">
                      <td className="w-24 whitespace-nowrap rounded-l-xl py-2 pl-3 group-hover:bg-white/[0.04]">
                        <LinkSimple className="size-4 text-fg-3" />
                      </td>
                      <td className="max-w-0 py-2 group-hover:bg-white/[0.04]">
                        <Tooltip label={l.link} side="top" delay={600}>
                          <div className="truncate text-fg-2">{l.short}</div>
                        </Tooltip>
                        <div className="mt-0.5 truncate text-sm text-fg-3">{t("newProject.linkWaits")}</div>
                      </td>
                      <td className="w-24 py-2 pr-2 text-right text-sm text-fg-3 group-hover:bg-white/[0.04]">—</td>
                      <td className="w-10 rounded-r-xl py-1 pr-1.5 group-hover:bg-white/[0.04]">
                        <Tooltip label={t("newProject.removeLink")} side="left">
                          <button
                            aria-label={t("newProject.removeLinkNamed", { link: l.short })}
                            onClick={() => setLinks((had) => had.filter((h) => h.link !== l.link))}
                            className="flex size-7 items-center justify-center rounded-full text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-white/[0.1] hover:text-fg-1 focus-visible:opacity-100"
                          >
                            <X className="size-3.5" />
                          </button>
                        </Tooltip>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {linksPanel && <div className="border-t border-white/[0.06]">{linksPanel}</div>}
            <div className="flex items-center justify-between border-t border-white/[0.06] bg-white/[0.02] px-4 py-2.5">
              <span className="text-sm text-fg-2">
                {(chosen.length > 0 || links.length === 0) &&
                  `${t("newProject.videos", { count: chosen.length })} · ${formatBytes(totalBytes)}`}
                {chosen.length > 0 && links.length > 0 && " · "}
                {links.length > 0 && t("newProject.linkCount", { count: links.length })}
                {scanning && <CircleNotch className="ml-2 inline size-3 animate-spin" />}
              </span>
              <div className="flex gap-1">
                <Button size="sm" variant="ghost" onClick={() => void pickFolder().then((p) => p && addSources([p]))}>
                  <FolderPlus /> {t("newProject.folder")}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => void pickVideoFiles().then(addSources)}>
                  <FilePlus /> {t("newProject.files")}
                </Button>
                {!linking && linksButton(true)}
              </div>
            </div>
          </>
        )}
      </div>
      {links.length > 0 && <p className="mt-2 text-sm text-fg-3">{t("newProject.linksNote")}</p>}

      <DialogFooter>
        <Button variant="ghost" onClick={onDone}>
          {t("common:action.cancel")}
        </Button>
        <Button variant="primary" busy={creating} disabled={!title.trim() || (chosen.length === 0 && links.length === 0)} onClick={() => void create()}>
          {t("newProject.create")}
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}
