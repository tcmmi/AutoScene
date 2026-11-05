import type * as React from "react"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { DownloadSimple, Info, Keyboard, SlidersHorizontal, Users } from "@/components/icons"
import { DiscordMark } from "@/components/brand/DiscordMark"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuRadioGroup, ContextMenuRadioItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { TopBar } from "@/components/ui/topbar"
import { SETTINGS_SECTIONS, type SettingsSection, isAppSection, useAppActions } from "@/lib/actions"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { AboutSection } from "./sections/AboutSection"
import { ContributorsSection } from "./sections/ContributorsSection"
import { DiscordSection } from "./sections/DiscordSection"
import { GeneralSection } from "./sections/GeneralSection"
import { ShortcutsSection } from "./sections/ShortcutsSection"
import { UpdatesSection } from "./sections/UpdatesSection"
import { sectionLabel } from "./sections"

const META: Record<SettingsSection, { icon: React.ReactNode; render: () => React.ReactNode }> = {
  general: { icon: <SlidersHorizontal />, render: () => <GeneralSection /> },
  // White, and black on the gold of the open section: colours Discord allows.
  discord: {
    icon: <DiscordMark className="text-white group-data-[status=active]:text-black" />,
    render: () => <DiscordSection />,
  },
  shortcuts: { icon: <Keyboard />, render: () => <ShortcutsSection /> },
  updates: { icon: <DownloadSimple />, render: () => <UpdatesSection /> },
  about: { icon: <Info />, render: () => <AboutSection /> },
  contributors: { icon: <Users />, render: () => <ContributorsSection /> },
}

export function SettingsPage() {
  const { t } = useTranslation("settings")
  const params = useParams({ from: "/settings/$section" })
  // An old link to plugins went to Plugins before this page was shown.
  const section = isAppSection(params.section) ? params.section : "general"
  const navigate = useNavigate()
  const { checkUpdates } = useAppActions()
  const menu = (
    <>
      <ContextMenuRadioGroup
        value={section}
        onValueChange={(s) => void navigate({ to: "/settings/$section", params: { section: s as SettingsSection } })}
      >
        {SETTINGS_SECTIONS.map((s) => (
          <ContextMenuRadioItem key={s} value={s}>
            {sectionLabel(s)}
          </ContextMenuRadioItem>
        ))}
      </ContextMenuRadioGroup>
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={checkUpdates}>
        <DownloadSimple /> {t("updates.check")}
      </ContextMenuItem>
    </>
  )
  return (
    <div className="flex h-full flex-col">
      <TopBar title={t("page.title")} />
      <ContextArea kind="page" menu={menu}>
        <div className="flex min-h-0 flex-1">
          <nav aria-label={t("page.title")} className="w-60 shrink-0 border-r border-white/[0.06] px-3 pt-6">
            <h1 className="display mb-5 break-words px-3 text-3xl text-fg-1">{t("page.title")}</h1>
            <div className="flex flex-col gap-1">
              {SETTINGS_SECTIONS.map((s) => (
                <NavLink key={s} section={s} current={section} icon={META[s].icon}>
                  {sectionLabel(s)}
                </NavLink>
              ))}
            </div>
          </nav>
          <div className="min-w-0 flex-1 overflow-y-auto">
            <div key={section} className="mx-auto max-w-3xl animate-rise-in px-10 pt-8 pb-14">
              <h2 className="display mb-8 text-5xl text-fg-1">{sectionLabel(section)}</h2>
              {META[section].render()}
            </div>
          </div>
        </div>
      </ContextArea>
    </div>
  )
}

function NavLink({
  section,
  current,
  icon,
  children,
}: {
  section: SettingsSection
  current: string
  icon: React.ReactNode
  children: React.ReactNode
}) {
  const here = section === current
  return (
    <Link
      to="/settings/$section"
      params={{ section }}
      className={cn(
        "group flex h-11 min-w-0 items-center gap-3 rounded-xl px-2 text-md font-medium transition-colors",
        here ? "bg-white/[0.09] text-fg-1" : "text-fg-2 hover:bg-white/[0.05] hover:text-fg-1",
      )}
    >
      <span
        className={cn(
          "flex size-7 shrink-0 items-center justify-center rounded-lg transition-colors [&_svg]:size-4",
          here ? "bg-accent text-on-accent" : "bg-white/[0.07] text-fg-2 group-hover:text-fg-1",
        )}
      >
        {icon}
      </span>
      {children}
    </Link>
  )
}
