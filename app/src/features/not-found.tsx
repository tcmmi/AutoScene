import { Link } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { Compass } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { Page } from "@/components/ui/page"
import { TopBar } from "@/components/ui/topbar"

/** Unknown route (a stale link, a removed project). */
export function NotFound() {
  const { t } = useTranslation("shell")
  return (
    <Page topbar={<TopBar />}>
      <EmptyState
        icon={<Compass />}
        title={t("notFound.title")}
        description={t("notFound.description")}
        className="h-full"
        actions={
          <Button asChild>
            <Link to="/">{t("notFound.back")}</Link>
          </Button>
        }
      />
    </Page>
  )
}
