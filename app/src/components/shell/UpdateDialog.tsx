import { useTranslation } from "react-i18next"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { formatBytes } from "@/lib/format"
import { noticeOpen, useFastScanNotice } from "@/state/fast-scan-notice"
import { useUpdates } from "@/state/updates"

export function UpdateDialog() {
  const { t } = useTranslation("shell")
  const u = useUpdates()
  // Two modals never stack: the update waits while the first-start notice is open.
  const notice = useFastScanNotice(noticeOpen)
  const status = u.status
  if (!status) return null

  const busy = u.phase === "downloading" || u.phase === "installing"
  const fraction = u.total ? u.downloaded / u.total : null

  return (
    <Dialog open={u.dialogOpen && !notice} onOpenChange={(o) => !busy && u.setDialogOpen(o)}>
      <DialogContent
        hideClose={busy}
        title={
          <span className="flex items-center gap-2">
            {t("update.available", { version: status.version ?? "" })}
            {status.simulated && <Badge tone="outline">{t("update.simulated")}</Badge>}
          </span>
        }
        description={
          status.date
            ? t("update.youHaveDated", { current: status.currentVersion, date: status.date })
            : t("update.youHave", { current: status.currentVersion })
        }
      >
        {status.notes && (
          <div
            data-selectable
            className="max-h-56 overflow-y-auto rounded-md border border-hairline bg-inset p-3 text-sm whitespace-pre-wrap text-fg-2"
          >
            {status.notes}
          </div>
        )}

        {(busy || u.phase === "ready") && (
          <div className="mt-4 space-y-2">
            <Progress value={u.phase === "installing" || u.phase === "ready" ? 1 : fraction} />
            <p className="text-xs text-fg-3 tabular">
              {u.phase === "downloading" &&
                (u.total
                  ? t("update.downloading", { done: formatBytes(u.downloaded), total: formatBytes(u.total) })
                  : formatBytes(u.downloaded))}
              {u.phase === "installing" && t("update.installing")}
              {u.phase === "ready" && t("update.ready")}
            </p>
          </div>
        )}

        {u.phase === "error" && u.error && <p className="mt-4 text-sm text-danger">{u.error}</p>}

        <DialogFooter>
          {u.phase === "ready" ? (
            <Button variant="primary" onClick={() => void u.relaunch()}>
              {t("update.restart")}
            </Button>
          ) : (
            <>
              <Button variant="ghost" disabled={busy} onClick={u.skipVersion}>
                {t("update.skip")}
              </Button>
              <Button disabled={busy} onClick={u.remindLater}>
                {t("update.later")}
              </Button>
              <Button variant="primary" disabled={busy} onClick={() => void u.install()}>
                {busy ? t("update.updating") : t("update.install")}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
