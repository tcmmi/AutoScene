import { useEffect, useMemo, useRef, useState } from "react"
import { Link, useNavigate, useParams, useSearch } from "@tanstack/react-router"
import { Trans, useTranslation } from "react-i18next"
import { CaretRight, Check, Checks, Play, Prohibit, User, X } from "@/components/icons"
import type { Character } from "@/bindings"
import { Artwork } from "@/components/ui/artwork"
import { Avatar } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { EmptyState } from "@/components/ui/empty-state"
import { Kbd } from "@/components/ui/kbd"
import { Page } from "@/components/ui/page"
import { Progress } from "@/components/ui/progress"
import { TopBar } from "@/components/ui/topbar"
import { ProjectLoadError } from "@/features/project/ProjectLoadError"
import { assetUrl } from "@/lib/media"
import { firstFailure, portraitQuery, queryClient, useCast, useCastTracks, useMediaList, usePortrait } from "@/lib/queries"
import { timecode } from "@/lib/timecode"
import { REVIEW_SHORTCUTS, digitOf, useShortcutKeys, useShortcuts } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"
import { castEdit, characterLabel } from "./cast"

/**
 * Faces the analysis wasn't sure about, one at a time: confirm the
 * suggestion, reject it, call it not a face, or give it to someone else.
 * Every answer is a pin or a forbid, so it survives any later analysis.
 */
/** How long a key waits after the face (or its suggestion) on screen changed. */
const SETTLE_MS = 350

export function ReviewPage() {
  const { t } = useTranslation("cast")
  const { pid } = useParams({ from: "/p/$pid/cast/review" })
  const { c } = useSearch({ from: "/p/$pid/cast/review" })
  const navigate = useNavigate()
  const keys = useShortcutKeys()
  const castQuery = useCast(pid)
  const mediaQuery = useMediaList(pid)
  const { data: cast } = castQuery
  const { data: media } = mediaQuery
  const queue = useCastTracks(pid, c ? { kind: "unsure", id: c } : { kind: "all_unsure" })
  const [skipped, setSkipped] = useState<Set<number>>(new Set())
  // Answered faces leave at once, before the list is fetched again: a quick
  // second key must not land on the face just decided.
  const [answered, setAnswered] = useState<Set<number>>(new Set())
  const [done, setDone] = useState(0)
  const [busy, setBusy] = useState(false)
  // The guard itself: two answers before the page draws again would both
  // still see `busy` as it was (which only disables the buttons).
  const answering = useRef(false)
  // Quick picks 1–9: the biggest characters when the review started, in a
  // fixed order (numbers don't move as answers change the screen time).
  const [pickIds, setPickIds] = useState<string[] | null>(null)
  if (pickIds === null && cast) setPickIds(cast.characters.filter((x) => !x.hidden).slice(0, 9).map((x) => x.id))

  const items = (queue.data ?? []).filter((t) => !answered.has(t.id))
  // The face on screen stays until it is answered or skipped: a decision
  // places other faces again a moment later, and the list that comes back
  // may start elsewhere.
  const [currentId, setCurrentId] = useState<number | null>(null)
  const current =
    items.find((t) => t.id === currentId && !skipped.has(t.id)) ?? items.find((t) => !skipped.has(t.id))
  if ((current?.id ?? null) !== currentId) setCurrentId(current?.id ?? null)
  const byId = useMemo(() => new Map((cast?.characters ?? []).map((x) => [x.id, x])), [cast])
  const suggestion = current?.characterId ? byId.get(current.characterId) : undefined
  // Faces are placed again a moment after each decision: what is on screen
  // can change under a finger. A key meant for what was there before
  // doesn't answer what came instead, and every answer names the character
  // that was shown (not whatever the face's suggestion is by then).
  const shown = `${current?.id ?? ""}:${suggestion?.id ?? ""}`
  const shownAt = useRef(0)
  useEffect(() => {
    shownAt.current = performance.now()
  }, [shown])
  const settled = () => performance.now() - shownAt.current > SETTLE_MS
  // A pick merged away meanwhile leaves its number empty.
  const picks = (pickIds ?? []).map((id) => byId.get(id))
  const m = current ? media?.find((x) => x.id === current.mediaId) : undefined
  const total = done + items.length
  // Shown big: the face's sharp portrait, the thumbnail until it's made.
  const portrait = usePortrait(pid, current)
  // The next faces' portraits are made while this one is decided.
  const upcoming = items.filter((t) => !skipped.has(t.id) && t.id !== current?.id).slice(0, 2)
  const upcomingIds = upcoming.map((t) => t.id).join(",")
  useEffect(() => {
    for (const t of upcoming) void queryClient.prefetchQuery(portraitQuery(pid, t))
    // Once per new pair of faces (`upcomingIds`), not per render.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [pid, upcomingIds])

  const answer = async (id: number, fn: () => Promise<unknown>) => {
    if (answering.current) return
    answering.current = true
    setBusy(true)
    setAnswered((s) => new Set(s).add(id))
    let r: unknown
    try {
      r = await fn()
    } finally {
      answering.current = false
      setBusy(false)
    }
    if (r !== undefined) {
      setDone((d) => d + 1)
    } else {
      setAnswered((s) => {
        const next = new Set(s)
        next.delete(id)
        return next
      })
    }
  }

  // The answers — on the keyboard (REVIEW_SHORTCUTS) and in the menu. One
  // with nothing to answer here (no suggestion, an empty quick pick) says
  // so (`false`), and its key passes through.
  const decide = {
    yes: () => {
      if (!current || !suggestion) return false
      if (!settled()) return
      void answer(current.id, () => castEdit(pid, { op: "assign", tracks: [current.id], characterId: suggestion.id }))
    },
    no: () => {
      if (!current || !suggestion) return false
      if (!settled()) return
      void answer(current.id, () => castEdit(pid, { op: "not_them", tracks: [current.id], characterId: suggestion.id }))
    },
    notFace: () => {
      if (!current) return false
      if (!settled()) return
      void answer(current.id, () => castEdit(pid, { op: "not_a_face", tracks: [current.id] }))
    },
    skip: () => {
      if (!current) return false
      setSkipped((s) => new Set(s).add(current.id))
    },
    pick: (n: number) => {
      const target = picks[n]
      if (!current || !target) return false
      if (!settled()) return
      void answer(current.id, () => castEdit(pid, { op: "assign", tracks: [current.id], characterId: target.id }))
    },
    watch: () => {
      if (!current) return false
      void navigate({ to: "/p/$pid/timeline/$mid", params: { pid, mid: current.mediaId }, search: { f: current.start } })
    },
  }
  useShortcuts(
    REVIEW_SHORTCUTS,
    { ...decide, pick: (e) => decide.pick(Number(digitOf(e)) - 1) },
    { enabled: current !== undefined },
  )

  // The face is the page: anywhere on it, the answers come first.
  const answers = current && (
    <>
      {suggestion && (
        <ContextMenuItem onSelect={() => void decide.yes()} shortcut={keys("review.yes")}>
          <Check /> {t("review.yes", { name: characterLabel(suggestion) })}
        </ContextMenuItem>
      )}
      {suggestion && (
        <ContextMenuItem onSelect={() => void decide.no()} shortcut={keys("review.no")}>
          <X /> {t("review.no")}
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={() => void decide.notFace()} shortcut={keys("review.notFace")}>
        <Prohibit /> {t("faces.notAFace")}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => void decide.skip()} shortcut={keys("review.skip")}>
        <CaretRight /> {t("review.skipForNow")}
      </ContextMenuItem>
      <ContextMenuSeparator />
      {picks.map(
        (p, i) =>
          p &&
          p.id !== suggestion?.id && (
            <ContextMenuItem key={p.id} onSelect={() => void decide.pick(i)} shortcut={[String(i + 1)]}>
              <User /> {t("review.itsThem", { name: characterLabel(p) })}
            </ContextMenuItem>
          ),
      )}
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={() => void decide.watch()}>
        <Play /> {t("faces.watchMoment")}
      </ContextMenuItem>
    </>
  )

  const toolbar = <TopBar back={{ label: t("title"), to: `/p/${pid}/cast` }} title={t("review.title")} />

  const failure = firstFailure(queue, castQuery, mediaQuery)
  if (failure) {
    return (
      <Page topbar={toolbar}>
        <ProjectLoadError error={failure.error} onRetry={failure.retry} />
      </Page>
    )
  }

  // Only a queue that loaded can be empty.
  if (queue.isSuccess && !current) {
    return (
      <Page topbar={toolbar}>
        <EmptyState
          icon={<Checks />}
          title={done > 0 ? t("review.allDone") : t("review.nothing")}
          description={
            skipped.size > 0
              ? t("review.skipped", { count: skipped.size })
              : done > 0
                ? t("review.saved", { count: done })
                : t("review.waiting")
          }
          actions={
            skipped.size > 0 ? (
              <Button onClick={() => setSkipped(new Set())}>{t("review.showSkipped")}</Button>
            ) : (
              <Button asChild>
                <Link to="/p/$pid/cast" params={{ pid }}>
                  {t("character.backToCast")}
                </Link>
              </Button>
            )
          }
          className="mt-24"
        />
      </Page>
    )
  }

  return (
    <Page topbar={toolbar} menu={answers}>
      {/* The face being decided lights the room. */}
      {current && (
        <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-screen overflow-hidden opacity-40">
          <img
            key={current.id}
            src={assetUrl(current.thumb)}
            alt=""
            className="size-full scale-125 animate-fade-in object-cover blur-[90px] saturate-150"
          />
          <div className="absolute inset-0 bg-gradient-to-b from-canvas/40 via-canvas/70 to-canvas" />
        </div>
      )}
      <div className="relative mx-auto flex max-w-[920px] flex-col px-10 pt-6 pb-12">
        <div className="flex items-center gap-4">
          <Progress className="flex-1" value={total > 0 ? done / total : null} />
          <span className="timecode shrink-0 text-sm text-fg-2">
            {t("review.progress", { done, total })}
            {skipped.size > 0 && <span className="text-fg-3"> · {t("review.skippedCount", { count: skipped.size })}</span>}
          </span>
        </div>

        {current && (
          <>
            <h2 className="display mt-[min(40px,4vh)] text-center text-5xl text-fg-1">
              {suggestion ? (
                <Trans
                  t={t}
                  i18nKey="review.isThis"
                  values={{ name: characterLabel(suggestion) }}
                  components={{ x: <span className={cn(!suggestion.name && "italic")} /> }}
                />
              ) : (
                t("review.whoIsThis")
              )}
            </h2>
            {suggestion && (
              <p className="mt-2 text-center text-md text-fg-2">
                {(current.score ?? 0) >= 0.45 ? t("review.lookALot") : t("review.mightBe")}
              </p>
            )}

            <div className="mt-[min(40px,4vh)] flex items-center justify-center gap-8">
              <figure className="flex flex-col items-center">
                {/* The app's picture: a failed one falls back quietly, never the browser's broken-image icon. */}
                <Artwork
                  key={current.id}
                  path={portrait.data ?? current.thumb}
                  alt={t("review.faceAlt")}
                  className="size-[min(300px,32vh)] rounded-[32px] bg-inset shadow-[0_40px_80px_-30px_rgb(0_0_0/0.9),inset_0_0_0_1px_rgb(255_255_255/0.12)]"
                  imgClassName="object-[50%_30%]"
                  fallback={
                    <div className="flex size-full items-center justify-center text-fg-disabled [&_svg]:size-12">
                      <User />
                    </div>
                  }
                />
                <figcaption className="mt-3 flex items-center gap-2 text-sm text-fg-3">
                  <span className="timecode">
                    {m?.label && `${m.label} · `}
                    {m?.fps ? timecode(current.start, m.fps, false) : current.start}
                  </span>
                  <button
                    onClick={() =>
                      void navigate({
                        to: "/p/$pid/timeline/$mid",
                        params: { pid, mid: current.mediaId },
                        search: { f: current.start },
                      })
                    }
                    className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 font-medium text-fg-2 transition-colors hover:bg-white/[0.08] hover:text-fg-1"
                  >
                    <Play weight="fill" className="size-3" /> {t("review.watch")}
                  </button>
                </figcaption>
              </figure>
              {suggestion && <Suggestion character={suggestion} />}
            </div>

            <div className="mt-[min(40px,4vh)] flex flex-wrap items-center justify-center gap-3">
              {suggestion && (
                <>
                  <Button
                    size="lg"
                    variant="primary"
                    disabled={busy}
                    onClick={() =>
                      void answer(current.id, () =>
                        castEdit(pid, { op: "assign", tracks: [current.id], characterId: suggestion.id }),
                      )
                    }
                  >
                    <Check weight="bold" /> {t("review.yes", { name: characterLabel(suggestion) })}
                    <Kbd keys={keys("review.yes")} className="ml-1" />
                  </Button>
                  <Button
                    size="lg"
                    variant="secondary"
                    disabled={busy}
                    onClick={() =>
                      void answer(current.id, () =>
                        castEdit(pid, { op: "not_them", tracks: [current.id], characterId: suggestion.id }),
                      )
                    }
                  >
                    {t("review.no")} <Kbd keys={keys("review.no")} className="ml-1" />
                  </Button>
                </>
              )}
              <Button
                size="lg"
                variant="ghost"
                disabled={busy}
                onClick={() => void answer(current.id, () => castEdit(pid, { op: "not_a_face", tracks: [current.id] }))}
              >
                {t("faces.notAFace")} <Kbd keys={keys("review.notFace")} className="ml-1" />
              </Button>
              <Button size="lg" variant="ghost" onClick={() => setSkipped((s) => new Set(s).add(current.id))}>
                {t("review.skip")} <Kbd keys={keys("review.skip")} className="ml-1" />
              </Button>
            </div>
          </>
        )}

        {current && picks.some(Boolean) && (
          <div className="mt-14">
            <h3 className="mb-4 text-center text-md font-semibold text-fg-2">{t("review.orSomeoneElse")}</h3>
            <div className="flex flex-wrap justify-center gap-2.5">
              {picks.map(
                (p, i) =>
                  p && (
                    <button
                      key={p.id}
                      disabled={busy}
                      onClick={() =>
                        void answer(current.id, () => castEdit(pid, { op: "assign", tracks: [current.id], characterId: p.id }))
                      }
                      className={cn(
                        "flex items-center gap-2.5 rounded-full border border-white/10 bg-white/[0.05] py-1 pr-2 pl-1 text-left backdrop-blur-xl transition-colors hover:border-white/25 hover:bg-white/[0.1]",
                        p.id === suggestion?.id && "border-accent-line",
                      )}
                    >
                      <Avatar path={p.coverSmall} />
                      <span className={cn("max-w-[140px] truncate text-sm font-medium", p.name ? "text-fg-1" : "text-fg-2")}>
                        {characterLabel(p)}
                      </span>
                      <Kbd keys={[String(i + 1)]} />
                    </button>
                  ),
              )}
            </div>
          </div>
        )}
      </div>
    </Page>
  )
}

/** Who the analysis thinks it is, as big as the face being decided. */
function Suggestion({ character: c }: { character: Character }) {
  return (
    <>
      <span className="display text-4xl text-fg-3" aria-hidden>
        ?
      </span>
      <figure className="flex flex-col items-center">
        <div className="size-[min(300px,32vh)] overflow-hidden rounded-[32px] bg-inset shadow-[0_40px_80px_-30px_rgb(0_0_0/0.9),inset_0_0_0_1px_rgb(255_255_255/0.12)]">
          {c.cover && <img src={assetUrl(c.cover)} alt="" className="size-full object-cover object-[50%_30%]" />}
        </div>
        <figcaption className={cn("display mt-3 text-2xl", c.name ? "text-fg-1" : "text-fg-2 italic")}>{characterLabel(c)}</figcaption>
      </figure>
    </>
  )
}
