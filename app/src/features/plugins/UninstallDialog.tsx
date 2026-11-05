import { useState } from "react"
import { toast } from "sonner"
import type { PluginInfo } from "@/bindings"
import { Trash } from "@/components/icons"
import { ConfirmDialog } from "@/components/ui/confirm-dialog"
import { OptionRow } from "@/components/ui/option-row"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"

/**
 * Asks before the plugin `p` goes (open while `p` is set), and whether what
 * it kept goes too. It can't be closed while it runs. `onUninstalled` runs
 * once it has gone, before the list forgets it: the plugin's own screen
 * leaves for Plugins there, so it never says the plugin isn't installed.
 */
export function UninstallDialog({
  p,
  onClose,
  onUninstalled,
}: {
  p: PluginInfo | null
  onClose: () => void
  onUninstalled?: () => Promise<unknown> | void
}) {
  return p && <UninstallForm key={p.id} p={p} onClose={onClose} onUninstalled={onUninstalled} />
}

function UninstallForm({ p, onClose, onUninstalled }: { p: PluginInfo; onClose: () => void; onUninstalled?: () => Promise<unknown> | void }) {
  const [dataToo, setDataToo] = useState(false)
  const uninstall = async () => {
    await commands.pluginUninstall(p.id, dataToo)
    // It has gone either way: a way out that failed leaves the list to say so.
    await Promise.resolve(onUninstalled?.()).catch(() => {})
    queryClient.setQueryData<PluginInfo[]>(qk.plugins, (list) => list?.filter((x) => x.id !== p.id))
    toast(`Uninstalled ${p.name}`)
  }
  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && onClose()}
      title={`Uninstall ${p.name}?`}
      description="Its commands, settings and actions will go."
      answer="Uninstall"
      icon={<Trash />}
      failure="Couldn't uninstall the plugin"
      onConfirm={uninstall}
    >
      <OptionRow
        label="Delete what it kept too"
        description="Remove its saved settings, credentials, caches and logs."
        checked={dataToo}
        onCheckedChange={setDataToo}
      />
    </ConfirmDialog>
  )
}
