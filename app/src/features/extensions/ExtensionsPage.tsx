import { useMemo, useState } from "react"
import { ArrowCounterClockwise, ArrowSquareOut, FolderOpen, GithubLogo, MagnifyingGlass, Package, Play } from "@/components/icons"
import { toast } from "sonner"
import type { ExtensionKind, InstalledExtension } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { Switch } from "@/components/ui/switch"
import { TopBar } from "@/components/ui/topbar"
import { commands, errorMessage } from "@/lib/ipc"
import { convertFileSrc } from "@tauri-apps/api/core"
import { openExternalUrl } from "@/lib/os"
import { isTauri } from "@/lib/platform"
import { qk, queryClient, useAppInfo, useExtensions, useSettings } from "@/lib/queries"
import { cn } from "@/lib/utils"

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
  const [filter, setFilter] = useState<ExtensionKind | "all">("all")
  const [search, setSearch] = useState("")

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

  const disabledSet = useMemo(() => new Set(settings?.disabledExtensions ?? []), [settings?.disabledExtensions])

  const filtered = useMemo(() => {
    return extensions.filter((ext) => {
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
  }, [extensions, filter, search])

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
            {/* Header Hero banner */}
            <div className="relative mb-8 flex flex-col justify-between gap-6 overflow-hidden rounded-3xl border border-white/[0.08] bg-[radial-gradient(120%_140%_at_0%_0%,rgb(224_180_92/0.14),transparent_55%),rgb(255_255_255/0.03)] p-8 sm:flex-row sm:items-center">
              <div className="min-w-0">
                <h1 className="display text-3xl font-semibold text-fg-1">Extensions</h1>
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
                  placeholder="Search extensions…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="h-9 w-64 rounded-xl border border-white/[0.08] bg-white/[0.03] pr-3 pl-9 text-sm text-fg-1 placeholder:text-fg-4 focus:border-white/20 focus:outline-none"
                />
              </div>
            </div>

            {/* Extension Cards Grid */}
            {filtered.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-3xl border border-dashed border-white/10 bg-white/[0.01] p-16 text-center">
                <Package className="mb-3 size-10 text-fg-4" />
                <h3 className="text-lg font-medium text-fg-2">
                  {search ? "No matching extensions" : "No extensions installed yet"}
                </h3>
                <p className="mt-1 max-w-md text-sm text-fg-4">
                  {search
                    ? `No extension matches "${search}". Try searching for another keyword or clear the filter.`
                    : "Add extensions by placing an extension directory with an extension.json manifest inside your extensions folder."}
                </p>
                <div className="mt-4 flex items-center gap-2">
                  <Button variant="ghost" size="sm" onClick={reload}>
                    <ArrowCounterClockwise /> Refresh
                  </Button>
                  {!search && (
                    <Button variant="secondary" size="sm" onClick={openFolder}>
                      <FolderOpen /> Open Folder
                    </Button>
                  )}
                </div>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                {filtered.map((ext) => {
                  const enabled = !disabledSet.has(ext.manifest.id)
                  return (
                    <ExtensionCard
                      key={ext.manifest.id}
                      extension={ext}
                      enabled={enabled}
                      onToggle={(next) => void toggleExtension(ext.manifest.id, next)}
                    />
                  )
                })}
              </div>
            )}
          </div>
        </div>
      </ContextArea>
    </div>
  )
}

function ExtensionCard({
  extension,
  enabled,
  onToggle,
}: {
  extension: InstalledExtension
  enabled: boolean
  onToggle: (enabled: boolean) => void
}) {
  const { manifest } = extension
  const [testing, setTesting] = useState(false)

  const runTest = async () => {
    setTesting(true)
    try {
      const res = await commands.extensionTest(manifest.id)
      toast.success(res)
    } catch (e) {
      toast.error("Extension test failed", { description: errorMessage(e) })
    } finally {
      setTesting(false)
    }
  }

  const iconSrc = extension.iconPath ? (isTauri ? convertFileSrc(extension.iconPath) : extension.iconPath) : null
  const bannerSrc = extension.bannerPath ? (isTauri ? convertFileSrc(extension.bannerPath) : extension.bannerPath) : null
  const accent = manifest.accentColor ?? "#e0b45c"

  // Derive author avatar and GitHub info cleanly
  const authorAvatarSrc = useMemo(() => {
    if (extension.authorAvatarPath) {
      return isTauri ? convertFileSrc(extension.authorAvatarPath) : extension.authorAvatarPath
    }
    if (manifest.authorAvatar?.startsWith("http://") || manifest.authorAvatar?.startsWith("https://")) {
      return manifest.authorAvatar
    }
    if (manifest.authorGithub) {
      const clean = manifest.authorGithub.replace(/^@/, "").trim()
      return `https://github.com/${clean}.png?size=64`
    }
    // Fallback: extract owner from repository URL if it points to github.com
    if (manifest.repository) {
      const match = manifest.repository.match(/github\.com\/([^/]+)/i)
      if (match?.[1]) {
        return `https://github.com/${match[1]}.png?size=64`
      }
    }
    return null
  }, [extension.authorAvatarPath, manifest.authorAvatar, manifest.authorGithub, manifest.repository])

  const authorProfileUrl = useMemo(() => {
    if (manifest.authorUrl) return manifest.authorUrl
    if (manifest.authorGithub) {
      const clean = manifest.authorGithub.replace(/^@/, "").trim()
      return `https://github.com/${clean}`
    }
    if (manifest.repository) {
      const match = manifest.repository.match(/github\.com\/([^/]+)/i)
      if (match?.[1]) {
        return `https://github.com/${match[1]}`
      }
    }
    return null
  }, [manifest.authorUrl, manifest.authorGithub, manifest.repository])

  return (
    <div
      className={cn(
        "group relative flex flex-col justify-between overflow-hidden rounded-2xl border border-white/[0.08] transition-all",
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
          <div className="rounded-full bg-black/40 p-0.5 backdrop-blur-md">
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
          <div className="flex items-center gap-1.5">
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
            {authorProfileUrl ? (
              <button
                type="button"
                onClick={() => void openExternalUrl(authorProfileUrl)}
                className="group/author inline-flex items-center gap-1.5 rounded-full hover:text-fg-1 transition-colors"
              >
                {authorAvatarSrc && (
                  <img
                    src={authorAvatarSrc}
                    alt=""
                    className="size-4 rounded-full object-cover ring-1 ring-white/10"
                    onError={(e) => {
                      // Hide if image fails to load (e.g. offline or private)
                      ;(e.target as HTMLElement).style.display = "none"
                    }}
                  />
                )}
                <span className="font-medium underline decoration-white/20 underline-offset-2 group-hover/author:decoration-white/60">
                  {manifest.author}
                </span>
              </button>
            ) : (
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
            )}
          </div>
        </div>

        <p className="mt-3 text-sm leading-relaxed text-fg-3 line-clamp-2">{manifest.description}</p>
      </div>

      {/* Footer Controls */}
      <div className="flex items-center justify-between border-t border-white/[0.06] bg-black/10 px-5 py-3">
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
        <div className="flex flex-wrap gap-1">
          {manifest.capabilities.map((cap) => (
            <span
              key={cap}
              className="rounded border border-white/[0.08] bg-white/[0.03] px-1.5 py-0.5 text-[10px] text-fg-3"
            >
              {cap}
            </span>
          ))}
        </div>
      </div>
    </div>
  )
}

