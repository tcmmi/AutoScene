import { useState } from "react"
import { toast } from "sonner"
import type { PluginInfo } from "@/bindings"
import { CircleNotch, Trash } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Switch } from "@/components/ui/switch"
import { SettingRow } from "@/features/settings/primitives"
import { commands, errorMessage } from "@/lib/ipc"
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
  const [busy, setBusy] = useState(false)
  return (
    <Dialog open={p != null} onOpenChange={(open) => !open && !busy && onClose()}>
      {p && <UninstallForm key={p.id} p={p} busy={busy} setBusy={setBusy} onDone={onClose} onUninstalled={onUninstalled} />}
    </Dialog>
  )
}

function UninstallForm({
  p,
  busy,
  setBusy,
  onDone,
  onUninstalled,
}: {
  p: PluginInfo
  busy: boolean
  setBusy: (b: boolean) => void
  onDone: () => void
  onUninstalled?: () => Promise<unknown> | void
}) {
  const [dataToo, setDataToo] = useState(false)
  const uninstall = async () => {
    setBusy(true)
    try {
      await commands.pluginUninstall(p.id, dataToo)
    } catch (e) {
      toast.error("Couldn't uninstall the plugin", { description: errorMessage(e) })
      setBusy(false)
      return
    }
    // It has gone either way: a way out that failed leaves the list to say so.
    await Promise.resolve(onUninstalled?.()).catch(() => {})
    queryClient.setQueryData<PluginInfo[]>(qk.plugins, (list) => list?.filter((x) => x.id !== p.id))
    setBusy(false)
    onDone()
    toast(`Uninstalled ${p.name}`)
  }

  return (
    <DialogContent title={`Uninstall ${p.name}?`} description="Its commands, settings and actions will go.">
      <div className="my-4">
        <SettingRow label="Delete what it kept too" description="Remove its saved settings, credentials, caches and logs.">
          <Switch aria-label="Delete what it kept too" checked={dataToo} disabled={busy} onCheckedChange={setDataToo} />
        </SettingRow>
      </div>
      <DialogFooter>
        <Button variant="ghost" disabled={busy} onClick={onDone}>
          Cancel
        </Button>
        <Button variant="danger" disabled={busy} onClick={() => void uninstall()}>
          {busy ? <CircleNotch className="animate-spin" /> : <Trash />} Uninstall
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}
