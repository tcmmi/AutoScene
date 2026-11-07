import { QueryClient, keepPreviousData, useQuery } from "@tanstack/react-query"
import { commands, type CharacterId, type DiscordRpcSettings, type FaceTrackItem, type MediaId, type PluginInfo, type ProjectId, type TrackFilter } from "./ipc"

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Data changes are pushed as events that invalidate queries; never poll.
      staleTime: Infinity,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
})

/** Query keys. `lib/events.ts` maps backend events onto these. */
export const qk = {
  appInfo: ["appInfo"] as const,
  settings: ["settings"] as const,
  uiLocale: ["uiLocale"] as const,
  scanDefaults: ["scanDefaults"] as const,
  discordPreview: (show: DiscordRpcSettings) => ["discordPreview", show] as const,
  hardware: ["hardware"] as const,
  mediaTools: ["mediaTools"] as const,
  presets: ["exportPresets"] as const,
  projects: ["projects"] as const,
  project: (id: ProjectId) => ["project", id] as const,
  media: (pid: ProjectId) => ["media", pid] as const,
  frameMap: (pid: ProjectId, mid: MediaId) => ["frameMap", pid, mid] as const,
  segments: (pid: ProjectId, mid: MediaId) => ["segments", pid, mid] as const,
  exports: (pid: ProjectId) => ["exports", pid] as const,
  cast: (pid: ProjectId) => ["cast", pid] as const,
  tracks: (pid: ProjectId, filter: TrackFilter) => ["cast", pid, "tracks", filter] as const,
  // Outside the cast: a face's portrait doesn't change when the cast does.
  // The face's place keeps a track number handed out again apart.
  portraits: (pid: ProjectId) => ["portrait", pid] as const,
  portrait: (pid: ProjectId, t: Pick<FaceTrackItem, "id" | "mediaId" | "start">) =>
    ["portrait", pid, t.id, t.mediaId, t.start] as const,
  presence: (pid: ProjectId, mid: MediaId) => ["cast", pid, "presence", mid] as const,
  characterShots: (pid: ProjectId, cid: CharacterId) => ["cast", pid, "shots", cid] as const,
  // Under the cast: a pack is made from it, so every cast change refreshes it.
  pack: (pid: ProjectId, cid: CharacterId) => ["cast", pid, "pack", cid] as const,
  // The pack the filters make, and its names and sizes: one query per filters (their spec follows).
  packPreviews: (pid: ProjectId, cid: CharacterId) => ["cast", pid, "pack", cid, "preview"] as const,
  packPlans: (pid: ProjectId, cid: CharacterId) => ["cast", pid, "pack", cid, "plan"] as const,
  // Under the cast: a name waits only for a character still shown and
  // unnamed. The look-up says it began and ended with `anilist:changed`.
  anilistNames: (pid: ProjectId) => ["cast", pid, "anilist"] as const,
  // Under the cast: an edit may answer what Rescan asks.
  rescan: (pid: ProjectId) => ["cast", pid, "rescan"] as const,
  exportDir: (pid: ProjectId) => ["exportDir", pid] as const,
  plugins: ["plugins"] as const,
  // Under the plugins: `plugins:changed` refreshes both.
  pluginSettings: (id: string) => ["plugins", id, "settings"] as const,
  // Outside the plugins: `plugins:changed` fires on every settings save, and
  // a banner is up to 512 KB. Another picture comes with another hash.
  pluginBanner: (id: string, hash: string | null) => ["pluginBanner", id, hash] as const,
  // Outside the plugins, but what it offers is worked out against them:
  // `plugins:changed` and `catalog:changed` refresh it.
  catalog: ["catalog"] as const,
}

/**
 * A pack as the filters make it — its preview or its plan — of any
 * character in any project. It shows what the plugins do (their filters,
 * whether they are on), so what changes them (`plugins:changed`) makes
 * every one again, also those not on screen, and so does a new language
 * (`chooseLanguage`): a plan names files in the backend's. The settings it
 * was made with are not part of it.
 */
export const isMadePack = (q: { queryKey: readonly unknown[] }) =>
  q.queryKey[0] === "cast" && q.queryKey[2] === "pack" && (q.queryKey[4] === "preview" || q.queryKey[4] === "plan")

export function useAppInfo() {
  return useQuery({ queryKey: qk.appInfo, queryFn: () => commands.appInfo() })
}

export function useSettings() {
  return useQuery({ queryKey: qk.settings, queryFn: () => commands.settingsGet() })
}

/** The language AutoScene speaks, the choice it comes from and what Automatic comes to here. */
export function useUiLocale() {
  return useQuery({ queryKey: qk.uiLocale, queryFn: () => commands.uiLocale() })
}

/**
 * What Settings shows as the person's status. While a switch's answer is on
 * its way the last one stays: the same two samples, so the page doesn't jump.
 */
export function useDiscordPreview(show: DiscordRpcSettings) {
  return useQuery({
    queryKey: qk.discordPreview(show),
    queryFn: () => commands.discordPreview(show),
    placeholderData: keepPreviousData,
  })
}

/**
 * The scan new projects start with, and why. Read afresh wherever it is shown:
 * a choice in Settings or the faceless-people switch changes it.
 */
export function useScanDefaults() {
  return useQuery({ queryKey: qk.scanDefaults, queryFn: () => commands.scanDefaultsGet(), staleTime: 0 })
}

/**
 * The graphics the analysis can run on, and the one it does. The first ask
 * of a session starts the engine to look (a second or so); `hardware:changed`
 * refreshes it after that, and a failure waits for Try again.
 */
export function useHardware() {
  return useQuery({ queryKey: qk.hardware, queryFn: () => commands.hardwareInfo(), retry: false })
}

export function useMediaTools() {
  return useQuery({ queryKey: qk.mediaTools, queryFn: () => commands.mediaTools() })
}

export function usePresets() {
  return useQuery({ queryKey: qk.presets, queryFn: () => commands.exportPresets() })
}

export function useProjects() {
  return useQuery({ queryKey: qk.projects, queryFn: () => commands.projectsList() })
}

export function useProject(id: ProjectId, enabled = true) {
  return useQuery({ queryKey: qk.project(id), queryFn: () => commands.projectGet(id), enabled })
}

export function useMediaList(pid: ProjectId, enabled = true) {
  return useQuery({ queryKey: qk.media(pid), queryFn: () => commands.mediaList(pid), enabled })
}

export function useFrameMap(pid: ProjectId, mid: MediaId, enabled: boolean) {
  return useQuery({
    queryKey: qk.frameMap(pid, mid),
    queryFn: () => commands.mediaFrameMap(pid, mid),
    enabled,
    retry: false,
  })
}

/** Shots and transitions of a media (empty until detected). */
export function useSegments(pid: ProjectId, mid: MediaId, enabled: boolean) {
  return useQuery({ queryKey: qk.segments(pid, mid), queryFn: () => commands.mediaSegments(pid, mid), enabled })
}

/** A project's exports, newest first. */
export const exportsQuery = (pid: ProjectId) => ({ queryKey: qk.exports(pid), queryFn: () => commands.exportsList(pid) })

export function useExports(pid: ProjectId) {
  return useQuery(exportsQuery(pid))
}

export function useExportDir(pid: ProjectId) {
  return useQuery({ queryKey: qk.exportDir(pid), queryFn: () => commands.exportDefaultDir(pid) })
}

/** The project's characters and where face analysis stands. */
export function useCast(pid: ProjectId, enabled = true) {
  return useQuery({ queryKey: qk.cast(pid), queryFn: () => commands.castGet(pid), enabled })
}

/** Names from AniList waiting for a yes, and whether the look-up runs (the
 *  first ask starts it for a project never looked up). */
export function useAniListNames(pid: ProjectId, enabled: boolean) {
  return useQuery({ queryKey: qk.anilistNames(pid), queryFn: () => commands.anilistNames(pid), enabled })
}

/** What Rescan is waiting to hear about. */
export function useRescanResults(pid: ProjectId, enabled = true) {
  return useQuery({ queryKey: qk.rescan(pid), queryFn: () => commands.rescanResults(pid), enabled })
}

export function useCastTracks(pid: ProjectId, filter: TrackFilter, enabled = true) {
  return useQuery({ queryKey: qk.tracks(pid, filter), queryFn: () => commands.castTracks(pid, filter), enabled })
}

/**
 * A face's sharp portrait for showing it big (made on first ask, from the
 * exact frame); `null` while it can't be made. Show the thumbnail meanwhile.
 */
export const portraitQuery = (pid: ProjectId, t: FaceTrackItem) => ({
  queryKey: qk.portrait(pid, t),
  queryFn: () => commands.castPortrait(pid, t.id),
  // A failure leaves the thumbnail; it isn't worth a retry.
  retry: false,
})

export function usePortrait(pid: ProjectId, t: FaceTrackItem | undefined) {
  return useQuery({
    queryKey: t ? qk.portrait(pid, t) : qk.portraits(pid),
    queryFn: () => commands.castPortrait(pid, t!.id),
    retry: false,
    enabled: t !== undefined,
  })
}

/** Every shot a character is seen in, in episode order. */
export function useCharacterShots(pid: ProjectId, cid: CharacterId, enabled = true) {
  return useQuery({
    queryKey: qk.characterShots(pid, cid),
    queryFn: () => commands.characterShots(pid, cid),
    enabled,
  })
}

/** Where characters are on screen in one media. */
export function usePresence(pid: ProjectId, mid: MediaId, enabled: boolean) {
  return useQuery({ queryKey: qk.presence(pid, mid), queryFn: () => commands.mediaPresence(pid, mid), enabled })
}

/** AutoScene's plugin catalog as it holds it: never the network (`checkCatalog` asks it). */
export function useCatalog() {
  return useQuery({ queryKey: qk.catalog, queryFn: () => commands.catalogList() })
}

/** The installed plugins, broken ones too. */
export function usePlugins() {
  return useQuery({ queryKey: qk.plugins, queryFn: () => commands.pluginsList() })
}

/** A plugin's settings: the person's value of each, else its default. */
export function usePluginSettings(id: string) {
  return useQuery({ queryKey: qk.pluginSettings(id), queryFn: () => commands.pluginSettingsGet(id) })
}

/** What a plugin's banner may be: its PNG, JPEG or WebP as a base64 `data:` URL (the backend reads and checks the file). */
const BANNER_URL = /^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/

/** `url` while it is a banner in hand. One function, so a query checks each answer once. */
const pictureOnly = (url: string | null) => (url != null && BANNER_URL.test(url) ? url : null)

/**
 * A plugin's banner as a `data:` URL: fetched once for each of its hashes,
 * never without one, and only ever a picture in hand, never a link.
 */
export function usePluginBanner(p: Pick<PluginInfo, "id" | "bannerHash">) {
  return useQuery({
    queryKey: qk.pluginBanner(p.id, p.bannerHash),
    queryFn: () => commands.pluginBanner(p.id),
    enabled: p.bannerHash != null,
    staleTime: Infinity,
    select: pictureOnly,
  })
}

type Refetchable = { isError: boolean; error: unknown; data?: unknown; refetch: () => unknown }

/**
 * The first of a view's queries that failed with nothing to show, and a retry
 * that fetches those again. A refetch that fails keeps its last data on screen.
 */
export function firstFailure(...queries: Refetchable[]): { error: unknown; retry: () => void } | null {
  const failed = queries.filter((q) => q.isError && q.data === undefined)
  if (failed.length === 0) return null
  return { error: failed[0].error, retry: () => failed.forEach((q) => void q.refetch()) }
}
