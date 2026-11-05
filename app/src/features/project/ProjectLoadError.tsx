import { Link } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { Compass } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { ErrorState } from "@/components/ui/error-state"
import { errorMessage, isAppError } from "@/lib/ipc"

/**
 * A project view whose data couldn't be loaded. A project that is gone
 * (removed, or its folder moved) says so and leads back to the library;
 * anything else can be tried again.
 */
export function ProjectLoadError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const { t } = useTranslation("project")
  if (isAppError(error) && error.code === "not_found") {
    return (
      <EmptyState
        icon={<Compass />}
        title={t("loadError.goneTitle")}
        description={errorMessage(error)}
        className="h-full"
        actions={
          <Button asChild>
            <Link to="/">{t("loadError.backToLibrary")}</Link>
          </Button>
        }
      />
    )
  }
  return <ErrorState error={error} className="h-full" onRetry={onRetry} />
}
