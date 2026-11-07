import { toast } from "sonner"
import type { RescanAnswer, RescanFace, RescanRequest } from "@/bindings"
import { t } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { useRescan } from "@/state/rescan"

/**
 * A face below this quality can't shape a character: the faces a closer look
 * can help. Clear faces in Everyone else are strangers; reading them again
 * changes nothing.
 */
export const UNCLEAR_QUALITY = 0.45

/** Start a rescan; `undefined` when it couldn't be started. */
export async function startRescan(pid: string, request: RescanRequest): Promise<true | undefined> {
  try {
    useRescan.getState().started(pid, request.characters)
    await commands.castRescan(pid, request)
    return true
  } catch (e) {
    useRescan.getState().ended(pid)
    toast.error(t("cast:rescan.startFailed"), { description: errorMessage(e) })
    return undefined
  }
}

/** Say the person's answer; a failure throws (a dialog stays open on it). */
export async function sayRescan(pid: string, answer: RescanAnswer): Promise<void> {
  try {
    await commands.rescanAnswer(pid, answer)
  } finally {
    void queryClient.invalidateQueries({ queryKey: qk.rescan(pid) })
  }
}

/** Say the person's answer; a toast says so and `undefined` comes back when it failed. */
export async function answerRescan(pid: string, answer: RescanAnswer): Promise<true | undefined> {
  try {
    await sayRescan(pid, answer)
    return true
  } catch (e) {
    toast.error(t("cast:editFailed"), { description: errorMessage(e) })
    return undefined
  }
}

/** What the results page shows, sorted into its parts. */
export type RescanParts = {
  /** Per character, the faces that look like them: clear ones first. */
  lanes: { target: string; faces: RescanFace[] }[]
  between: RescanFace[]
  /** Per character, the faces that may not be them. */
  doubts: { target: string; faces: RescanFace[] }[]
  noFace: RescanFace[]
}

const clear = (f: RescanFace) => f.verdict === "newly" || f.verdict === "elsewhere"

export function rescanParts(faces: RescanFace[]): RescanParts {
  const lanes = new Map<string, RescanFace[]>()
  const doubts = new Map<string, RescanFace[]>()
  const parts: RescanParts = { lanes: [], between: [], doubts: [], noFace: [] }
  const add = (into: Map<string, RescanFace[]>, f: RescanFace) => {
    if (f.target) into.set(f.target, [...(into.get(f.target) ?? []), f])
  }
  for (const f of faces) {
    // A guess with someone close behind is a choice between the two.
    if (f.verdict === "between" || (f.verdict === "maybe" && f.other)) parts.between.push(f)
    else if (f.verdict === "no_face") parts.noFace.push(f)
    else if (f.verdict === "doubt") add(doubts, f)
    else add(lanes, f)
  }
  parts.lanes = [...lanes]
    .map(([target, list]) => ({
      target,
      faces: list.toSorted((a, b) => Number(clear(b)) - Number(clear(a)) || (b.score ?? 0) - (a.score ?? 0)),
    }))
    .sort((a, b) => b.faces.length - a.faces.length)
  parts.doubts = [...doubts].map(([target, list]) => ({ target, faces: list }))
  return parts
}

/** Faces with a clear answer, by the character they go to. */
export function clearMatches(faces: RescanFace[]): Map<string, number[]> {
  const out = new Map<string, number[]>()
  for (const f of faces) {
    if (f.target && clear(f)) out.set(f.target, [...(out.get(f.target) ?? []), f.face.id])
  }
  return out
}
