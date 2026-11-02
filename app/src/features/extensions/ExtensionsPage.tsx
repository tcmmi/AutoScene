import { useMemo, useState } from "react"
import {
  ArrowCounterClockwise,
  CheckCircle,
  DownloadSimple,
  FolderOpen,
  MagnifyingGlass,
  Package,
  Play,
  ShieldCheck,
  ShieldWarning,
  Trash,
} from "@/components/icons"
import { toast } from "sonner"
import type { AspInspectionResult, ExtensionKind, InstalledExtension } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { Switch } from "@/components/ui/switch"
import { TopBar } from "@/components/ui/topbar"
import { commands, errorMessage } from "@/lib/ipc"
import { convertFileSrc } from "@tauri-apps/api/core"
import { openExternalUrl } from "@/lib/os"
import { isTauri } from "@/lib/platform"
import { qk, queryClient, useAppInfo, usePlugins, useSettings } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { PluginInstallModal } from "./PluginInstallModal"

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
  const { data: plugins = [], isFetching } = usePlugins()
  const [filter, setFilter] = useState<ExtensionKind | "all">("all")
  const [search, setSearch] = useState("")
  const [inspectTarget, setInspectTarget] = useState<{ inspection: AspInspectionResult; filePath: string } | null>(null)

  const reload = async () => {
    try {
      await queryClient.invalidateQueries({ queryKey: qk.plugins })
      toast.success("Plugins reloaded")
    } catch (e) {
      toast.error("Couldn't reload plugins", { description: errorMessage(e) })
    }
  }

  const openFolder = async () => {
    try {
      await commands.openPluginsDir()
    } catch (e) {
      toast.error("Couldn't open plugins directory", { description: errorMessage(e) })
    }
  }

  const installAspFile = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog")
      const selected = await open({
        multiple: false,
        filters: [{ name: "AutoScene Plugin (*.asp)", extensions: ["asp"] }],
      })
      if (!selected || typeof selected !== "string") return

      toast.info("Inspecting plugin package...")
      const inspection = await commands.pluginInspectAsp(selected)
      setInspectTarget({ inspection, filePath: selected })
    } catch (e) {
      toast.error("Couldn't inspect .asp plugin", { description: errorMessage(e) })
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
      toast.success(enabled ? "Plugin enabled" : "Plugin disabled")
    } catch (e) {
      toast.error("Couldn't update plugin state", { description: errorMessage(e) })
    }
  }

  const uninstallPlugin = async (id: string, name: string) => {
    if (!confirm(`Are you sure you want to uninstall "${name}"?`)) return
    try {
      await commands.pluginUninstall(id)
      await queryClient.invalidateQueries({ queryKey: qk.plugins })
      toast.success(`Uninstalled "${name}"`)
    } catch (e) {
      toast.error("Failed to uninstall plugin", { description: errorMessage(e) })
    }
  }

  const disabledSet = useMemo(() => new Set(settings?.disabledExtensions ?? []), [settings?.disabledExtensions])

  const filtered = useMemo(() => {
    return plugins.filter((ext) => {
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
  }, [plugins, filter, search])

  const menu = (
    <>
      <ContextMenuItem onSelect={installAspFile}>
        <DownloadSimple /> Install .asp Plugin…
      </ContextMenuItem>
      <ContextMenuItem onSelect={reload}>
        <ArrowCounterClockwise /> Reload plugins
      </ContextMenuItem>
      <ContextMenuItem onSelect={openFolder}>
        <FolderOpen /> Open plugins folder
      </ContextMenuItem>
    </>
  )

  return (
    <div className="flex h-full flex-col">
      <TopBar title="Plugins">
        <div className="flex items-center gap-2">
          <Button size="sm" variant="secondary" onClick={installAspFile}>
            <DownloadSimple /> Install .asp…
          </Button>
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
                <h1 className="display text-3xl font-semibold text-fg-1">Plugins</h1>
                <p className="mt-1.5 max-w-2xl text-md text-fg-2">
                  Enhance AutoScene with verified .asp packages: out-of-process exporters, notification webhooks, automated tagging, and media
                  post-processing pipelines.
                </p>
                {info && (
                  <div className="mt-3 flex items-center gap-2 text-xs text-fg-3">
                    <span className="font-mono text-fg-4">{info.extensionsDir}</span>
                  </div>
                )}
              </div>
              <div className="flex shrink-0 gap-3">
                <Button variant="secondary" onClick={installAspFile}>
                  <DownloadSimple />
                  Install .asp
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
                  placeholder="Search plugins…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="h-9 w-64 rounded-xl border border-white/[0.08] bg-white/[0.03] pl-9 pr-3 text-xs text-fg-1 placeholder:text-fg-4 focus:border-white/20 focus:outline-none"
                />
              </div>
            </div>

            {/* Grid */}
            {filtered.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-3xl border border-dashed border-white/10 p-16 text-center">
                <div className="flex size-14 items-center justify-center rounded-2xl bg-white/[0.05] text-fg-4 mb-4">
                  <Package className="size-8" />
                </div>
                <h3 className="text-base font-medium text-fg-2">No plugins found</h3>
                <p className="mt-1 text-sm text-fg-4 max-w-sm">
                  {search
                    ? "Try a different search term or category filter."
                    : "Install .asp plugin packages or copy plugin folders into the plugins folder."}
                </p>
                <div className="mt-6 flex items-center gap-3">
                  <Button variant="secondary" onClick={installAspFile}>
                    <DownloadSimple /> Install .asp
                  </Button>
                  <Button variant="ghost" onClick={openFolder}>
                    <FolderOpen /> Open folder
                  </Button>
                </div>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
                {filtered.map((ext) => (
                  <PluginCard
                    key={ext.manifest.id}
                    extension={ext}
                    enabled={!disabledSet.has(ext.manifest.id)}
                    onToggle={(enabled) => toggleExtension(ext.manifest.id, enabled)}
                    onUninstall={() => uninstallPlugin(ext.manifest.id, ext.manifest.name)}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      </ContextArea>

      {inspectTarget && (
        <PluginInstallModal
          inspection={inspectTarget.inspection}
          filePath={inspectTarget.filePath}
          onClose={() => setInspectTarget(null)}
          onSuccess={() => setInspectTarget(null)}
        />
      )}
    </div>
  )
}

function PluginCard({
  extension,
  enabled,
  onToggle,
  onUninstall,
}: {
  extension: InstalledExtension
  enabled: boolean
  onToggle: (enabled: boolean) => void
  onUninstall: () => void
}) {
  const { manifest } = extension
  const [testing, setTesting] = useState(false)

  const runTest = async () => {
    setTesting(true)
    try {
      const out = await commands.pluginTest(manifest.id)
      toast.success(`Test output: ${out}`)
    } catch (e) {
      toast.error("Plugin test failed", { description: errorMessage(e) })
    } finally {
      setTesting(false)
    }
  }

  const iconSrc = useMemo(() => {
    if (!extension.iconPath) return null
    return isTauri ? convertFileSrc(extension.iconPath) : extension.iconPath
  }, [extension.iconPath])

  const bannerSrc = useMemo(() => {
    if (!extension.bannerPath) return null
    return isTauri ? convertFileSrc(extension.bannerPath) : extension.bannerPath
  }, [extension.bannerPath])

  const authorAvatarSrc = useMemo(() => {
    if (extension.authorAvatarPath) {
      return isTauri ? convertFileSrc(extension.authorAvatarPath) : extension.authorAvatarPath
    }
    return manifest.authorAvatar ?? null
  }, [extension.authorAvatarPath, manifest.authorAvatar])

  const authorProfileUrl = manifest.authorGithub ?? manifest.authorUrl ?? null

  const signatureBadge = useMemo(() => {
    if (extension.signatureStatus === "official") {
      return (
        <span className="inline-flex items-center gap-1 rounded bg-emerald-500/10 border border-emerald-500/20 px-1.5 py-0.5 text-[10px] font-medium text-emerald-400">
          <ShieldCheck className="size-3" /> Official
        </span>
      )
    }
    if (extension.signatureStatus === "communitySigned") {
      return (
        <span className="inline-flex items-center gap-1 rounded bg-blue-500/10 border border-blue-500/20 px-1.5 py-0.5 text-[10px] font-medium text-blue-400">
          <CheckCircle className="size-3" /> Signed
        </span>
      )
    }
    return (
      <span className="inline-flex items-center gap-1 rounded bg-amber-500/10 border border-amber-500/20 px-1.5 py-0.5 text-[10px] font-medium text-amber-400">
        <ShieldWarning className="size-3" /> Unsigned
      </span>
    )
  }, [extension.signatureStatus])

  return (
    <div
      className={cn(
        "group relative flex flex-col justify-between overflow-hidden rounded-2xl border transition-all duration-200",
        enabled
          ? "border-white/[0.08] bg-white/[0.02] hover:border-white/[0.15] hover:bg-white/[0.04]"
          : "border-white/[0.04] bg-white/[0.01] opacity-60",
      )}
    >
      {bannerSrc && (
        <div className="relative h-24 w-full overflow-hidden border-b border-white/[0.06] bg-black/20">
          <img
            src={bannerSrc}
            alt=""
            className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-bg-1 via-transparent to-transparent" />
        </div>
      )}

      <div className="flex-1 p-5">
        <div className="mb-4 flex items-start justify-between gap-3">
          <div className="flex items-center gap-3">
            {iconSrc ? (
              <img
                src={iconSrc}
                alt=""
                className="size-11 rounded-xl object-contain bg-white/[0.05] p-1.5 ring-1 ring-white/10"
              />
            ) : (
              <div
                className="flex size-11 items-center justify-center rounded-xl font-bold text-base ring-1 ring-white/10"
                style={{
                  backgroundColor: manifest.accentColor ? `${manifest.accentColor}25` : "rgba(255,255,255,0.05)",
                  color: manifest.accentColor ?? "var(--fg-1)",
                }}
              >
                {manifest.name.slice(0, 1).toUpperCase()}
              </div>
            )}
            <div className="flex flex-col gap-0.5">
              <span className="font-mono text-[10px] text-fg-4">{manifest.id}</span>
              <div className="flex items-center gap-1.5">
                <span className="rounded bg-white/[0.06] px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-fg-3">
                  {manifest.kind}
                </span>
                {signatureBadge}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Switch checked={enabled} onCheckedChange={onToggle} aria-label={`Toggle ${manifest.name}`} />
            <Button
              size="icon-sm"
              variant="ghost"
              onClick={onUninstall}
              aria-label="Uninstall plugin"
              className="text-fg-4 hover:text-red-400"
            >
              <Trash className="size-3.5" />
            </Button>
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
