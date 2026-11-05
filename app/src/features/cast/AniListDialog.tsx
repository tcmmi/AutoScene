import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import type { AniListMedia, AniListMerge, AniListSuggestion, Character, ProjectId } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient, useProject } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { characterLabel, namesCredit, namesSource } from "./cast"

type Stage = "search" | "chain" | "suggestions"

/** Fetch names from AniList (anime) or TVmaze (animation): the person
 *  confirms the series, then the series' cast portraits are matched to the
 *  project's characters and the names are suggested (or given at once when
 *  the project's auto flag is set). Opened from the Cast page and the names
 *  banner while the names setting is on. */
export function AniListDialog({
  pid,
  characters,
  open,
  onOpenChange,
}: {
  pid: ProjectId
  characters: Character[]
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation(["cast", "common"])
  const [stage, setStage] = useState<Stage>("search")
  const [text, setText] = useState("")
  const [busy, setBusy] = useState(false)
  const [results, setResults] = useState<AniListMedia[]>([])
  // The text the last search was for (`null`: none yet).
  const [searched, setSearched] = useState<string | null>(null)
  const [chain, setChain] = useState<AniListMedia[]>([])
  const [rev, setRev] = useState(0)
  const [names, setNames] = useState<AniListSuggestion[]>([])
  const [merges, setMerges] = useState<AniListMerge[]>([])
  const [auto, setAuto] = useState(false)
  // Suggestions the person took out (`n:` a named character, `m:` a merge's `from`).
  const [out, setOut] = useState<ReadonlySet<string>>(new Set())
  const { data: project } = useProject(pid)
  const source = namesSource(project?.content, project?.kind) ?? "AniList"
  const film = source === "Fandom"
  // Each opening starts at the search: suggestions shown before may be from
  // a series turned down since (the dialog stays mounted between openings).
  const [wasOpen, setWasOpen] = useState(open)
  if (open !== wasOpen) {
    setWasOpen(open)
    if (open) {
      setStage("search")
      setResults([])
      setSearched(null)
      setChain([])
      setNames([])
      setMerges([])
    }
  }
  const nameOf = (id: string) => {
    const found = characters.find((x) => x.id === id)
    return found ? characterLabel(found) : t("names.aCharacter")
  }

  // The suggested search text is filled once the dialog opens (unless the
  // person typed meanwhile); nothing is sent before the Search click.
  useEffect(() => {
    if (open && text === "") {
      commands
        .anilistSearchText(pid)
        .then((suggested) => setText((typed) => typed || suggested))
        .catch(() => {})
    }
  }, [open, pid, text])

  const run = (work: () => Promise<void>) => {
    setBusy(true)
    void work()
      .catch((e) => toast.error(t("names.couldntAnswer", { source }), { description: errorMessage(e) }))
      .finally(() => setBusy(false))
  }

  const search = () =>
    run(async () => {
      setResults(await commands.anilistSearch(pid, text))
      setSearched(text.trim())
    })

  const confirm = (m: AniListMedia) =>
    run(async () => {
      setChain(await commands.anilistConfirm(pid, m.id))
      setStage("chain")
      // Names found in another series' cast no longer wait.
      void queryClient.invalidateQueries({ queryKey: qk.anilistNames(pid), exact: true })
    })

  const suggest = () =>
    run(async () => {
      const s = await commands.anilistSuggest(pid)
      setRev(s.rev)
      setNames(s.names)
      setMerges(s.merges)
      setOut(new Set())
      setAuto(s.auto)
      setStage("suggestions")
      if (s.auto) {
        toast.success(t("names.named", { count: s.names.length, source }))
        void queryClient.invalidateQueries({ queryKey: qk.cast(pid) })
      }
    })

  const keptNames = names.filter((n) => !out.has(`n:${n.characterId}`))
  const keptMerges = merges.filter((m) => !out.has(`m:${m.from}`))
  const toggle = (key: string) => {
    const next = new Set(out)
    if (!next.delete(key)) next.add(key)
    setOut(next)
  }
  const apply = () =>
    run(async () => {
      await commands.anilistApply(pid, rev, keptNames, keptMerges)
      toast.success(t("names.applied"))
      void queryClient.invalidateQueries({ queryKey: qk.cast(pid) })
      onOpenChange(false)
    })

  const title = {
    search: t("names.fromSource", { source }),
    chain: film ? t("names.thisFilm") : t("names.thisSeries"),
    suggestions: t("names.suggestions"),
  }[stage]
  // Fandom's wikis are found through Wikidata: both see the search. The credit the licence asks for ends it.
  const credit = namesCredit(source) ?? ""
  const description = (film ? t("names.descriptionFilm", { credit }) : t("names.descriptionSeries", { source, credit })).trim()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        title={title}
        description={description}
      >
        {stage === "search" && (
          <>
            <div className="flex gap-2">
              <Input value={text} onChange={(e) => setText(e.target.value)} placeholder={film ? t("names.searchFilm") : t("names.searchSeries")} aria-label={t("names.searchLabel")} />
              <Button busy={busy} disabled={!text.trim()} onClick={search}>
                {t("names.search")}
              </Button>
            </div>
            <ul className="mt-4 max-h-72 space-y-1 overflow-y-auto">
              {results.map((m) => (
                <li key={m.id}>
                  <button
                    type="button"
                    className="w-full rounded-lg px-3 py-2 text-left transition-colors hover:bg-white/[0.06]"
                    disabled={busy}
                    onClick={() => confirm(m)}
                  >
                    <span className="text-md text-fg-1">{m.title}</span>
                    <span className="ml-2 text-sm text-fg-3">
                      {[m.format, m.year, m.episodes != null ? t("names.episodesShort", { n: m.episodes }) : null].filter(Boolean).join(" · ")}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
            {searched !== null && results.length === 0 && !busy && (
              <p className="mt-1 text-sm text-fg-2">
                {film ? t("names.nothingFilm", { query: searched }) : t("names.nothingSeries", { source, query: searched })}
              </p>
            )}
          </>
        )}
        {stage === "chain" && (
          <>
            <p className="text-md text-fg-2">
              {chain.length > 1 ? t("names.chain", { count: chain.length }) : t("names.chainOne")}
            </p>
            <ul className="mt-3 space-y-1">
              {chain.map((m) => (
                <li key={m.id} className="rounded-lg bg-white/[0.04] px-3 py-2 text-md text-fg-1">
                  {m.title}
                </li>
              ))}
            </ul>
            <DialogFooter className="mt-5">
              <Button variant="ghost" disabled={busy} onClick={() => setStage("search")}>
                {t("pick.back")}
              </Button>
              <Button busy={busy} onClick={suggest}>
                {t("names.matchCast")}
              </Button>
            </DialogFooter>
          </>
        )}
        {stage === "suggestions" && (
          <>
            {names.length === 0 && merges.length === 0 ? (
              <p className="text-md text-fg-2">{t("names.noMatch")}</p>
            ) : (
              <>
                {!auto && <p className="mb-3 text-sm text-fg-2">{t("names.clickWrong")}</p>}
                <ul className="max-h-72 space-y-1 overflow-y-auto">
                  {names.map((n) => {
                    const leftOut = out.has(`n:${n.characterId}`)
                    return (
                      <li key={n.characterId}>
                        <button
                          type="button"
                          aria-pressed={!leftOut}
                          disabled={auto || busy}
                          onClick={() => toggle(`n:${n.characterId}`)}
                          className={cn(
                            "flex w-full items-center justify-between rounded-lg bg-white/[0.04] px-3 py-2 text-left transition-[opacity,background-color] enabled:hover:bg-white/[0.08]",
                            leftOut && "opacity-45",
                          )}
                        >
                          <span className={cn("text-md text-fg-1", leftOut && "line-through")}>{n.name}</span>
                          <span className="text-sm text-fg-3">
                            {nameOf(n.characterId)} · {Math.round((n.score ?? 0) * 100)}%
                          </span>
                        </button>
                      </li>
                    )
                  })}
                  {merges.map((m) => {
                    const leftOut = out.has(`m:${m.from}`)
                    return (
                      <li key={m.from}>
                        <button
                          type="button"
                          aria-pressed={!leftOut}
                          disabled={auto || busy}
                          onClick={() => toggle(`m:${m.from}`)}
                          className={cn(
                            "w-full rounded-lg bg-white/[0.04] px-3 py-2 text-left text-sm text-fg-2 transition-[opacity,background-color] enabled:hover:bg-white/[0.08]",
                            leftOut && "line-through opacity-45",
                          )}
                        >
                          {t("names.merge", { from: nameOf(m.from), into: nameOf(m.into) })}
                        </button>
                      </li>
                    )
                  })}
                </ul>
              </>
            )}
            <DialogFooter className="mt-5">
              <Button variant="ghost" onClick={() => onOpenChange(false)}>
                {auto ? t("common:action.close") : t("names.notNow")}
              </Button>
              {!auto && (names.length > 0 || merges.length > 0) && (
                <Button busy={busy} disabled={keptNames.length === 0 && keptMerges.length === 0} onClick={apply}>
                  {keptNames.length + keptMerges.length === names.length + merges.length
                    ? t("names.apply")
                    : t("names.applyN", { n: keptNames.length + keptMerges.length })}
                </Button>
              )}
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
