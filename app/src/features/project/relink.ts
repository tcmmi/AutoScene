import { toast } from "sonner"
import type { MediaItem, Relink, RelinkSummary } from "@/bindings"
import { commands, errorMessage } from "@/lib/ipc"
import { t } from "@/lib/i18n"
import { mediaTitle, pickFolder, pickVideoFile } from "@/lib/media"
import { useRelink } from "@/state/relink"

/**
 * Show AutoScene where a missing episode is now. The same file keeps
 * everything; another copy is indexed again; another video is asked about
 * first (`RelinkDialog`).
 */
export async function locateFile(pid: string, m: MediaItem) {
  const path = await pickVideoFile(t("project:relink.whereIs", { title: mediaTitle(m) }))
  if (!path) return
  try {
    const r = await commands.mediaRelink(pid, m.id, path, false)
    if (r.kind === "different") useRelink.getState().ask({ pid, media: m, path, reason: r.reason })
    else relinked(r, mediaTitle(m))
  } catch (e) {
    toast.error(t("project:relink.couldntUse"), { description: errorMessage(e) })
  }
}

/** The file asked about, taken anyway: analysed again, decisions kept. */
export async function relinkAnyway(pid: string, m: MediaItem, path: string) {
  try {
    await commands.mediaRelink(pid, m.id, path, true)
    toast(t("project:relink.using", { title: mediaTitle(m) }), { description: t("project:relink.usingDescription") })
  } catch (e) {
    toast.error(t("project:relink.couldntUse"), { description: errorMessage(e) })
  }
}

function relinked(r: Relink, title: string) {
  if (r.kind === "same") toast(t("project:relink.found", { title }))
  else toast(t("project:relink.foundAgain", { title }), { description: t("project:relink.foundAgainDescription") })
}

/**
 * Look for every missing episode in one folder (and the folders inside it).
 * Resolves once done; `false` when nothing was looked for.
 */
export async function locateFolder(pid: string, missing: number, movie = false): Promise<boolean> {
  const dir = await pickFolder(movie ? t("project:relink.whereAreFiles") : t("project:relink.whereAreEpisodes"))
  if (!dir) return false
  try {
    const s = await commands.mediaRelinkFolder(pid, dir)
    const { title, description } = folderResult(s, missing, movie)
    if (s.same + s.reindex === 0) toast.error(title, { description })
    else toast(title, { description })
  } catch (e) {
    toast.error(t("project:relink.couldntLook"), { description: errorMessage(e) })
  }
  return true
}

export function folderResult(s: RelinkSummary, missing: number, movie = false): { title: string; description?: string } {
  // The keys are `relink.folder.{episodes,files}.*`: both kinds have the same set (language.test.tsx), and a key scan only sees them here.
  const words = movie ? "files" : "episodes"
  const found = s.same + s.reindex
  const checking = s.reindex > 0 ? t("project:relink.folder.copies", { count: s.reindex }) : undefined
  if (found === 0) {
    return { title: t(`project:relink.folder.${words}.noneTitle`), description: t(`project:relink.folder.${words}.noneDescription`) }
  }
  if (s.missing === 0) return { title: t(`project:relink.folder.${words}.foundAll`, { count: found }), description: checking }
  const still = t(`project:relink.folder.${words}.stillMissing`, { count: s.missing })
  return {
    title: t(`project:relink.folder.${words}.foundSome`, { found, count: missing }),
    description: checking ? `${checking} ${still}` : still,
  }
}
