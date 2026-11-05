import type * as React from "react"
import { useTranslation } from "react-i18next"
import { ArrowCounterClockwise, CaretDown, FunnelSimple, Warning } from "@/components/icons"
import type { MediaItem, PackFilter, PackOrder, PackSpec, PluginInfo, ShotSize } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ProgressRing } from "@/components/ui/progress"
import { Segmented } from "@/components/ui/segmented"
import { ToggleChip } from "@/components/ui/toggle-chip"
import { Tooltip, TruncatedText } from "@/components/ui/tooltip"
import { PluginIcon } from "@/features/plugins/PluginIcon"
import { mediaTitle } from "@/lib/media"
import { cn } from "@/lib/utils"
import { DEFAULT_SPEC, HANDLES, LENGTHS, LIMITS, SIZES, checkingText, episodesOn, filtersOn, isDefault, lengthLabel, lengthOf, pluginFiltersOf, withPluginFilter } from "./spec"

/**
 * The pack's order, its filters in one menu and the handles, in a bar that
 * stays under the top bar while the shots scroll. The menu stays open while
 * filters are chosen, so several take one visit; a plugin's filters stay
 * chips of their own, because each can have something to say (a ring, a
 * warning mark).
 */
export function PackFilters({
  spec,
  media,
  faceless,
  filters = NO_FILTERS,
  plugins = NO_PLUGINS,
  onChange,
}: {
  spec: PackSpec
  media: MediaItem[]
  /** The pack has shots where the character's face doesn't show. */
  faceless: boolean
  /** The plugins' shot filters the pack can show: every one of a plugin that is on, and those `spec` turns on. */
  filters?: PackFilter[]
  /** The installed plugins: their marks. */
  plugins?: PluginInfo[]
  onChange: (spec: PackSpec) => void
}) {
  const { t } = useTranslation("pack")
  const set = (patch: Partial<PackSpec>) => onChange({ ...spec, ...patch })
  const toggleSize = (s: ShotSize) =>
    set({ sizes: spec.sizes.includes(s) ? spec.sizes.filter((x) => x !== s) : SIZES.map((x) => x.value).filter((x) => x === s || spec.sizes.includes(x)) })
  const length = lengthOf(spec)
  const narrowed = episodesOn(spec, media)
  const episodes = narrowed ? spec.media.filter((id) => media.some((m) => m.id === id)).length : media.length
  const count = filtersOn(spec, media)

  return (
    <div
      role="group"
      aria-label={t("filters.title")}
      className="sticky top-[var(--topbar-h)] z-10 flex flex-wrap items-center gap-2 border-b border-hairline bg-canvas/80 px-10 py-3 backdrop-blur-2xl backdrop-saturate-150"
    >
      <Segmented<PackOrder>
        aria-label={t("filters.order.aria")}
        value={spec.order}
        onChange={(order) => set({ order })}
        options={[
          { value: "episodes", label: t("filters.order.episodes") },
          { value: "best", label: t("filters.order.best") },
        ]}
      />
      <Divider />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ToggleChip pressed={count > 0} aria-haspopup="menu" aria-pressed={undefined} aria-label={count > 0 ? t("filters.titleOn", { count }) : t("filters.title")}>
            <FunnelSimple className="size-4" /> {t("filters.title")}
            {count > 0 && (
              <span className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-2xs font-semibold tabular-nums text-on-accent">
                {count}
              </span>
            )}
            <CaretDown className="size-3.5 opacity-70" />
          </ToggleChip>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-[300px]">
          {/* No header of its own: the names say it, and the menu fits the window's smallest height (1024x640) at rest. */}
          <div role="group" aria-label={t("filters.sizes")}>
            {SIZES.map((s) => (
              <DropdownMenuCheckboxItem key={s.value} checked={spec.sizes.includes(s.value)} onCheckedChange={() => toggleSize(s.value)}>
                {t(`size.${s.key}.label`)}
              </DropdownMenuCheckboxItem>
            ))}
          </div>
          <DropdownMenuSeparator />
          <Choice label={t("filters.length")} value={length.label} active={spec.minSec != null || spec.maxSec != null}>
            <DropdownMenuRadioGroup
              value={length.id ?? ""}
              onValueChange={(id) => {
                const l = LENGTHS.find((x) => x.id === id)
                if (l) set({ minSec: l.min, maxSec: l.max })
              }}
            >
              {LENGTHS.map((l) => (
                <DropdownMenuRadioItem key={l.id} value={l.id} onSelect={stayOpen}>
                  {lengthLabel(l.min, l.max)}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </Choice>
          {media.length > 1 && (
            <Choice label={t("filters.episodes")} value={narrowed ? t("filters.episodesOf", { n: episodes, total: media.length }) : t("filters.all")} active={narrowed}>
              <DropdownMenuCheckboxItem checked={!narrowed} disabled={!narrowed} onCheckedChange={() => set({ media: [] })}>
                {t("filters.allEpisodes")}
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              {/* A long series scrolls here, so "All episodes" stays in reach above. */}
              <div className="max-h-[min(420px,60vh)] overflow-y-auto">
                {media.map((m) => {
                  const picked = !narrowed || spec.media.includes(m.id)
                  const only = picked && episodes === 1
                  return (
                    <DropdownMenuCheckboxItem
                      key={m.id}
                      checked={picked}
                      // One episode at least: none would mean all again.
                      disabled={only}
                      onCheckedChange={(checked) => {
                        const ids = narrowed ? spec.media : media.map((x) => x.id)
                        const next = checked ? [...ids, m.id] : ids.filter((id) => id !== m.id)
                        const all = media.every((x) => next.includes(x.id))
                        set({ media: all ? [] : media.map((x) => x.id).filter((id) => next.includes(id)) })
                      }}
                    >
                      {/* Cut at the list's width; the rest shows in a tooltip, only when it is cut. */}
                      <TruncatedText>{mediaTitle(m)}</TruncatedText>
                    </DropdownMenuCheckboxItem>
                  )
                })}
              </div>
            </Choice>
          )}
          <Choice label={t("filters.limit")} value={spec.limit == null ? t("filters.noLimit") : t("filters.best", { n: spec.limit })} active={spec.limit != null}>
            <DropdownMenuRadioGroup
              value={String(spec.limit)}
              onValueChange={(v) => set({ limit: v === "null" ? null : Number(v) })}
            >
              {LIMITS.map((n) => (
                <DropdownMenuRadioItem key={String(n)} value={String(n)} onSelect={stayOpen}>
                  {n == null ? t("filters.noLimit") : t("filters.theBest", { n })}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </Choice>
          <DropdownMenuSeparator />
          <DropdownMenuCheckboxItem checked={spec.solo} onCheckedChange={(solo) => set({ solo })}>
            {t("filters.solo")}
          </DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem checked={spec.skipTiny} onCheckedChange={(skipTiny) => set({ skipTiny })}>
            {t("filters.skipTiny")}
          </DropdownMenuCheckboxItem>
          {(faceless || spec.skipFaceless) && (
            // Its hint is a line of its own, not a tooltip: a tooltip would take the first Esc from the menu.
            <DropdownMenuCheckboxItem
              hint={t("filters.withFaceHint")}
              checked={spec.skipFaceless ?? false}
              onCheckedChange={(skipFaceless) => set({ skipFaceless })}
            >
              {t("filters.withFace")}
            </DropdownMenuCheckboxItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
      <Menu label={spec.handles === 0 ? t("filters.noHandles") : t("filters.handles", { n: spec.handles })} active={spec.handles > 0}>
        <DropdownMenuRadioGroup value={String(spec.handles)} onValueChange={(v) => set({ handles: Number(v) })}>
          {HANDLES.map((n) => (
            <DropdownMenuRadioItem key={n} value={String(n)}>
              {n === 0 ? t("filters.noHandles") : t("filters.framesAround", { count: n })}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </Menu>
      {filters.length > 0 && <Divider />}
      {filters.map((f) => (
        <PluginFilterChip
          key={f.key}
          filter={f}
          on={pluginFiltersOf(spec).includes(f.key)}
          icon={plugins.find((p) => p.id === pluginOf(f.key))?.icon}
          onChange={(on) => onChange(withPluginFilter(spec, f.key, on))}
        />
      ))}
      {!isDefault(spec) && (
        <Button size="sm" variant="ghost" className="ml-auto" onClick={() => onChange(DEFAULT_SPEC)}>
          <ArrowCounterClockwise /> {t("filters.reset")}
        </Button>
      )}
    </div>
  )
}

/** A radio entry's choice shows at once and the menu stays: the next filter is one step away. */
const stayOpen = (e: Event) => e.preventDefault()

/** A filter with a list of its own: its name, what is chosen, and the list one step further. */
function Choice({ label, value, active, children }: { label: string; value: string; active: boolean; children: React.ReactNode }) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger className="pl-9">
        {label}
        <span className={cn("ml-auto text-sm", active ? "text-accent" : "text-fg-3")}>{value}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="min-w-[220px] max-w-[360px]">{children}</DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

const NO_FILTERS: PackFilter[] = []
const NO_PLUGINS: PluginInfo[] = []

/** The plugin a filter key (`<pluginId>/<filterId>`) belongs to. */
const pluginOf = (key: string) => key.slice(0, Math.max(0, key.indexOf("/")))

/**
 * A plugin's shot filter as a chip with its plugin's mark. Turned on, it
 * leaves out the shots its plugin says no to; while it checks them, a
 * ring fills; once its check failed, a warning mark says so (the footer
 * offers Try again). Its plugin is off: the chip is dimmed and says so, the
 * pack ignores it, and it can be turned off. No plugin has it now: the same,
 * and turning it off is all there is to do.
 */
function PluginFilterChip({
  filter: f,
  on,
  icon,
  onChange,
}: {
  filter: PackFilter
  on: boolean
  icon: string | null | undefined
  onChange: (on: boolean) => void
}) {
  const { t } = useTranslation("pack")
  const gone = f.state === "gone"
  const off = f.state === "off"
  const checking = on && f.state === "checking"
  const failed = on && f.state === "failed" && f.checked < f.total
  const label = gone
    ? t("filters.plugin.gone")
    : off
      ? t("filters.plugin.off", { plugin: f.plugin })
      : t("filters.plugin.says", { plugin: f.plugin, what: f.description ?? f.title })
  const hint = checking
    ? checkingText(f.checked, f.total)
    : failed
      ? t("check.failed", { count: f.total - f.checked })
      : undefined
  return (
    <Tooltip label={label} hint={hint}>
      <ToggleChip pressed={on} onClick={() => onChange(!on)} className={cn((off || gone) && "opacity-60")}>
        <PluginIcon src={icon} className="size-4" />
        {f.title}
        {checking && (
          <ProgressRing
            aria-label={t("filters.plugin.checkingAria", { title: f.title })}
            className="size-3.5"
            value={f.checked > 0 && f.total > 0 ? f.checked / f.total : null}
          />
        )}
        {failed && <Warning role="img" aria-label={t("filters.plugin.failedAria", { title: f.title })} className="size-3.5 text-danger" />}
        {off && <span className="text-xs font-normal text-fg-3">{t("filters.plugin.pluginOff")}</span>}
      </ToggleChip>
    </Tooltip>
  )
}

function Divider() {
  return <span aria-hidden className="mx-1 h-5 w-px bg-white/10" />
}

function Menu({ label, active, children }: { label: string; active: boolean; children: React.ReactNode }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <ToggleChip pressed={active} aria-haspopup="menu" aria-pressed={undefined}>
          {label} <CaretDown className="size-3.5 opacity-70" />
        </ToggleChip>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[220px]">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
