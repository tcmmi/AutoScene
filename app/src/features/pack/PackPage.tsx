import { type ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { toast } from "sonner"
import { ArrowCounterClockwise, ArrowLeft, ArrowsDownUp, Check, Package, User, Users, X } from "@/components/icons"
import type { CastEdit, Character, MediaItem, PackClip } from "@/bindings"
import { Artwork } from "@/components/ui/artwork"
import { Button } from "@/components/ui/button"
import {
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
} from "@/components/ui/context-menu"
import { EmptyState } from "@/components/ui/empty-state"
import { ErrorState } from "@/components/ui/error-state"
import { type MenuKit, contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { Page } from "@/components/ui/page"
import { Segmented } from "@/components/ui/segmented"
import { Skeleton } from "@/components/ui/skeleton"
import { TopBar } from "@/components/ui/topbar"
import { VirtualGrid } from "@/components/ui/virtual-grid"
import { castEdit, plural } from "@/features/cast/cast"
import { PluginIcon } from "@/features/plugins/PluginIcon"
import { runPackAction } from "@/features/plugins/plugin-actions"
import { type PackActionItem, packActionItems } from "@/features/plugins/plugin-items"
import { ProjectLoadError } from "@/features/project/ProjectLoadError"
import { EXPORT_REQUEST } from "@/lib/actions"
import { shortFormatName } from "@/lib/format"
import { exportWithMenu } from "@/lib/keymap"
import { firstFailure, useCast, useMediaList, usePlugins, usePresets, useSettings } from "@/lib/queries"
import { PACK_SHORTCUTS, useShortcutKeys, useShortcuts } from "@/lib/shortcuts"
import { type BodyVerdict } from "@/features/shots/shots"
import { useUi } from "@/state/ui"
import { PackCard } from "./PackCard"
import { PackExportDialog } from "./PackExportDialog"
import { PackFilters } from "./PackFilters"
import { PackFooter } from "./PackFooter"
import { type Show, between, exportable, handPicked, keyOf, pickable, savable, saveHeading, shown } from "./spec"
import { usePack, usePackExport } from "./use-pack"

const NO_CLIPS: PackClip[] = []
const NO_MEDIA: MediaItem[] = []
/** One at a time: a click on every shot at once replaces the toast of the one before. */
const BULK_TOAST = "pack-bulk"

/** A character's scenepack: filters, every shot of them (the pack's bright), and Export. */
export function PackPage() {
  const { pid, cid } = useParams({ from: "/p/$pid/cast/$cid/pack" })
  // A pack's filters and picks are its character's: another one starts afresh.
  return <PackView key={cid} pid={pid} cid={cid} />
}

function PackView({ pid, cid }: { pid: string; cid: string }) {
  const castQuery = useCast(pid)
  const mediaQuery = useMediaList(pid)
  const character = castQuery.data?.characters.find((c) => c.id === cid)
  const media = mediaQuery.data ?? NO_MEDIA
  const mediaById = useMemo(() => new Map(media.map((m) => [m.id, m])), [media])
  const pack = usePack(pid, cid, mediaById)

  const back = { label: character?.label ?? "Character", to: `/p/${pid}/cast/${cid}` }
  const toolbar = <TopBar back={back} title={character ? `${character.label} · Scenepack` : "Scenepack"} />

  const failure = firstFailure(castQuery, mediaQuery, pack.saved)
  if (failure) {
    return (
      <Page topbar={toolbar}>
        <ProjectLoadError error={failure.error} onRetry={failure.retry} />
      </Page>
    )
  }
  if (castQuery.data && !character) {
    return (
      <Page topbar={toolbar}>
        <EmptyState
          icon={<Users />}
          title="This character isn't in the cast any more"
          description="It may have been merged into someone else."
          actions={
            <Button asChild>
              <Link to="/p/$pid/cast" params={{ pid }}>
                <ArrowLeft /> Back to the cast
              </Link>
            </Button>
          }
          className="mt-24"
        />
      </Page>
    )
  }
  return <Builder pid={pid} cid={cid} character={character} media={media} mediaById={mediaById} pack={pack} toolbar={toolbar} />
}

// Exported for its test: the page's wiring (what a menu entry asks for) is all here.
export function Builder({
  pid,
  cid,
  character,
  media,
  mediaById,
  pack,
  toolbar,
}: {
  pid: string
  cid: string
  character: Character | undefined
  media: MediaItem[]
  mediaById: Map<string, MediaItem>
  pack: ReturnType<typeof usePack>
  toolbar: ReactNode
}) {
  const keys = useShortcutKeys()
  const navigate = useNavigate()
  const { data: settings } = useSettings()
  const { data: presets } = usePresets()
  const { data: plugins } = usePlugins()
  const [show, setShow] = useState<Show>("all")
  const [exportOpen, setExportOpen] = useState(false)
  const clips = pack.preview.data?.clips ?? NO_CLIPS
  const totals = pack.preview.data?.totals ?? { clips: 0, frames: 0, seconds: 0 }
  const visible = useMemo(() => shown(clips, show), [clips, show])
  const picks = useMemo(() => handPicked(clips), [clips])
  const taken = totals.clips
  // One export of the pack at a time: the button shows the one under way.
  const exporting = usePackExport(pid, cid)
  // While a plugin's filter checks shots the pack is not final: Export waits (a failed check doesn't hold it up).
  const canExport = exportable({
    taken,
    current: pack.current,
    character: character != null,
    exporting: exporting != null,
    check: pack.check,
  })
  // A pack action saves a file, so a pack that is being exported can be saved too.
  const canSave = savable({ taken, current: pack.current, character: character != null, check: pack.check })
  const saves = useMemo(() => packActionItems(plugins), [plugins])
  const saveHead = saveHeading({ taken, current: pack.current, check: pack.check })
  const packState = pack.state
  const stamp = pack.preview.data?.stamp
  // An entry already open when a check starts still asks: the backend refuses
  // it with its own words (a toast), instead of an entry that does nothing.
  const save = useCallback(
    (action: PackActionItem) => {
      if (!packState || stamp == null) return
      void runPackAction(action, pid, cid, packState.spec, packState.naming, stamp)
    },
    [packState, stamp, pid, cid],
  )
  const filters = pack.filters
  const bytes = pack.plan.data?.sizes.find((s) => s.presetId === settings?.exportPresetId)?.bytes ?? null
  const format = presets?.find((p) => p.id === settings?.exportPresetId)?.name

  // On macOS the menu's Export item exports the pack while it is open, with
  // the pack's key, and asks for this.
  useEffect(() => {
    exportWithMenu("pack.export")
    return () => exportWithMenu("player.export")
  }, [])
  useEffect(() => {
    const onRequest = () => canExport && setExportOpen(true)
    window.addEventListener(EXPORT_REQUEST, onRequest)
    return () => window.removeEventListener(EXPORT_REQUEST, onRequest)
  }, [canExport])

  // The cards call back with their index; these read the latest list (so
  // the cards, memoised, needn't draw again when it changes).
  const latest = useRef({ visible, setIncluded: pack.setIncluded, restore: pack.restore, show })
  useLayoutEffect(() => {
    latest.current = { visible, setIncluded: pack.setIncluded, restore: pack.restore, show }
  })
  // The last card clicked: Shift+click takes every shot from it.
  const anchor = useRef<string | null>(null)
  const toggle = useCallback((index: number, range: boolean, byKey?: boolean) => {
    const { visible, setIncluded, show } = latest.current
    const clip = visible[index]
    if (!clip) return
    const included = !clip.included
    const from = range ? anchor.current : null
    anchor.current = keyOf(clip)
    void setIncluded(from ? between(visible, from, keyOf(clip)) : [clip], included)
    // In the pack's or the left-out view the card leaves the grid: the
    // keyboard goes on with the one that takes its place.
    if (byKey && show !== "all") {
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const cards = document.querySelectorAll<HTMLElement>("[data-pack-index]")
          const next = [...cards].find((c) => c.dataset.packIndex === String(index)) ?? cards[cards.length - 1]
          next?.focus()
        }),
      )
    }
  }, [])
  const open = useCallback(
    (index: number) => {
      const clip = latest.current.visible[index]
      if (clip) void navigate({ to: "/p/$pid/timeline/$mid", params: { pid, mid: clip.shot.mediaId }, search: { f: clip.start } })
    },
    [navigate, pid],
  )
  // Bodies that aren't them (or aren't anyone) leave the pack, and stay out after a new analysis.
  const characterId = character?.id
  const label = character?.label
  const onBody = useCallback(
    (bodies: number[], verdict: BodyVerdict) => {
      if (!characterId) return
      const edit: CastEdit =
        verdict === "not_them" ? { op: "body_not_them", bodies, characterId } : { op: "not_a_person", bodies }
      void castEdit(pid, edit).then((r) => {
        if (r !== undefined) toast(verdict === "not_them" ? `Not ${label ?? "them"}: out of the pack` : "Not a person: out of the pack")
      })
    },
    [pid, characterId, label],
  )
  const faceless = useMemo(() => clips.some((c) => c.shot.faceless), [clips])
  // The menu's entries act on the clip itself (the grid may have moved).
  const { setIncluded, clearPicks } = pack
  const onPick = useCallback(
    (clip: PackClip, include: boolean | null) => void (include == null ? clearPicks([clip]) : setIncluded([clip], include)),
    [setIncluded, clearPicks],
  )
  const onWatch = useCallback(
    (clip: PackClip) =>
      void navigate({ to: "/p/$pid/timeline/$mid", params: { pid, mid: clip.shot.mediaId }, search: { f: clip.start } }),
    [navigate, pid],
  )

  // What can be done to every shot at once: in the ⋯ and the right-click alike, and by key.
  const free = useMemo(() => clips.filter(pickable), [clips])
  const canBulk = pack.ready && free.length > 0
  const allIn = free.every((c) => c.included)
  const noneIn = !free.some((c) => c.included)
  // A click on every shot at once rewrites the picks, and says so with a way back: the shots as they were.
  const limited = pack.state?.spec.limit != null
  const bulk = (said: string, run: () => Promise<boolean>, pinned = false) => {
    const before = clips
    void run().then((changed) => {
      if (!changed) return
      toast(said, {
        id: BULK_TOAST,
        description: pinned && limited ? "Picked by hand: the limit leaves them alone." : undefined,
        action: { label: "Undo", onClick: () => void latest.current.restore(before) },
      })
    })
  }
  const takeAll = () => bulk("Took every shot in", () => pack.setEach(clips, () => true), true)
  const leaveAll = () => bulk("Left every shot out", () => pack.setEach(clips, () => false), true)
  const invert = () => bulk("Inverted the pack", () => pack.invertIncluded(clips), true)
  const handBack = () =>
    bulk(`Handed ${plural(picks.length, "shot")} back to the filters`, () => pack.clearPicks(picks))
  useShortcuts(PACK_SHORTCUTS, {
    export: () => (canExport ? setExportOpen(true) : false),
    takeAll: () => (canBulk ? (allIn ? leaveAll() : takeAll()) : false),
    leaveAll: () => (canBulk && !noneIn ? leaveAll() : false),
    invert: () => (canBulk ? invert() : false),
  })
  const bulkItems = ({ Item }: MenuKit) => (
    <>
      {/* With every shot in the key leaves them out: it isn't promised on an entry that takes. */}
      <Item disabled={!canBulk || allIn} onSelect={takeAll} shortcut={allIn ? undefined : keys("pack.takeAll")}>
        <Check /> Take every shot
      </Item>
      <Item disabled={!canBulk || noneIn} onSelect={leaveAll} shortcut={keys("pack.leaveAll")}>
        <X /> Leave every shot out
      </Item>
      <Item disabled={!canBulk} onSelect={invert} shortcut={keys("pack.invert")}>
        <ArrowsDownUp /> Invert the pack
      </Item>
      {picks.length > 0 && (
        <Item onSelect={handBack}>
          <ArrowCounterClockwise /> Hand every shot back to the filters
        </Item>
      )}
    </>
  )

  // The pack's empty space: export it, give the hand-picked shots back, choose what shows.
  const pageMenu = (
    <>
      <ContextMenuItem disabled={!canExport} onSelect={() => setExportOpen(true)} shortcut={keys("pack.export")}>
        <Package /> Export the pack…
      </ContextMenuItem>
      <ContextMenuSeparator />
      {bulkItems(contextKit)}
      {/* Plugins save the pack as a file of theirs, before anything is rendered: a group of their own, after the app's. */}
      {saves.length > 0 && (
        <>
          <ContextMenuSeparator />
          <ContextMenuLabel>{saveHead}</ContextMenuLabel>
          {saves.map((a) => (
            <ContextMenuItem key={a.key} disabled={!canSave} onSelect={() => save(a)}>
              <PluginIcon src={a.icon} /> {a.title}
              <span className="ml-auto pl-4 text-xs text-fg-3">{a.pluginName}</span>
            </ContextMenuItem>
          ))}
        </>
      )}
      <ContextMenuSeparator />
      <ContextMenuLabel>Show</ContextMenuLabel>
      <ContextMenuRadioGroup value={show} onValueChange={(v) => setShow(v as Show)}>
        <ContextMenuRadioItem value="all">All shots</ContextMenuRadioItem>
        <ContextMenuRadioItem value="in">In the pack</ContextMenuRadioItem>
        <ContextMenuRadioItem value="out">Left out</ContextMenuRadioItem>
      </ContextMenuRadioGroup>
    </>
  )

  return (
    <div className="relative h-full">
      <Page topbar={toolbar} menu={pageMenu}>
        <Header character={character} shots={clips.length} />
        {pack.state ? (
          <PackFilters
            spec={pack.state.spec}
            media={media}
            faceless={faceless}
            filters={filters}
            plugins={plugins}
            onChange={pack.setSpec}
          />
        ) : (
          <div className="h-[calc(3.75rem+1px)] border-b border-hairline" />
        )}
        <section className="px-10 pt-5 pb-32">
          <div className="mb-4 flex flex-wrap items-center justify-between gap-4">
            <Segmented<Show>
              aria-label="Show"
              value={show}
              onChange={setShow}
              options={[
                { value: "all", label: `All ${clips.length}` },
                { value: "in", label: `In the pack ${taken}` },
                { value: "out", label: `Left out ${clips.length - taken}` },
              ]}
            />
            {clips.length > 0 && (
              <span className="text-sm text-fg-3">Click to take a shot in or out · Shift for every shot since · Enter to watch</span>
            )}
          </div>
          {pack.preview.isPending ? (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-4">
              {Array.from({ length: 12 }, (_, i) => (
                <Skeleton key={i} className="aspect-video rounded-xl" />
              ))}
            </div>
          ) : pack.preview.isError && pack.preview.data === undefined ? (
            <ErrorState
              error={pack.preview.error}
              title="The pack couldn't be made"
              onRetry={() => void pack.preview.refetch()}
            />
          ) : clips.length === 0 ? (
            <p className="text-md text-fg-3">
              {character && character.waiting > 0
                ? "Their episode is being analysed again; the shots come back when it's done."
                : "No shots of them yet."}
            </p>
          ) : visible.length === 0 ? (
            <p className="text-md text-fg-3">{show === "in" ? "The pack takes none of the shots. Loosen a filter or click shots to take them." : "The pack takes every shot."}</p>
          ) : (
            <VirtualGrid
              items={visible}
              minItemWidth={240}
              itemHeight={(w) => (w * 9) / 16}
              getKey={keyOf}
              renderItem={(c, i) => (
                <PackCard
                  clip={c}
                  index={i}
                  media={mediaById.get(c.shot.mediaId)}
                  name={character?.label}
                  filter={c.filter != null ? filters?.[c.filter] : undefined}
                  onToggle={toggle}
                  onOpen={open}
                  onWatch={onWatch}
                  onPick={onPick}
                  onBody={onBody}
                />
              )}
            />
          )}
        </section>
        {character && (
          <PackExportDialog pid={pid} character={character} pack={pack} open={exportOpen} onOpenChange={setExportOpen} />
        )}
      </Page>
      {clips.length > 0 && (
        <PackFooter
          totals={totals}
          bytes={bytes}
          format={format ? shortFormatName(format) : null}
          picks={picks.length}
          canExport={canExport}
          exporting={exporting}
          check={pack.check}
          items={bulkItems(dropdownKit)}
          actions={saves}
          canSave={canSave}
          saveHeading={saveHead}
          onSave={save}
          onClearPicks={handBack}
          onTryAgain={pack.tryAgain}
          onExport={() => setExportOpen(true)}
          onShowExport={() => useUi.getState().setActivityOpen(true)}
        />
      )}
    </div>
  )
}

function Header({ character: c, shots }: { character?: Character; shots: number }) {
  return (
    <header className="flex items-center gap-5 px-10 pt-8 pb-6">
      <Artwork
        key={c?.id}
        path={c?.coverSmall}
        className="size-20 shrink-0 rounded-[20px] shadow-[0_20px_40px_-16px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.12)]"
        imgClassName="object-[50%_30%]"
        fallback={
          <div className="flex size-full items-center justify-center bg-white/[0.06] text-white/60 [&_svg]:size-8">
            <User />
          </div>
        }
      />
      <div className="min-w-0">
        <p className="eyebrow">Scenepack</p>
        <h1 className="display truncate text-5xl leading-[1.05] text-fg-1">{c?.label ?? " "}</h1>
        {c && (
          <p className="mt-1 text-sm text-fg-3">
            {plural(shots, "shot")} in {plural(c.media, "episode")} · the pack takes the bright ones
          </p>
        )}
      </div>
    </header>
  )
}
