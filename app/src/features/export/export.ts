import type { AppSettings, ExportOptions } from "@/bindings"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"

/**
 * The format and options an export started with become the next one's. The
 * export is on its way already: when this can't be saved, the next dialog
 * only opens on the old ones, so it is noted in the log rather than shown.
 */
export function rememberPreset(
  settings: AppSettings | undefined,
  presetId: string,
  exportOptions: ExportOptions | undefined = settings?.exportOptions,
) {
  if (!settings) return
  const sameOptions =
    !!settings.exportOptions?.removeBars === !!exportOptions?.removeBars &&
    !!settings.exportOptions?.matchLoudness === !!exportOptions?.matchLoudness
  if (settings.exportPresetId === presetId && sameOptions) return
  commands
    .settingsUpdate({ ...settings, exportPresetId: presetId, exportOptions: exportOptions ?? settings.exportOptions })
    .then((s) => queryClient.setQueryData(qk.settings, s))
    .catch((e) => console.warn("remembering the export format:", e))
}
