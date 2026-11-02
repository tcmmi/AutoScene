import { useState } from "react"
import { CheckCircle, ShieldCheck, ShieldWarning, Warning, X } from "@/components/icons"
import { toast } from "sonner"
import type { AspInspectionResult, InstalledExtension } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"

interface PluginInstallModalProps {
  inspection: AspInspectionResult
  filePath: string
  onClose: () => void
  onSuccess: (installed: InstalledExtension) => void
}

export function PluginInstallModal({
  inspection,
  filePath,
  onClose,
  onSuccess,
}: PluginInstallModalProps) {
  const [installing, setInstalling] = useState(false)
  const [allowUnsigned, setAllowUnsigned] = useState(false)
  const { manifest, signature } = inspection

  const isOfficial = signature.status === "official"
  const isCommunity = signature.status === "communitySigned"
  const isUnsigned = signature.status === "unsigned"
  const isTampered = signature.status === "tampered"

  const handleInstall = async () => {
    setInstalling(true)
    try {
      const installed = await commands.pluginInstallAsp(filePath, allowUnsigned)
      await queryClient.invalidateQueries({ queryKey: qk.plugins })
      toast.success(`Plugin "${manifest.name}" installed successfully`)
      onSuccess(installed)
      onClose()
    } catch (e) {
      toast.error("Installation failed", { description: errorMessage(e) })
    } finally {
      setInstalling(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4 animate-in fade-in duration-200">
      <div className="relative w-full max-w-lg rounded-2xl border border-white/10 bg-bg-1 shadow-2xl overflow-hidden flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-white/10 px-5 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex size-8 items-center justify-center rounded-lg bg-white/10 text-fg-1 font-semibold text-sm">
              ASP
            </div>
            <div>
              <h2 className="text-sm font-semibold text-fg-1 leading-none">Install AutoScene Plugin</h2>
              <span className="text-xs text-fg-3">Security and integrity review</span>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1 text-fg-3 hover:bg-white/10 hover:text-fg-1 transition-colors"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* Content Body */}
        <div className="p-5 flex flex-col gap-4 text-xs">
          {/* Plugin Summary */}
          <div className="rounded-xl border border-white/5 bg-white/[0.03] p-3.5 flex flex-col gap-2">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h3 className="text-sm font-semibold text-fg-1">{manifest.name}</h3>
                <span className="text-fg-3 font-mono text-[11px]">{manifest.id} · v{manifest.version}</span>
              </div>
              <span className="rounded-md bg-white/10 px-2 py-0.5 text-[11px] font-medium text-fg-2 uppercase">
                {manifest.kind}
              </span>
            </div>
            {manifest.description && (
              <p className="text-fg-2 leading-relaxed">{manifest.description}</p>
            )}
            <div className="text-[11px] text-fg-3 pt-1 border-t border-white/5 flex items-center justify-between">
              <span>Author: {manifest.author}</span>
              <span>{inspection.fileCount} files ({Math.round(inspection.totalUncompressedBytes / 1024)} KB)</span>
            </div>
          </div>

          {/* Cryptographic Signature Card */}
          <div className="flex flex-col gap-1.5">
            <span className="text-[11px] font-medium uppercase tracking-wider text-fg-3">
              Cryptographic Signature
            </span>

            {isOfficial && (
              <div className="flex items-start gap-3 rounded-xl border border-emerald-500/20 bg-emerald-500/10 p-3 text-emerald-400">
                <ShieldCheck className="size-5 shrink-0 mt-0.5" />
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium text-fg-1">Verified Official AutoScene Plugin</span>
                  <span className="text-fg-3">
                    Cryptographically signed by AutoScene official key. Content is unaltered and reviewed.
                  </span>
                </div>
              </div>
            )}

            {isCommunity && (
              <div className="flex items-start gap-3 rounded-xl border border-blue-500/20 bg-blue-500/10 p-3 text-blue-400">
                <CheckCircle className="size-5 shrink-0 mt-0.5" />
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium text-fg-1">Signed by Developer</span>
                  <span className="text-fg-3">
                    {signature.authorIdentity ? `Signed by ${signature.authorIdentity}` : "Authentic developer signature."}
                  </span>
                  {signature.keyFingerprint && (
                    <span className="font-mono text-[10px] text-fg-3 mt-1 truncate max-w-sm">
                      Fingerprint: {signature.keyFingerprint}
                    </span>
                  )}
                </div>
              </div>
            )}

            {isUnsigned && (
              <div className="flex items-start gap-3 rounded-xl border border-amber-500/20 bg-amber-500/10 p-3 text-amber-400">
                <ShieldWarning className="size-5 shrink-0 mt-0.5" />
                <div className="flex flex-col gap-1.5">
                  <span className="font-medium text-amber-200">Unsigned Package</span>
                  <span className="text-fg-3">
                    This plugin does not contain a digital signature. Only install plugins from developers you trust.
                  </span>
                  <div className="flex items-center gap-2.5 mt-1 select-none">
                    <Switch
                      checked={allowUnsigned}
                      onCheckedChange={setAllowUnsigned}
                      aria-label="Allow installing unsigned plugin"
                    />
                    <span className="text-fg-2 font-medium">I understand the risks and want to install anyway</span>
                  </div>
                </div>
              </div>
            )}

            {isTampered && (
              <div className="flex items-start gap-3 rounded-xl border border-red-500/30 bg-red-500/10 p-3 text-red-400">
                <ShieldWarning className="size-5 shrink-0 mt-0.5" />
                <div className="flex flex-col gap-0.5">
                  <span className="font-semibold text-red-300">Security Warning: Tampered or Corrupted</span>
                  <span className="text-fg-2">
                    {signature.errorMessage || "Package verification failed. File contents do not match signature."}
                  </span>
                </div>
              </div>
            )}
          </div>

          {/* Requested Capabilities */}
          {manifest.capabilities.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium uppercase tracking-wider text-fg-3">
                Permissions & Capabilities
              </span>
              <div className="flex flex-wrap gap-1.5">
                {manifest.capabilities.map((cap) => (
                  <span
                    key={cap}
                    className="inline-flex items-center gap-1 rounded-md bg-white/5 border border-white/10 px-2 py-1 text-[11px] font-mono text-fg-2"
                  >
                    <Warning className="size-3 text-fg-3" />
                    {cap}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Footer Actions */}
        <div className="flex items-center justify-end gap-2.5 border-t border-white/10 px-5 py-3.5 bg-black/20">
          <Button variant="ghost" onClick={onClose} disabled={installing}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={handleInstall}
            disabled={installing || isTampered || (isUnsigned && !allowUnsigned)}
          >
            {installing ? "Installing..." : "Install Plugin"}
          </Button>
        </div>
      </div>
    </div>
  )
}
