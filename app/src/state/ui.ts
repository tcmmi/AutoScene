import { create } from "zustand"
import { persist } from "zustand/middleware"

export type CastViewMode = "grid" | "list"
/** The tabs of the player's details panel. */
export type PlayerTab = "cast" | "file"

type UiState = {
  sidebarCollapsed: boolean
  /** The player's details panel (Cast, File). */
  inspectorOpen: boolean
  /** Its tab. Read it with `playerTabOf`: a stored value it doesn't know opens Cast. */
  playerTab: PlayerTab
  paletteOpen: boolean
  activityOpen: boolean
  castView: CastViewMode
  toggleSidebar: () => void
  toggleInspector: () => void
  setPaletteOpen: (open: boolean) => void
  setActivityOpen: (open: boolean) => void
  setCastView: (view: CastViewMode) => void
  setPlayerTab: (tab: PlayerTab) => void
}

export const useUi = create<UiState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      inspectorOpen: true,
      paletteOpen: false,
      activityOpen: false,
      castView: "grid",
      playerTab: "cast",
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
      setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
      setActivityOpen: (activityOpen) => set({ activityOpen }),
      setCastView: (castView) => set({ castView }),
      setPlayerTab: (playerTab) => set({ playerTab }),
    }),
    {
      name: "autoscene.ui",
      // Only layout preferences survive a restart; overlays always start closed.
      partialize: (s) => ({
        sidebarCollapsed: s.sidebarCollapsed,
        inspectorOpen: s.inspectorOpen,
        castView: s.castView,
        playerTab: s.playerTab,
      }),
    },
  ),
)

/** The details panel's tab; anything else stored (an older or newer version, a hand edit) opens Cast. */
export function playerTabOf(v: unknown): PlayerTab {
  return v === "file" ? "file" : "cast"
}
