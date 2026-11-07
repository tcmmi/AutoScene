import { create } from "zustand"

/** How long a face counts as just read: its sweep crosses it (see `ScanSweep`). */
const SWEEP_MS = 1200

type Run = { done: number; total: number; characters: string[] }

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
  started: (pid, characters) => set((s) => ({ runs: { ...s.runs, [pid]: { done: 0, total: 0, characters } } })),
  progress: (pid, looked, done, total) => {
    set((s) => ({
      runs: { ...s.runs, [pid]: { characters: s.runs[pid]?.characters ?? [], done, total } },
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
      delete runs[pid]
      return { runs }
    }),
}))
