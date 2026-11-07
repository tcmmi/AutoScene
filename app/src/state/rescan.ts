import { create } from "zustand"

/** How long a face counts as just read: its sweep crosses it (see `ScanSweep`). */
const SWEEP_MS = 1200

/** `jobs`: rescans asked for and not ended; one may wait behind another. */
type Run = { done: number; total: number; characters: string[]; jobs: number }

type RescanState = {
  /** What runs, by project. Progress stays here, out of React Query. */
  runs: Record<string, Run>
  /** Faces read a moment ago. */
  sweeping: Set<number>
  /** A rescan of these characters was asked for. */
  started: (pid: string, characters: string[]) => void
  /** The backend read `looked`, `done` of `total` by now. */
  progress: (pid: string, looked: number[], done: number, total: number) => void
  ended: (pid: string) => void
}

export const useRescan = create<RescanState>((set) => ({
  runs: {},
  sweeping: new Set(),
  started: (pid, characters) =>
    set((s) => {
      const run = s.runs[pid]
      const all = [...new Set([...(run?.characters ?? []), ...characters])]
      return { runs: { ...s.runs, [pid]: { done: run?.done ?? 0, total: run?.total ?? 0, characters: all, jobs: (run?.jobs ?? 0) + 1 } } }
    }),
  progress: (pid, looked, done, total) => {
    set((s) => ({
      runs: { ...s.runs, [pid]: { characters: s.runs[pid]?.characters ?? [], jobs: s.runs[pid]?.jobs ?? 1, done, total } },
      sweeping: looked.length > 0 ? new Set([...s.sweeping, ...looked]) : s.sweeping,
    }))
    if (looked.length === 0) return
    setTimeout(() => {
      set((s) => {
        const sweeping = new Set(s.sweeping)
        for (const id of looked) sweeping.delete(id)
        return { sweeping }
      })
    }, SWEEP_MS)
  },
  ended: (pid) =>
    set((s) => {
      const runs = { ...s.runs }
      const run = runs[pid]
      if (run && run.jobs > 1) runs[pid] = { ...run, jobs: run.jobs - 1, done: 0, total: 0 }
      else delete runs[pid]
      return { runs }
    }),
}))
