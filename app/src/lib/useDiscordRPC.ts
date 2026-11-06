import { useEffect } from "react"
import { useParams, useRouterState } from "@tanstack/react-router"
import type { DiscordDoing } from "@/bindings"
import { t } from "@/lib/i18n"
import { commands } from "@/lib/ipc"
import { mediaTitle } from "@/lib/media"
import { useCast, useMediaList, useProject, useSettings } from "@/lib/queries"
import { useActivity } from "@/state/activity"

type Jobs = ReturnType<typeof useActivity.getState>

/** The export running now. */
function runningExport(s: Jobs) {
  return s.order.map((id) => s.jobs[id]).find((j) => j?.kind === "export" && j.state === "running")
}

/**
 * Tells the app what is on screen, for the person's status on Discord
 * (`src-tauri/src/discord.rs` decides what of it goes out). While that is off
 * this reads and says nothing, and a name is told only with its own switch on.
 */
export function useDiscordRPC() {
  const { data: settings, dataUpdatedAt: saved } = useSettings()
  const show = settings?.discordRpc
  const on = show?.enabled === true
  const route = useParams({ strict: false })
  const packing = useRouterState({ select: (s) => s.location.pathname.endsWith("/pack") })

  // One value each, not the job: a tick that changes none of them renders nothing.
  const exporting = useActivity((s) => on && runningExport(s) !== undefined)
  const exported = useActivity((s) => (on ? runningExport(s)?.projectId : undefined))
  const progress = useActivity((s) => (on ? runningExport(s)?.progress : undefined))

  const pid = exported ?? route.pid
  const watching = !exporting && !!route.mid
  const wantsProject = on && show?.showProject === true && !!pid
  // The player needs the project with its title's switch off too, for its kind.
  const { data: opened } = useProject(pid ?? "", wantsProject || (on && watching))

  let doing: DiscordDoing = "idle"
  if (exporting) doing = "export"
  else if (route.cid && packing) doing = "character"
  // Until the kind is known, "in a project" is true for both.
  else if (watching && opened) doing = opened.kind === "movie" ? "file" : "episode"
  else if (route.pid) doing = "project"

  // A query that stops keeps its data, so each name below asks these again.
  const wantsEpisode = on && show?.showEpisode === true && (doing === "episode" || doing === "file")
  const wantsCharacter = on && show?.showCharacter === true && doing === "character"
  const { data: media } = useMediaList(pid ?? "", wantsEpisode)
  const { data: cast } = useCast(pid ?? "", wantsCharacter)

  const project = (wantsProject && opened?.title) || null
  const shown = wantsEpisode ? media?.find((m) => m.id === route.mid) : undefined
  const episode = shown ? mediaTitle(shown) : null
  const seen = wantsCharacter ? cast?.characters.find((c) => c.id === route.cid) : undefined
  // Discord's lines are English, whatever AutoScene speaks (docs/TRANSLATING.md), and so is the name beside them.
  const character = seen ? (seen.name ?? t("cast:unnamed", { n: seen.number, lng: "en" })) : null
  const percent = exporting && progress != null ? Math.round(progress * 100) : null

  // Again after each save: one that ended the connection on the way forgot
  // what this said, and the switch had moved before it.
  useEffect(() => {
    if (!on) return
    // A status that didn't get through isn't worth telling anyone about.
    commands.discordView({ doing, project, episode, character, percent }).catch(() => {})
  }, [on, saved, doing, project, episode, character, percent])
}
