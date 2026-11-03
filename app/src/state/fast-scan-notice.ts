import { create } from "zustand"

/**
 * The first-start Fast Scan notice (`shell/FastScanNotice.tsx`):
 * - waiting: for the splash and the first answer;
 * - due: that answer said so; it opens the first moment the library is free;
 * - open: the update dialog waits meanwhile, so two modals never stack;
 * - done: for this run.
 * Decided once per run, so a change made later (in Settings) never opens it.
 */
export type NoticePhase = "waiting" | "due" | "open" | "done"

export const useFastScanNotice = create<{ phase: NoticePhase }>(() => ({ phase: "waiting" }))

/** Whether the notice is open. */
export const noticeOpen = (s: { phase: NoticePhase }) => s.phase === "open"
