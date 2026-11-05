import { useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import type { Cast, ProjectId } from "@/bindings"
import { ArrowsMerge, Check, CircleNotch, Users } from "@/components/icons"
import { Avatar } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient, useAniListNames, useProject, useSettings } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { AniListDialog } from "./AniListDialog"
import { characterLabel, namesCredit, namesSource } from "./cast"

/**
 * Names for an anime project's cast from AniList, for an animation
 * project's from TVmaze. Once the characters are found the app looks them
 * up by itself (the series by the project's title, then its cast's
 * portraits matched to the characters); this shows that it is looking, then
 * the names it found, given with one click. Shown while the names setting
 * is on.
 */
export function AniListNames({ pid, cast, className }: { pid: ProjectId; cast: Cast | undefined; className?: string }) {
  const { t } = useTranslation("cast")
  const { data: settings } = useSettings()
  const { data: project } = useProject(pid)
  const source = namesSource(project?.content, project?.kind)
  const on = !!settings?.anilist && source !== null
  const { data } = useAniListNames(pid, on && !!cast)
  const [busy, setBusy] = useState(false)
  const [searching, setSearching] = useState(false)
  // Suggestions the person took out (a character id; for a merge, its
  // `from`), for the set of suggestions they were taken out of.
  const [left, setLeft] = useState<{ of: string; ids: ReadonlySet<string> }>({ of: "", ids: new Set() })
  if (!on || !data || !cast) return null

  const { names, merges } = data
  const shown = names.length > 0 || merges.length > 0
  const set = [data.seriesId, ...names.map((n) => `${n.characterId}:${n.anilistId}`), ...merges.map((m) => m.from)].join()
  const out = left.of === set ? left.ids : new Set<string>()
  const kept = names.filter((n) => !out.has(n.characterId))
  const keptMerges = merges.filter((m) => !out.has(m.from))
  const toggle = (id: string) => {
    const ids = new Set(out)
    if (!ids.delete(id)) ids.add(id)
    setLeft({ of: set, ids })
  }
  const character = (id: string) => cast.characters.find((c) => c.id === id)
  const refresh = () => void queryClient.invalidateQueries({ queryKey: qk.cast(pid) })
  const act = (failed: string, work: () => Promise<void>) => {
    setBusy(true)
    void work()
      .catch((e) => {
        toast.error(failed, { description: errorMessage(e) })
        refresh()
      })
      .finally(() => setBusy(false))
  }
  const give = () =>
    act(t("names.giveFailed"), async () => {
      // What was shown and kept, and what was left out: a look-up that ran
      // since gives nothing unseen.
      const leftNames = names.filter((n) => out.has(n.characterId))
      const leftMerges = merges.filter((m) => out.has(m.from))
      await commands.anilistAccept(pid, kept, keptMerges, leftNames, leftMerges)
      toast.success(kept.length > 0 ? t("names.named", { count: kept.length, source }) : t("names.merged"))
      refresh()
    })
  const dismiss = () =>
    act(t("names.dismissFailed"), async () => {
      await commands.anilistDismiss(pid)
      refresh()
    })
  // Not this series: it is turned down (its names go with it), and the
  // search by hand opens for the right one.
  const wrongSeries = () =>
    act(t("names.rejectFailed"), async () => {
      if (data.seriesId != null) await commands.anilistReject(pid, data.seriesId)
      refresh()
      setSearching(true)
    })

  return (
    <>
      {shown ? (
        <div role="status" className={cn("rounded-2xl border border-accent-line/50 bg-accent-subtle px-5 py-4", className)}>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-3">
            <Users weight="fill" className="size-5 shrink-0 text-accent" />
            <div className="min-w-0 flex-1">
              <p className="text-md font-medium text-fg-1">
                {names.length > 0 ? t("names.hasNames", { source, count: names.length }) : t("names.seesOne", { source })}
              </p>
              <p className="text-sm text-fg-2">
                {data.series != null ? t("names.fromCast", { series: data.series }) : t("names.fromCastUnknown")}{" "}
                {namesCredit(source)}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="ghost" disabled={busy} onClick={wrongSeries}>
                {source === "Fandom" ? t("names.wrongFilm") : t("names.wrongSeries")}
              </Button>
              <Button variant="ghost" disabled={busy} onClick={dismiss}>
                {t("names.notNow")}
              </Button>
              <Button
                variant="primary"
                busy={busy}
                disabled={kept.length === 0 && keptMerges.length === 0}
                onClick={give}
              >
                <Check weight="bold" />
                {names.length > 0 ? (kept.length === names.length ? t("names.giveNames") : t("names.giveN", { n: kept.length })) : t("pick.merge")}
              </Button>
            </div>
          </div>
          <ul className="mt-4 flex flex-wrap gap-2">
            {names.map((n) => {
              const c = character(n.characterId)
              const leftOut = out.has(n.characterId)
              return (
                <li key={n.characterId}>
                  <button
                    type="button"
                    aria-pressed={!leftOut}
                    onClick={() => toggle(n.characterId)}
                    className={cn(
                      "flex items-center gap-2.5 rounded-full bg-white/[0.06] py-1 pr-3.5 pl-1 transition-[opacity,background-color] hover:bg-white/[0.1]",
                      leftOut && "opacity-45",
                    )}
                  >
                    <Avatar path={c?.coverSmall} className="size-7 bg-raised" />
                    <span className={cn("text-sm text-fg-1", leftOut && "line-through")}>{n.name}</span>
                    <span className="text-sm text-fg-3">{c ? characterLabel(c) : t("names.aCharacter")}</span>
                  </button>
                </li>
              )
            })}
            {merges.map((m) => {
              const leftOut = out.has(m.from)
              const from = character(m.from)
              const into = character(m.into)
              return (
                <li key={m.from}>
                  <button
                    type="button"
                    aria-pressed={!leftOut}
                    onClick={() => toggle(m.from)}
                    className={cn(
                      "flex items-center gap-2 rounded-full bg-white/[0.06] px-3.5 py-1.5 text-sm text-fg-2 transition-[opacity,background-color] hover:bg-white/[0.1]",
                      leftOut && "line-through opacity-45",
                    )}
                  >
                    <ArrowsMerge className="size-4 text-fg-3" />{" "}
                    {t("names.same", {
                      from: from ? characterLabel(from) : t("names.aCharacterStart"),
                      into: into ? characterLabel(into) : t("names.another"),
                    })}
                  </button>
                </li>
              )
            })}
          </ul>
        </div>
      ) : (
        data.looking && (
          <p role="status" className={cn("flex items-center gap-2.5 text-sm text-fg-2", className)}>
            <CircleNotch className="size-4 animate-spin" /> {t("names.looking", { source })}
          </p>
        )
      )}
      {/* The search by hand outlives the banner: a series confirmed there
          makes the names here go. */}
      <AniListDialog pid={pid} characters={cast.characters} open={searching} onOpenChange={setSearching} />
    </>
  )
}
