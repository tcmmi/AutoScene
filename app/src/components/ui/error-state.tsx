import type * as React from "react"
import { ArrowCounterClockwise, FileText, Warning } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { commands, errorMessage, isAppError } from "@/lib/ipc"

/**
 * A view whose data couldn't be loaded: what went wrong in plain words, and a
 * way to try again. Failures of the app itself (its data, its files) also
 * offer the logs, where their technical side is written. `retrying` holds
 * Try again while it runs; `icon` says what failed where the warning would
 * say less; `actions` are other ways out, between the two buttons.
 */
export function ErrorState({
  error,
  title = "This couldn't be loaded",
  onRetry,
  retrying,
  icon = <Warning />,
  actions,
  className,
}: {
  error: unknown
  title?: string
  onRetry?: () => void
  retrying?: boolean
  icon?: React.ReactNode
  actions?: React.ReactNode
  className?: string
}) {
  const ownFault = !isAppError(error) || error.code === "internal" || error.code === "io" || error.code === "media"
  return (
    <EmptyState
      tone="danger"
      icon={icon}
      title={title}
      description={isAppError(error) ? error.message : "Something went wrong while showing this."}
      className={className}
      actions={
        <>
          {onRetry && (
            <Button busy={retrying} onClick={onRetry}>
              <ArrowCounterClockwise /> Try again
            </Button>
          )}
          {actions}
          {ownFault && (
            <Button variant="ghost" onClick={() => void commands.openLogsDir().catch(() => {})}>
              <FileText /> Open logs
            </Button>
          )}
        </>
      }
    >
      {/* A bug in the view itself: its own words, small and copyable, for a report. */}
      {!isAppError(error) && (
        <p data-selectable className="timecode mt-6 max-w-lg text-xs break-words text-fg-3">
          {errorMessage(error)}
        </p>
      )}
    </EmptyState>
  )
}
