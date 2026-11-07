import { useMemo, useState } from "react"
import { Link, useParams } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { Check, Checks, Scan, X } from "@/components/icons"
import type { Character, RescanFace, RescanPair } from "@/bindings"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ConfirmDialog } from "@/components/ui/confirm-dialog"
import { EmptyState } from "@/components/ui/empty-state"
import { Page } from "@/components/ui/page"
import { Skeleton } from "@/components/ui/skeleton"
import { TopBar } from "@/components/ui/topbar"
import { ProjectLoadError } from "@/features/project/ProjectLoadError"
import { assetUrl } from "@/lib/media"
import { firstFailure, useCast, useRescanResults } from "@/lib/queries"
import { characterLabel } from "./cast"
import { ChoiceFace } from "./ChoiceFace"
import { FaceLane } from "./FaceLane"
import { LikenessCard } from "./LikenessCard"
import { RescanBar } from "./RescanBar"
import { answerRescan, clearMatches, rescanParts } from "./rescan"

const pairKey = (p: RescanPair) => `${p.a}:${p.b}`

/**
 * What Rescan found: suggestions, with the faces themselves as the argument.
 * Nothing here has happened yet; every answer is the person's.
 */
export function RescanPage() {
  const { t } = useTranslation("cast")
  const { pid } = useParams({ from: "/p/$pid/cast/rescan" })
  const results = useRescanResults(pid)
  const castQuery = useCast(pid)
  // Answered things leave at once, before the list is read again.
  const [gone, setGone] = useState<Set<number>>(new Set())
  const [goneNames, setGoneNames] = useState<Set<string>>(new Set())
  const [accepting, setAccepting] = useState(false)
  const [merging, setMerging] = useState<RescanPair | null>(null)
  const [leaving, setLeaving] = useState(false)

  const byId = useMemo(() => new Map((castQuery.data?.characters ?? []).map((c) => [c.id, c])), [castQuery.data])
  const faces = useMemo(() => (results.data?.faces ?? []).filter((f) => !gone.has(f.face.id)), [results.data, gone])
  const pairs = (results.data?.pairs ?? []).filter((p) => !goneNames.has(pairKey(p)) && byId.has(p.a) && byId.has(p.b))
  const parts = useMemo(() => rescanParts(faces), [faces])
  const matches = useMemo(() => clearMatches(faces), [faces])
  const clearCount = [...matches.values()].reduce((n, l) => n + l.length, 0)
  const tally = results.data?.tally
  const lit = faces[0]?.face.thumb

  const toolbar = <TopBar back={{ label: t("title"), to: `/p/${pid}/cast` }} title={t("rescan.title")} />
  const failure = firstFailure(results, castQuery)
  if (failure) {
    return (
      <Page topbar={toolbar}>
        <ProjectLoadError error={failure.error} onRetry={failure.retry} />
      </Page>
    )
  }

  const leave = (ids: number[]) => setGone((s) => new Set([...s, ...ids]))
  const back = (ids: number[]) =>
    setGone((s) => {
      const next = new Set(s)
      for (const id of ids) next.delete(id)
      return next
    })
  /** Answer about `list`: they leave now, and come back if it failed. */
  const answer = async (list: RescanFace[], run: (ids: number[]) => Promise<true | undefined>) => {
    const ids = list.map((f) => f.face.id)
    leave(ids)
    if ((await run(ids)) === undefined) back(ids)
  }
  const accept = (list: RescanFace[], who: Character) =>
    answer(list, (tracks) => answerRescan(pid, { op: "accept", tracks, characterId: who.id }))
  const dismiss = (list: RescanFace[]) => answer(list, (tracks) => answerRescan(pid, { op: "dismiss", tracks }))
  /** Not this person: said out loud for a face on no one, left alone for one on someone. */
  const refuse = (list: RescanFace[], who: Character) =>
    answer(list, (tracks) =>
      list.every((f) => f.verdict !== "elsewhere")
        ? answerRescan(pid, { op: "not_them", tracks, characterId: who.id })
        : answerRescan(pid, { op: "dismiss", tracks }),
    )
  const notThem = (list: RescanFace[], who: Character) =>
    answer(list, (tracks) => answerRescan(pid, { op: "not_them", tracks, characterId: who.id }))
  const notFaces = (list: RescanFace[]) => answer(list, (tracks) => answerRescan(pid, { op: "not_a_face", tracks }))

  const settlePair = async (p: RescanPair, run: () => Promise<true | undefined>) => {
    setGoneNames((s) => new Set(s).add(pairKey(p)))
    if ((await run()) === undefined) {
      setGoneNames((s) => {
        const next = new Set(s)
        next.delete(pairKey(p))
        return next
      })
    }
  }
  /** The one with more faces keeps going under their name. */
  const mergeOrder = (p: RescanPair): [Character, Character] => {
    const [a, b] = [byId.get(p.a)!, byId.get(p.b)!]
    return a.tracks >= b.tracks ? [b, a] : [a, b]
  }

  const left = faces.length + pairs.length
  const loading = results.isPending || castQuery.isPending

  return (
    <Page topbar={toolbar}>
      {/* The first face waiting lights the room. */}
      {lit && (
        <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-screen overflow-hidden opacity-30">
          <img src={assetUrl(lit)} alt="" className="size-full scale-125 animate-fade-in object-cover blur-[90px] saturate-150" />
          <div className="absolute inset-0 bg-gradient-to-b from-canvas/40 via-canvas/70 to-canvas" />
        </div>
      )}
      <div className="relative mx-auto flex max-w-[1180px] flex-col gap-10 px-10 pt-8 pb-32">
        {loading ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(116px,1fr))] gap-3">
            {Array.from({ length: 16 }, (_, i) => (
              <Skeleton key={i} className="aspect-[3/4] rounded-xl" />
            ))}
          </div>
        ) : left === 0 ? (
          <EmptyState
            icon={<Checks />}
            title={t("rescan.done.title")}
            description={t("rescan.done.about")}
            actions={
              <Button asChild>
                <Link to="/p/$pid/cast" params={{ pid }}>
                  {t("character.backToCast")}
                </Link>
              </Button>
            }
            className="mt-24"
          />
        ) : (
          <>
            <header className="flex flex-wrap items-end justify-between gap-6">
              <div>
                <h1 className="display text-5xl text-fg-1">{t("rescan.heading")}</h1>
                <p className="mt-2 text-md text-fg-2">{t("rescan.nothingYet")}</p>
                {tally && (
                  <div className="mt-4 flex flex-wrap gap-2">
                    <Badge tone="neutral">{t("rescan.tally.looked", { count: tally.looked })}</Badge>
                    <Badge tone="neutral">{t("rescan.tally.read", { count: tally.read })}</Badge>
                    {tally.right > 0 && <Badge tone="neutral">{t("rescan.tally.right", { count: tally.right })}</Badge>}
                    {tally.unread > 0 && <Badge tone="neutral">{t("rescan.tally.unread", { count: tally.unread })}</Badge>}
                  </div>
                )}
              </div>
              {clearCount > 0 && (
                <Button variant="primary" size="lg" onClick={() => setAccepting(true)}>
                  <Check weight="bold" /> {t("rescan.acceptClear", { count: clearCount })}
                </Button>
              )}
            </header>

            {pairs.length > 0 && (
              <div className="flex flex-col gap-5">
                <h2 className="text-xl font-semibold text-fg-1">{t("rescan.section.pairs")}</h2>
                {pairs.map((p) => (
                  <LikenessCard
                    key={pairKey(p)}
                    pair={p}
                    a={byId.get(p.a)!}
                    b={byId.get(p.b)!}
                    onSame={() => setMerging(p)}
                    onApart={() => void settlePair(p, () => answerRescan(pid, { op: "two_people", a: p.a, b: p.b }))}
                  />
                ))}
              </div>
            )}

            {parts.lanes.length > 0 && (
              <div className="flex flex-col gap-5">
                <h2 className="text-xl font-semibold text-fg-1">{t("rescan.section.faces")}</h2>
                {parts.lanes.flatMap(({ target, faces: list }) => {
                  const who = byId.get(target)
                  if (!who) return []
                  const name = characterLabel(who)
                  return (
                    <FaceLane
                      key={target}
                      pid={pid}
                      who={who}
                      heading={name}
                      about={t("rescan.lane.lookLike", { count: list.length, name })}
                      faces={list}
                      from={(f) => (f.face.characterId && f.face.assign !== "unsure" ? byId.get(f.face.characterId) : undefined)}
                      yes={t("maybe.yes", { name })}
                      no={t("character.not", { name })}
                      onYes={(f) => void accept([f], who)}
                      onNo={(f) => void refuse([f], who)}
                      actions={
                        <>
                          <Button variant="ghost" onClick={() => void dismiss(list)}>
                            {t("rescan.lane.leave")}
                          </Button>
                          <Button variant="accent" onClick={() => void accept(list, who)}>
                            <Check weight="bold" /> {t("maybe.allAre", { name })}
                          </Button>
                        </>
                      }
                    />
                  )
                })}
              </div>
            )}

            {parts.between.length > 0 && (
              <div className="flex flex-col gap-5">
                <h2 className="text-xl font-semibold text-fg-1">{t("rescan.section.between")}</h2>
                <div className="grid gap-4 lg:grid-cols-2">
                  {parts.between.flatMap((f) => {
                    const [a, b] = [f.target && byId.get(f.target), f.other && byId.get(f.other)]
                    if (!a || !b) return []
                    return (
                      <ChoiceFace
                        key={f.face.id}
                        pid={pid}
                        face={f}
                        a={a}
                        b={b}
                        onPick={(who) => void accept([f], who)}
                        onNeither={() => void dismiss([f])}
                      />
                    )
                  })}
                </div>
              </div>
            )}

            {parts.doubts.length > 0 && (
              <div className="flex flex-col gap-5">
                <h2 className="text-xl font-semibold text-fg-1">{t("rescan.section.doubts")}</h2>
                {parts.doubts.flatMap(({ target, faces: list }) => {
                  const who = byId.get(target)
                  if (!who) return []
                  const name = characterLabel(who)
                  return (
                    <FaceLane
                      key={target}
                      pid={pid}
                      who={who}
                      heading={name}
                      about={t("rescan.lane.doubt", { count: list.length, name })}
                      faces={list}
                      yes={t("character.not", { name })}
                      no={t("rescan.lane.keep")}
                      onYes={(f) => void notThem([f], who)}
                      onNo={(f) => void dismiss([f])}
                      actions={
                        <Button variant="ghost" onClick={() => void dismiss(list)}>
                          {t("rescan.lane.leave")}
                        </Button>
                      }
                    />
                  )
                })}
              </div>
            )}

            {parts.noFace.length > 0 && (
              <FaceLane
                pid={pid}
                heading={t("rescan.section.noFace")}
                about={t("rescan.lane.noFace", { count: parts.noFace.length })}
                faces={parts.noFace}
                yes={t("faces.notAFace")}
                no={t("rescan.lane.keep")}
                onYes={(f) => void notFaces([f])}
                onNo={(f) => void dismiss([f])}
                actions={
                  <>
                    <Button variant="ghost" onClick={() => void dismiss(parts.noFace)}>
                      {t("rescan.lane.leave")}
                    </Button>
                    <Button variant="secondary" onClick={() => void notFaces(parts.noFace)}>
                      <X /> {t("rescan.lane.noneAreFaces")}
                    </Button>
                  </>
                }
              />
            )}
          </>
        )}
      </div>

      {left > 0 && (
        <div className="glass-strong fixed bottom-6 left-1/2 z-40 flex w-max max-w-[calc(100vw-3rem)] -translate-x-1/2 animate-pop-in items-center gap-1 rounded-full py-1.5 pr-1.5 pl-5">
          <Scan className="mr-2 size-4 text-accent" />
          <span className="pr-2 text-md font-semibold whitespace-nowrap text-fg-1">
            {t("rescan.left", { count: left })}
          </span>
          <Button size="sm" variant="ghost" onClick={() => setLeaving(true)}>
            {t("rescan.leaveRest")}
          </Button>
        </div>
      )}
      <RescanBar pid={pid} lifted={left > 0} />

      <ConfirmDialog
        open={accepting}
        onOpenChange={setAccepting}
        title={t("rescan.acceptTitle", { count: clearCount })}
        description={t("rescan.acceptAbout")}
        answer={t("rescan.acceptAnswer")}
        icon={<Check weight="bold" />}
        failure={t("editFailed")}
        onConfirm={async () => {
          for (const [target, tracks] of matches) {
            leave(tracks)
            await answerRescan(pid, { op: "accept", tracks, characterId: target })
          }
        }}
      />
      <ConfirmDialog
        open={merging !== null}
        onOpenChange={(open) => !open && setMerging(null)}
        title={
          merging &&
          t("pick.mergeQuestion", {
            from: characterLabel(mergeOrder(merging)[0]),
            into: characterLabel(mergeOrder(merging)[1]),
          })
        }
        description={t("pick.mergeDetail")}
        answer={t("pick.merge")}
        failure={t("editFailed")}
        onConfirm={async () => {
          if (!merging) return
          const [from, into] = mergeOrder(merging)
          await settlePair(merging, () => answerRescan(pid, { op: "same_person", from: from.id, into: into.id }))
        }}
      />
      <ConfirmDialog
        open={leaving}
        onOpenChange={setLeaving}
        title={t("rescan.leaveTitle")}
        description={t("rescan.leaveAbout")}
        answer={t("rescan.leaveRest")}
        failure={t("editFailed")}
        onConfirm={() => answerRescan(pid, { op: "leave_all" })}
      />
    </Page>
  )
}
