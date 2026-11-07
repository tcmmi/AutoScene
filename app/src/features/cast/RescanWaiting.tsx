import { Link } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { Scan } from "@/components/icons"
import { Banner } from "@/components/ui/banner"
import { Button } from "@/components/ui/button"
import { useRescanResults } from "@/lib/queries"

/** Gold while Rescan's suggestions wait for an answer: they are the person's to take. */
export function RescanWaiting({ pid, className }: { pid: string; className?: string }) {
  const { t } = useTranslation("cast")
  const { data } = useRescanResults(pid)
  const count = (data?.faces.length ?? 0) + (data?.pairs.length ?? 0)
  if (count === 0) return null
  return (
    <Banner
      tone="accent"
      icon={<Scan weight="fill" />}
      title={t("rescan.waiting.title", { count })}
      description={t("rescan.waiting.about")}
      actions={
        <Button variant="secondary" asChild>
          <Link to="/p/$pid/cast/rescan" params={{ pid }}>
            {t("rescan.waiting.open")}
          </Link>
        </Button>
      }
      className={className}
    />
  )
}
