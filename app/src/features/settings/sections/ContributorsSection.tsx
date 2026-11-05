import { useTranslation } from "react-i18next"
import { ArrowSquareOut, GithubLogo } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { openExternalUrl } from "@/lib/os"
import { SettingRow, SettingsGroup } from "../primitives"
import CONTRIBUTORS_JSON from "./contributors.json"

export interface Contributor {
  name: string
  handle: string
  avatar: string
  role: string
  profileUrl: string
}

const CONTRIBUTORS: Contributor[] = CONTRIBUTORS_JSON

const REPO_URL = "https://github.com/tcmmi/AutoScene"

export function ContributorsSection() {
  const { t } = useTranslation("settings")
  return (
    <>
      <div className="relative mb-10 flex items-center justify-between gap-6 overflow-hidden rounded-3xl border border-white/[0.08] bg-[radial-gradient(120%_140%_at_0%_0%,rgb(224_180_92/0.16),transparent_55%),rgb(255_255_255/0.03)] p-7">
        <div className="min-w-0">
          <h2 className="display text-3xl text-fg-1">{t("contributors.banner.title")}</h2>
          <p className="mt-1 text-md text-fg-2">{t("contributors.banner.description")}</p>
        </div>
        <Button size="sm" onClick={() => void openExternalUrl(REPO_URL)}>
          <GithubLogo /> {t("contributors.banner.action")}
        </Button>
      </div>

      <SettingsGroup title={t("contributors.title")} description={t("contributors.description")}>
        {CONTRIBUTORS.map((c) => (
          <SettingRow
            key={c.handle}
            label={
              <div className="flex items-center gap-3">
                <img
                  src={c.avatar}
                  alt={c.name}
                  className="size-8 shrink-0 rounded-full border border-white/[0.1] bg-white/[0.05] object-cover"
                  loading="lazy"
                  // Offline: the empty circle stays, not a broken picture.
                  onError={(e) => (e.currentTarget.style.visibility = "hidden")}
                />
                <div>
                  <span className="font-medium text-fg-1">{c.name}</span>
                  <span className="ml-2 font-mono text-xs text-fg-3">@{c.handle}</span>
                </div>
              </div>
            }
            description={<span className="block mt-1 text-fg-2">{c.role}</span>}
            className="min-h-16 py-3"
          >
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void openExternalUrl(c.profileUrl)}
              aria-label={t("contributors.open", { name: c.name })}
            >
              <ArrowSquareOut /> GitHub
            </Button>
          </SettingRow>
        ))}
      </SettingsGroup>
    </>
  )
}

