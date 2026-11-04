import { create } from "zustand"
import type { JobId, JobInfo, JobProgress } from "@/bindings"

/**
 * Live view of background jobs. Filled from `jobs_list` once and then kept
 * current by `job:changed` / `job:progress` events (see `lib/events.ts`).
 * Progress lives here, never in React Query, so 4 Hz updates don't refetch.
 *
 * A job's `progress` and `etaMs` here are what to show, not the last sample:
 * a `null` right after a value keeps that value for `HOLD_MS` (and, when the
 * null names a new stage, the stage it belongs to as well), and while the
 * backend measures a phase's pace anew, the last time left it told is
 * counted down (`ETA_BRIDGE_MS`). So the bar, its %, the time left, the
 * Rail's ring and `aria-valuenow` agree, and a momentary gap never blinks.
 */
type ActivityState = {
  jobs: Record<JobId, JobInfo>
  /** Newest first. */
  order: JobId[]
  replaceAll: (jobs: JobInfo[]) => void
  upsert: (job: JobInfo) => void
  progress: (p: JobProgress) => void
  /** The hold that began at `since` ran out: how far the job is isn't known (UI only; `rev` stays). */
  unhold: (id: JobId, since: number) => void
  clearFinished: () => void
}

export const isFinished = (j: JobInfo) => j.state === "done" || j.state === "failed" || j.state === "cancelled"

/**
 * What a job's bar or ring shows: empty and still while it waits (waiting
 * isn't working), its progress while it runs (`null`: how far isn't known,
 * the sweep).
 */
export const jobBar = (j: Pick<JobInfo, "state" | "progress">): number | null =>
  j.state === "queued" ? 0 : j.progress

/**
 * A `null` after a value shows as unknown only once it has lasted this long:
 * every measured momentary gap (a model's download starting, the next clip
 * after a failed one) was under 1.4 s.
 */
export const HOLD_MS = 1500
/** How long the last time left is counted down while the backend measures a phase anew. */
export const ETA_BRIDGE_MS = 5000

/**
 * The running holds, by job: when each began (so a stale timer knows it is stale), its timer, and
 * the newer stage it keeps back, if the null named one (shown once the hold runs out).
 */
const holds = new Map<
  JobId,
  { since: number; timer: ReturnType<typeof setTimeout>; next?: Pick<JobInfo, "stage" | "frames"> }
>()
/** The last time left the backend told, by job, and when it told it. */
const told = new Map<JobId, { eta: number; at: number }>()
/** A stage with its numbers left out ("Clip 2 of 3" reads as "Clip 3 of 3"), as the backend's own phase test. */
const words = (stage: string | null) => (stage ?? "").replace(/\d/g, "")

function stopHold(id: JobId) {
  clearTimeout(holds.get(id)?.timer)
  holds.delete(id)
}

/** A copy that waits or has ended holds nothing and borrows no time left. */
function forgetIfIdle(j: JobInfo) {
  if (j.state === "queued" || isFinished(j)) {
    stopHold(j.id)
    told.delete(j.id)
  }
}

/** Tests: forget every hold and time left (`src/test/setup.ts` calls it after each test). */
export function resetHolds() {
  for (const id of [...holds.keys()]) stopHold(id)
  told.clear()
}

/**
 * Events and the initial snapshot can arrive in any order, so every write
 * keeps whichever copy of a job has the higher `rev` (see `JobInfo.rev`).
 */
export const useActivity = create<ActivityState>((set) => ({
  jobs: {},
  order: [],
  replaceAll: (list) =>
    set((s) => {
      const jobs: Record<JobId, JobInfo> = { ...s.jobs }
      for (const j of list)
        if (!jobs[j.id] || jobs[j.id].rev <= j.rev) {
          jobs[j.id] = j
          forgetIfIdle(j)
        }
      const order = Object.values(jobs)
        .sort((a, b) => b.createdAt - a.createdAt)
        .map((j) => j.id)
      return { jobs, order }
    }),
  upsert: (job) =>
    set((s) => {
      const prev = s.jobs[job.id]
      if (prev && prev.rev > job.rev) return s
      forgetIfIdle(job)
      return { jobs: { ...s.jobs, [job.id]: job }, order: prev ? s.order : [job.id, ...s.order] }
    }),
  progress: (p) =>
    set((s) => {
      const j = s.jobs[p.id]
      if (!j || isFinished(j) || j.rev > p.rev) return s
      const now = Date.now()
      let progress = p.progress
      let stage = p.stage
      let frames = p.frames ?? null
      if (progress != null) stopHold(p.id)
      else if (j.progress != null) {
        // Between two values, or before a new stage's first one: hold what it showed.
        // Nulls in a row don't stretch the hold.
        progress = j.progress
        let hold = holds.get(p.id)
        if (!hold) {
          const since = now
          hold = { since, timer: setTimeout(() => useActivity.getState().unhold(p.id, since), HOLD_MS) }
          holds.set(p.id, hold)
        }
        // A new stage's name beside the old stage's value would read wrong ("Downloading AI model ·
        // 100 %" before it began), so its name waits with the value: for its first value or the hold's end.
        hold.next = words(p.stage) === words(j.stage) ? undefined : { stage: p.stage, frames }
        if (hold.next) {
          stage = j.stage
          frames = j.frames ?? null
        }
      }
      let etaMs = p.etaMs
      const t = told.get(p.id)
      if (etaMs != null) told.set(p.id, { eta: etaMs, at: now })
      else if (
        t &&
        now - t.at < ETA_BRIDGE_MS &&
        words(stage) === words(j.stage) &&
        (p.progress == null || j.progress == null || p.progress >= j.progress)
      )
        etaMs = Math.max(0, t.eta - (now - t.at))
      else told.delete(p.id) // a new stage, a value that went back, or told too long ago: borrow nothing
      const next = { ...j, progress, stage, etaMs, frames, rev: p.rev }
      return { jobs: { ...s.jobs, [p.id]: next } }
    }),
  unhold: (id, since) =>
    set((s) => {
      const hold = holds.get(id)
      if (hold?.since !== since) return s // a value came, or a newer hold began: nothing to do
      holds.delete(id)
      told.delete(id)
      const j = s.jobs[id]
      return !j || isFinished(j) ? s : { jobs: { ...s.jobs, [id]: { ...j, ...hold.next, progress: null, etaMs: null } } }
    }),
  clearFinished: () =>
    set((s) => {
      const keep = s.order.filter((id) => !isFinished(s.jobs[id]))
      return { order: keep, jobs: Object.fromEntries(keep.map((id) => [id, s.jobs[id]])) }
    }),
}))

/** All jobs, newest first. */
export function useJobs(): JobInfo[] {
  const jobs = useActivity((s) => s.jobs)
  const order = useActivity((s) => s.order)
  return order.map((id) => jobs[id]).filter(Boolean)
}

/** The unfinished job working on `mediaId` (of `kind`, if given), if any. */
export function useMediaJob(mediaId: string | undefined, kind?: JobInfo["kind"]): JobInfo | undefined {
  return useActivity((s) => {
    if (!mediaId) return undefined
    for (const id of s.order) {
      const j = s.jobs[id]
      if (j.mediaId === mediaId && !isFinished(j) && (!kind || j.kind === kind)) return j
    }
    return undefined
  })
}

/**
 * The queued and running jobs `of` takes, newest first, read once: not a
 * hook, so a dialog that says what stops keeps what it said when it opened.
 */
export function unfinishedJobs(of: (j: JobInfo) => boolean): JobInfo[] {
  const s = useActivity.getState()
  return s.order.map((id) => s.jobs[id]).filter((j) => j !== undefined && !isFinished(j) && of(j))
}

/** Count of queued + running jobs (the sidebar's Activity badge). */
export function useActiveCount(): number {
  return useActivity((s) => s.order.reduce((n, id) => n + (isFinished(s.jobs[id]) ? 0 : 1), 0))
}

/** Count of running jobs: the Activity badge is gold only while one runs. */
export function useRunningCount(): number {
  return useActivity((s) => s.order.reduce((n, id) => n + (s.jobs[id].state === "running" ? 1 : 0), 0))
}

/** What the Activity button tells assistive tech: "1 running, 2 waiting". */
export function describeActive(running: number, waiting: number): string {
  const parts = [running > 0 && `${running} running`, waiting > 0 && `${waiting} waiting`].filter(Boolean)
  return parts.length > 0 ? parts.join(", ") : "Nothing running"
}

/** Do the unfinished jobs `of` takes all wait their turn (and is there one)? */
function allWait(s: ActivityState, of: (j: JobInfo) => boolean): boolean {
  let waiting = false
  for (const id of s.order) {
    const j = s.jobs[id]
    if (isFinished(j) || !of(j)) continue
    if (j.state !== "queued") return false
    waiting = true
  }
  return waiting
}

/**
 * The mean progress of the running jobs `of` takes that report one. `null`
 * while the ones that run haven't told how far they are (a ring spins), and
 * when nothing matches. While every matching job waits it is `0`: waiting
 * isn't working, so a ring stands still, empty.
 */
function meanProgress(s: ActivityState, of: (j: JobInfo) => boolean): number | null {
  let [sum, n, running, waiting] = [0, 0, false, false]
  for (const id of s.order) {
    const j = s.jobs[id]
    if (isFinished(j) || !of(j)) continue
    if (j.state === "queued") {
      waiting = true
      continue
    }
    running = true
    if (j.progress != null) {
      sum += j.progress
      n += 1
    }
  }
  if (n > 0) return sum / n
  return waiting && !running ? 0 : null
}

/**
 * How far running work is, as one number: the mean progress of the running
 * jobs that report one (`null`: none of them has told yet; `0`: all wait).
 */
export function useActiveProgress(): number | null {
  return useActivity((s) => meanProgress(s, () => true))
}

/** How far the indexing of `projectId` is, as `useActiveProgress` tells it for all work. */
export function useProjectIndexProgress(projectId: string): number | null {
  return useActivity((s) => meanProgress(s, (j) => j.projectId === projectId && j.kind === "ingest"))
}

/**
 * Does all of `projectId`'s indexing wait its turn? A running job's stage
 * starts at 0 too, so its ring is told waiting apart from a 0.
 */
export function useProjectIndexWaiting(projectId: string): boolean {
  return useActivity((s) => allWait(s, (j) => j.projectId === projectId && j.kind === "ingest"))
}

/** Is any media of `projectId` being indexed right now? */
export function useProjectIndexing(projectId: string): boolean {
  return useActivity((s) =>
    s.order.some((id) => {
      const j = s.jobs[id]
      return j.projectId === projectId && j.kind === "ingest" && !isFinished(j)
    }),
  )
}
