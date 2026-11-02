import { useState } from "react"
import {
  ArrowCounterClockwise,
  ArrowSquareOut,
  Check,
  DownloadSimple,
  FolderOpen,
  GithubLogo,
  Info,
  MagnifyingGlass,
  Package,
  Play,
  Trash,
} from "@/components/icons"
import { toast } from "sonner"
import type { ExtensionKind, InstalledExtension } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { Sheet, SheetContent } from "@/components/ui/sheet"
import { Switch } from "@/components/ui/switch"
import { TopBar } from "@/components/ui/topbar"
import { commands, errorMessage } from "@/lib/ipc"
import { convertFileSrc } from "@tauri-apps/api/core"
import { openExternalUrl } from "@/lib/os"
import { isTauri } from "@/lib/platform"
import { qk, queryClient, useAppInfo, useExtensions, useSettings } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { type CatalogExtension, EXTENSIONS_CATALOG } from "./ExtensionsCatalog"

const KINDS: { id: ExtensionKind | "all"; label: string }[] = [
  { id: "all", label: "All" },
  { id: "exporter", label: "Exporters" },
  { id: "post-processor", label: "Post-Processing" },
  { id: "webhook", label: "Webhooks" },
  { id: "custom", label: "Custom" },
]

export function ExtensionsPage() {
  const { data: info } = useAppInfo()
  const { data: settings } = useSettings()
  const { data: extensions = [], isFetching } = useExtensions()
  const [tab, setTab] = useState<"installed" | "discover">("installed")
  const [filter, setFilter] = useState<ExtensionKind | "all">("all")
  const [search, setSearch] = useState("")
  const [selectedDetails, setSelectedDetails] = useState<CatalogExtension | InstalledExtension | null>(null)

  const reload = async () => {
    try {
      await queryClient.invalidateQueries({ queryKey: qk.extensions })
      toast.success("Extensions reloaded")
    } catch (e) {
      toast.error("Couldn't reload extensions", { description: errorMessage(e) })
    }
  }

  const openFolder = async () => {
    try {
      await commands.openExtensionsDir()
    } catch (e) {
      toast.error("Couldn't open extensions directory", { description: errorMessage(e) })
    }
  }

  const toggleExtension = async (id: string, enabled: boolean) => {
    if (!settings) return
    const currentDisabled = settings.disabledExtensions ?? []
    const nextDisabled = enabled
      ? currentDisabled.filter((x) => x !== id)
      : Array.from(new Set([...currentDisabled, id]))

    try {
      const updated = await commands.settingsUpdate({
        ...settings,
        disabledExtensions: nextDisabled,
      })
      queryClient.setQueryData(qk.settings, updated)
      toast.success(enabled ? "Extension enabled" : "Extension disabled")
    } catch (e) {
      toast.error("Couldn't update extension state", { description: errorMessage(e) })
    }
  }

  const uninstallExtension = async (id: string, name: string) => {
    try {
      await commands.extensionUninstall(id)
      await queryClient.invalidateQueries({ queryKey: qk.extensions })
      toast.success(`Uninstalled ${name}`)
      if (selectedDetails && ("manifest" in selectedDetails ? selectedDetails.manifest.id : selectedDetails.id) === id) {
        setSelectedDetails(null)
      }
    } catch (e) {
      toast.error(`Couldn't uninstall: ${errorMessage(e)}`)
    }
  }

  const disabledSet = new Set(settings?.disabledExtensions ?? [])
  const installedIds = new Set(extensions.map((e) => e.manifest.id))

  const filteredInstalled = extensions.filter((ext) => {
    if (filter !== "all" && ext.manifest.kind !== filter) return false
    if (!search.trim()) return true
    const q = search.toLowerCase()
    return (
      ext.manifest.name.toLowerCase().includes(q) ||
      ext.manifest.description.toLowerCase().includes(q) ||
      ext.manifest.author.toLowerCase().includes(q) ||
      ext.manifest.id.toLowerCase().includes(q)
    )
  })

  const filteredCatalog = EXTENSIONS_CATALOG.filter((item) => {
    if (filter !== "all" && item.kind !== filter) return false
    if (!search.trim()) return true
    const q = search.toLowerCase()
    return (
      item.name.toLowerCase().includes(q) ||
      item.description.toLowerCase().includes(q) ||
      item.author.toLowerCase().includes(q) ||
      item.id.toLowerCase().includes(q)
    )
  })

  const menu = (
    <>
      <ContextMenuItem onSelect={reload}>
        <ArrowCounterClockwise /> Reload extensions
      </ContextMenuItem>
      <ContextMenuItem onSelect={openFolder}>
        <FolderOpen /> Open extensions folder
      </ContextMenuItem>
    </>
  )

  return (
    <div className="flex h-full flex-col">
      <TopBar title="Extensions">
        <div className="flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={reload} disabled={isFetching}>
            <ArrowCounterClockwise className={isFetching ? "animate-spin" : undefined} />
            {isFetching ? "Refreshing…" : "Refresh"}
          </Button>
          <Button size="sm" variant="ghost" onClick={openFolder}>
            <FolderOpen /> Open folder
          </Button>
        </div>
      </TopBar>

      <ContextArea kind="page" menu={menu}>
        <div className="flex-1 overflow-y-auto p-8">
          <div className="mx-auto max-w-5xl">
            {/* Header Hero Banner */}
            <div className="relative mb-6 flex flex-col justify-between gap-6 overflow-hidden rounded-3xl border border-white/[0.08] bg-[radial-gradient(120%_140%_at_0%_0%,rgb(224_180_92/0.14),transparent_55%),rgb(255_255_255/0.03)] p-8 sm:flex-row sm:items-center">
              <div className="min-w-0">
                <div className="flex items-center gap-3">
                  <h1 className="display text-3xl font-semibold text-fg-1">Extensions Hub</h1>
                  <span className="rounded-full border border-white/10 bg-white/[0.04] px-2.5 py-0.5 text-xs text-fg-3">
                    Library & Marketplace
                  </span>
                </div>
                <p className="mt-1.5 max-w-2xl text-md text-fg-2">
                  Enhance AutoScene with out-of-process exporters, notification webhooks, automated tagging, and media
                  post-processing pipelines.
                </p>
                {info && (
                  <div className="mt-3 flex items-center gap-2 text-xs text-fg-3">
                    <span className="font-mono text-fg-4">{info.extensionsDir}</span>
                  </div>
                )}
              </div>
              <div className="flex shrink-0 gap-3">
                <Button variant="secondary" onClick={reload} disabled={isFetching}>
                  <ArrowCounterClockwise className={isFetching ? "animate-spin" : undefined} />
                  Refresh
                </Button>
                <Button variant="primary" onClick={openFolder}>
                  <FolderOpen /> Open Folder
                </Button>
              </div>
            </div>

            {/* Navigation Tabs (Installed vs Discover Library) */}
            <div className="mb-6 flex border-b border-white/[0.08] pb-px">
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setTab("installed")}
                  className={cn(
                    "relative pb-3 text-sm font-semibold transition-colors",
                    tab === "installed"
                      ? "text-fg-1 after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-accent"
                      : "text-fg-3 hover:text-fg-2",
                  )}
                >
                  Installed ({extensions.length})
                </button>
                <button
                  type="button"
                  onClick={() => setTab("discover")}
                  className={cn(
                    "relative pb-3 text-sm font-semibold transition-colors",
                    tab === "discover"
                      ? "text-fg-1 after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-accent"
                      : "text-fg-3 hover:text-fg-2",
                  )}
                >
                  Discover Library
                </button>
              </div>
            </div>

            {/* Filter and Search Bar */}
            <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
              {/* Category pills */}
              <div className="flex items-center gap-1.5 overflow-x-auto rounded-xl border border-white/[0.08] bg-white/[0.02] p-1">
                {KINDS.map((k) => (
                  <button
                    key={k.id}
                    onClick={() => setFilter(k.id)}
                    className={cn(
                      "rounded-lg px-3 py-1.5 text-xs font-medium transition-colors",
                      filter === k.id
                        ? "bg-white/[0.12] text-fg-1 shadow-sm"
                        : "text-fg-3 hover:bg-white/[0.05] hover:text-fg-2",
                    )}
                  >
                    {k.label}
                  </button>
                ))}
              </div>

              {/* Search box */}
              <div className="relative flex items-center">
                <MagnifyingGlass className="pointer-events-none absolute left-3 size-4 text-fg-4" />
                <input
                  type="text"
                  placeholder={tab === "installed" ? "Search installed extensions…" : "Search marketplace library…"}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="h-9 w-64 rounded-xl border border-white/[0.08] bg-white/[0.03] pr-3 pl-9 text-sm text-fg-1 placeholder:text-fg-4 focus:border-white/20 focus:outline-none"
                />
              </div>
            </div>

            {/* Content for Installed Tab */}
            {tab === "installed" && (
              <>
                {filteredInstalled.length === 0 ? (
                  <div className="flex flex-col items-center justify-center rounded-3xl border border-dashed border-white/10 bg-white/[0.01] p-16 text-center">
                    <Package className="mb-3 size-10 text-fg-4" />
                    <h3 className="text-lg font-medium text-fg-2">
                      {search ? "No matching extensions" : "No extensions installed yet"}
                    </h3>
                    <p className="mt-1 max-w-md text-sm text-fg-4">
                      {search
                        ? `No extension matches "${search}". Try searching for another keyword or clear the filter.`
                        : "Discover and install extensions from the Library tab, or place your extension folder inside the extensions directory."}
                    </p>
                    <div className="mt-4 flex items-center gap-2">
                      <Button variant="primary" size="sm" onClick={() => setTab("discover")}>
                        <DownloadSimple /> Browse Library
                      </Button>
                      <Button variant="secondary" size="sm" onClick={openFolder}>
                        <FolderOpen /> Open Folder
                      </Button>
                    </div>
                  </div>
                ) : (
                  <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                    {filteredInstalled.map((ext) => {
                      const enabled = !disabledSet.has(ext.manifest.id)
                      return (
                        <ExtensionCard
                          key={ext.manifest.id}
                          extension={ext}
                          enabled={enabled}
                          onToggle={(next) => void toggleExtension(ext.manifest.id, next)}
                          onSelect={() => setSelectedDetails(ext)}
                          onUninstall={() => void uninstallExtension(ext.manifest.id, ext.manifest.name)}
                        />
                      )
                    })}
                  </div>
                )}
              </>
            )}

            {/* Content for Discover Library Tab */}
            {tab === "discover" && (
              <div className="flex flex-col gap-6">
                {/* Featured spotlight */}
                {!search && filter === "all" && (
                  <div className="relative overflow-hidden rounded-3xl border border-white/[0.1] bg-gradient-to-r from-indigo-950/40 via-purple-950/20 to-black/40 p-6">
                    <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                      <div className="max-w-xl">
                        <span className="rounded-full border border-indigo-400/20 bg-indigo-500/10 px-2.5 py-0.5 text-[11px] font-semibold uppercase tracking-wider text-indigo-300">
                          Featured Community Workflow
                        </span>
                        <h2 className="mt-2 text-xl font-bold text-fg-1">AMVerge Deadframe Remover</h2>
                        <p className="mt-1 text-sm text-fg-2">
                          Automatically clean dead, motionless holding frames from exported character clips using dense
                          optical flow vectors.
                        </p>
                      </div>
                      <Button
                        variant="primary"
                        onClick={() => {
                          const item = EXTENSIONS_CATALOG.find((x) => x.id === "amverge.deadframes.remover")
                          if (item) setSelectedDetails(item)
                        }}
                      >
                        <Info /> View Details
                      </Button>
                    </div>
                  </div>
                )}

                {/* Catalog Cards Grid */}
                <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                  {filteredCatalog.map((item) => {
                    const isInstalled = installedIds.has(item.id)
                    return (
                      <CatalogCard
                        key={item.id}
                        item={item}
                        isInstalled={isInstalled}
                        onSelect={() => setSelectedDetails(item)}
                      />
                    )
                  })}
                </div>
              </div>
            )}
          </div>
        </div>
      </ContextArea>

      {/* Detail Slide-out Sheet */}
      <ExtensionDetailsDrawer
        item={selectedDetails}
        installed={selectedDetails ? ("manifest" in selectedDetails ? true : installedIds.has(selectedDetails.id)) : false}
        onClose={() => setSelectedDetails(null)}
      />
    </div>
  )
}

function ExtensionCard({
  extension,
  enabled,
  onToggle,
  onSelect,
  onUninstall,
}: {
  extension: InstalledExtension
  enabled: boolean
  onToggle: (enabled: boolean) => void
  onSelect: () => void
  onUninstall: () => void
}) {
  const { manifest } = extension
  const [testing, setTesting] = useState(false)

  const runTest = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setTesting(true)
    try {
      const res = await commands.extensionTest(manifest.id)
      toast.success(res)
    } catch (err) {
      toast.error("Extension test failed", { description: errorMessage(err) })
    } finally {
      setTesting(false)
    }
  }

  const iconSrc = extension.iconPath ? (isTauri ? convertFileSrc(extension.iconPath) : extension.iconPath) : null
  const bannerSrc = extension.bannerPath ? (isTauri ? convertFileSrc(extension.bannerPath) : extension.bannerPath) : null
  const accent = manifest.accentColor ?? "#e0b45c"

  const authorAvatarSrc = extension.authorAvatarPath
    ? isTauri
      ? convertFileSrc(extension.authorAvatarPath)
      : extension.authorAvatarPath
    : manifest.authorAvatar?.startsWith("http")
      ? manifest.authorAvatar
      : manifest.authorGithub
        ? `https://github.com/${manifest.authorGithub.replace(/^@/, "").trim()}.png?size=64`
        : null

  return (
    <div
      onClick={onSelect}
      className={cn(
        "group relative flex cursor-pointer flex-col justify-between overflow-hidden rounded-2xl border border-white/[0.08] transition-all",
        enabled ? "bg-white/[0.02] hover:border-white/[0.16] hover:bg-white/[0.03]" : "bg-black/20 opacity-60",
      )}
    >
      {/* Banner Header */}
      <div className="relative h-24 w-full overflow-hidden bg-white/[0.03]">
        {bannerSrc ? (
          <img
            src={bannerSrc}
            alt=""
            className="size-full object-cover transition-transform duration-500 group-hover:scale-105"
          />
        ) : (
          <div
            className="size-full opacity-35"
            style={{
              background: `radial-gradient(circle at top right, ${accent}33, transparent 70%), linear-gradient(to bottom, transparent, rgba(0,0,0,0.6))`,
            }}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-bg-canvas via-transparent to-black/20" />

        {/* Top Badges / Switch */}
        <div className="absolute top-3 right-3 flex items-center gap-2">
          <span className="shrink-0 rounded-full border border-white/[0.1] bg-black/60 px-2.5 py-0.5 text-[10px] font-medium uppercase tracking-wider text-fg-3 backdrop-blur-md">
            {manifest.kind}
          </span>
          <div
            className="rounded-full bg-black/40 p-0.5 backdrop-blur-md"
            onClick={(e) => e.stopPropagation()}
          >
            <Switch
              checked={enabled}
              onCheckedChange={onToggle}
              aria-label={`Toggle ${manifest.name}`}
            />
          </div>
        </div>
      </div>

      <div className="relative p-5 pt-0">
        {/* PFP / Icon overlapping banner */}
        <div className="-mt-8 mb-3 flex items-end justify-between">
          <div className="relative flex size-14 shrink-0 items-center justify-center overflow-hidden rounded-2xl border-2 border-bg-canvas bg-white/[0.06] shadow-xl">
            {iconSrc ? (
              <img src={iconSrc} alt={manifest.name} className="size-full object-cover" />
            ) : (
              <Package className="size-7 text-fg-3" />
            )}
          </div>

          {/* Social Links */}
          <div className="flex items-center gap-1.5" onClick={(e) => e.stopPropagation()}>
            {manifest.repository && (
              <Button
                size="sm"
                variant="ghost"
                className="size-8 p-0 text-fg-3 hover:text-fg-1"
                onClick={() => void openExternalUrl(manifest.repository!)}
                aria-label="View repository"
              >
                <GithubLogo className="size-4" />
              </Button>
            )}
            {manifest.homepage && (
              <Button
                size="sm"
                variant="ghost"
                className="size-8 p-0 text-fg-3 hover:text-fg-1"
                onClick={() => void openExternalUrl(manifest.homepage!)}
                aria-label="Visit homepage"
              >
                <ArrowSquareOut className="size-4" />
              </Button>
            )}
          </div>
        </div>

        {/* Title & Author */}
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-base font-semibold text-fg-1">{manifest.name}</h3>
            <span className="rounded bg-white/[0.08] px-1.5 py-0.5 font-mono text-[11px] text-fg-3">
              v{manifest.version}
            </span>
          </div>
          <div className="mt-1 flex items-center gap-1.5 text-xs text-fg-4">
            <span>By</span>
            <div className="inline-flex items-center gap-1.5">
              {authorAvatarSrc && (
                <img
                  src={authorAvatarSrc}
                  alt=""
                  className="size-4 rounded-full object-cover ring-1 ring-white/10"
                  onError={(e) => {
                    ;(e.target as HTMLElement).style.display = "none"
                  }}
                />
              )}
              <span className="font-medium text-fg-3">{manifest.author}</span>
            </div>
          </div>
        </div>

        <p className="mt-3 line-clamp-2 text-sm leading-relaxed text-fg-3">{manifest.description}</p>
      </div>

      {/* Footer Controls */}
      <div
        className="flex items-center justify-between border-t border-white/[0.06] bg-black/10 px-5 py-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            onClick={runTest}
            disabled={!enabled || testing}
          >
            <Play /> {testing ? "Running…" : "Run Test"}
          </Button>
          <span className="max-w-[130px] truncate font-mono text-xs text-fg-4">{manifest.entrypoint}</span>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            className="size-7 p-0 text-fg-4 hover:text-danger"
            onClick={onUninstall}
            aria-label="Uninstall extension"
          >
            <Trash className="size-3.5" />
          </Button>
        </div>
      </div>
    </div>
  )
}

function CatalogCard({
  item,
  isInstalled,
  onSelect,
}: {
  item: CatalogExtension
  isInstalled: boolean
  onSelect: () => void
}) {
  const authorAvatarSrc = item.authorGithub
    ? `https://github.com/${item.authorGithub}.png?size=64`
    : null

  return (
    <div
      onClick={onSelect}
      className="group relative flex cursor-pointer flex-col justify-between overflow-hidden rounded-2xl border border-white/[0.08] bg-white/[0.02] transition-all hover:border-white/[0.16] hover:bg-white/[0.03]"
    >
      {/* Card Header Gradient */}
      <div className={cn("relative h-20 w-full overflow-hidden bg-gradient-to-r", item.bannerGradient)}>
        <div className="absolute inset-0 bg-gradient-to-t from-bg-canvas via-transparent to-black/20" />
        <div className="absolute top-3 right-3 flex items-center gap-1.5">
          <span className="shrink-0 rounded-full border border-white/[0.1] bg-black/60 px-2.5 py-0.5 text-[10px] font-medium uppercase tracking-wider text-fg-3 backdrop-blur-md">
            {item.kind}
          </span>
          {isInstalled && (
            <span className="flex items-center gap-1 rounded-full border border-emerald-500/30 bg-emerald-950/70 px-2 py-0.5 text-[10px] font-medium text-emerald-400 backdrop-blur-md">
              <Check className="size-3" /> Installed
            </span>
          )}
        </div>
      </div>

      <div className="relative p-5 pt-0">
        <div className="-mt-7 mb-3 flex items-end justify-between">
          <div
            className="relative flex size-12 shrink-0 items-center justify-center overflow-hidden rounded-2xl border-2 border-bg-canvas shadow-xl"
            style={{ backgroundColor: `${item.accentColor}25` }}
          >
            <Package className="size-6" style={{ color: item.accentColor }} />
          </div>
          <span className="text-xs text-fg-4">{item.downloads} installs</span>
        </div>

        <h3 className="truncate text-base font-semibold text-fg-1">{item.name}</h3>
        <div className="mt-1 flex items-center gap-1.5 text-xs text-fg-4">
          <span>By</span>
          <div className="inline-flex items-center gap-1.5">
            {authorAvatarSrc && (
              <img
                src={authorAvatarSrc}
                alt=""
                className="size-4 rounded-full object-cover ring-1 ring-white/10"
                onError={(e) => {
                  ;(e.target as HTMLElement).style.display = "none"
                }}
              />
            )}
            <span className="font-medium text-fg-3">{item.author}</span>
          </div>
        </div>
        <p className="mt-2.5 line-clamp-2 text-sm leading-relaxed text-fg-3">{item.description}</p>
      </div>

      <div className="flex items-center justify-between border-t border-white/[0.06] bg-black/10 px-5 py-3">
        <div className="flex flex-wrap gap-1">
          {item.capabilities.map((c) => (
            <span key={c} className="rounded border border-white/[0.08] bg-white/[0.03] px-1.5 py-0.5 text-[10px] text-fg-4">
              {c}
            </span>
          ))}
        </div>
        <span className="text-xs font-semibold text-fg-2 group-hover:text-fg-1">View Details →</span>
      </div>
    </div>
  )
}

function ExtensionDetailsDrawer({
  item,
  installed,
  onClose,
}: {
  item: CatalogExtension | InstalledExtension | null
  installed: boolean
  onClose: () => void
}) {
  if (!item) return null

  const manifest = "manifest" in item ? item.manifest : item
  const accent = manifest.accentColor ?? "#e0b45c"

  return (
    <Sheet open={item != null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent title="Extension Details" className="w-[min(480px,calc(100vw-24px))]">
        <div className="p-6">
          {/* Header Banner */}
          <div
            className="relative mb-5 flex h-28 items-end rounded-2xl p-4 shadow-inner"
            style={{
              background: `radial-gradient(circle at top right, ${accent}40, transparent 70%), linear-gradient(to bottom, transparent, rgba(0,0,0,0.8))`,
            }}
          >
            <div className="flex items-center gap-3">
              <div className="flex size-12 items-center justify-center rounded-xl bg-black/60 backdrop-blur-md">
                <Package className="size-6" style={{ color: accent }} />
              </div>
              <div className="min-w-0">
                <h2 className="truncate text-lg font-bold text-fg-1">{manifest.name}</h2>
                <span className="text-xs text-fg-3">v{manifest.version}</span>
              </div>
            </div>
          </div>

          <div className="space-y-4">
            <div>
              <h4 className="text-xs font-semibold uppercase tracking-wider text-fg-4">About</h4>
              <p className="mt-1 text-sm leading-relaxed text-fg-2">{manifest.description}</p>
            </div>

            <div>
              <h4 className="text-xs font-semibold uppercase tracking-wider text-fg-4">Author</h4>
              <div className="mt-1.5 flex items-center justify-between">
                <span className="text-sm font-medium text-fg-1">{manifest.author}</span>
                {manifest.authorGithub && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => void openExternalUrl(`https://github.com/${manifest.authorGithub}`)}
                  >
                    <GithubLogo /> @{manifest.authorGithub}
                  </Button>
                )}
              </div>
            </div>

            <div>
              <h4 className="text-xs font-semibold uppercase tracking-wider text-fg-4">Declared Permissions</h4>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {manifest.capabilities.map((c) => (
                  <span
                    key={c}
                    className="rounded-lg border border-white/10 bg-white/[0.04] px-2.5 py-1 text-xs font-mono text-fg-2"
                  >
                    {c}
                  </span>
                ))}
              </div>
            </div>

            {"readme" in item && (
              <div>
                <h4 className="text-xs font-semibold uppercase tracking-wider text-fg-4">Documentation</h4>
                <div className="mt-2 rounded-xl border border-white/[0.08] bg-black/30 p-4 text-xs font-mono whitespace-pre-wrap text-fg-3">
                  {item.readme}
                </div>
              </div>
            )}

            <div className="pt-4 border-t border-white/[0.08] flex items-center gap-2">
              {installed ? (
                <Button variant="secondary" className="flex-1" disabled>
                  <Check /> Installed
                </Button>
              ) : (
                <Button
                  variant="primary"
                  className="flex-1"
                  onClick={() => {
                    toast.info("Opening extensions folder to place files...")
                    void commands.openExtensionsDir()
                  }}
                >
                  <DownloadSimple /> Install Extension
                </Button>
              )}
            </div>
          </div>
        </div>
      </SheetContent>
    </Sheet>
  )
}
