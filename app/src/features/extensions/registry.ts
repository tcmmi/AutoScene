import { useMemo } from "react"
import type {
  CommandContribution,
  InstalledExtension,
  MenuContribution,
  NavButtonContribution,
  RailButtonContribution,
  SettingFieldContribution,
} from "@/bindings"
import { useExtensions, useSettings } from "@/lib/queries"

export interface ActiveNavContribution {
  extension: InstalledExtension
  button: NavButtonContribution
}

export interface ActiveRailContribution {
  extension: InstalledExtension
  button: RailButtonContribution
}

export interface ActiveMenuContribution {
  extension: InstalledExtension
  menu: MenuContribution
}

export interface ActiveCommandContribution {
  extension: InstalledExtension
  command: CommandContribution
}

export interface ActiveSettingContribution {
  extension: InstalledExtension
  fields: SettingFieldContribution[]
}

/** Filter out disabled extensions according to AppSettings. */
function useEnabledExtensions(): InstalledExtension[] {
  const { data: extensions = [] } = useExtensions()
  const { data: settings } = useSettings()

  return useMemo(() => {
    const disabled = new Set(settings?.disabledExtensions ?? [])
    return extensions.filter((ext) => !disabled.has(ext.manifest.id))
  }, [extensions, settings?.disabledExtensions])
}

/** Check if a `when` clause holds for the current route pathname. */
function matchesWhen(when: string | undefined | null, pathname: string): boolean {
  if (!when || !when.trim()) return true
  const normalized = when.toLowerCase().trim()

  // Match expressions like "route == exports" or "route == timeline"
  if (normalized.includes("route == exports") || normalized.includes("route == /exports")) {
    return pathname.includes("/exports")
  }
  if (normalized.includes("route == timeline") || normalized.includes("route == /timeline")) {
    return pathname.includes("/timeline")
  }
  if (normalized.includes("route == pack") || normalized.includes("route == /pack")) {
    return pathname.includes("/pack")
  }
  if (normalized.includes("route == cast") || normalized.includes("route == /cast")) {
    return pathname.includes("/cast")
  }
  if (normalized.includes("route == library") || normalized.includes("route == /")) {
    return pathname === "/" || pathname === "/library"
  }
  return true
}

/** Returns all enabled extension nav buttons matching the active path. */
export function useNavContributions(pathname: string): ActiveNavContribution[] {
  const enabled = useEnabledExtensions()

  return useMemo(() => {
    const results: ActiveNavContribution[] = []
    for (const ext of enabled) {
      const navButtons = ext.manifest.contributes?.navButtons ?? []
      for (const btn of navButtons) {
        if (matchesWhen(btn.when, pathname)) {
          results.push({ extension: ext, button: btn })
        }
      }
    }
    return results
  }, [enabled, pathname])
}

/** Returns all enabled extension rail navigation items. */
export function useRailContributions(): ActiveRailContribution[] {
  const enabled = useEnabledExtensions()

  return useMemo(() => {
    const results: ActiveRailContribution[] = []
    for (const ext of enabled) {
      const railButtons = ext.manifest.contributes?.railButtons ?? []
      for (const btn of railButtons) {
        results.push({ extension: ext, button: btn })
      }
    }
    return results
  }, [enabled])
}

/** Returns all context menu contributions for a given target slot (e.g. "exports:clip", "exports:card"). */
export function useMenuContributions(target: string): ActiveMenuContribution[] {
  const enabled = useEnabledExtensions()

  return useMemo(() => {
    const results: ActiveMenuContribution[] = []
    for (const ext of enabled) {
      const menus = ext.manifest.contributes?.menus ?? []
      for (const item of menus) {
        if (item.target === target) {
          results.push({ extension: ext, menu: item })
        }
      }
    }
    return results
  }, [enabled, target])
}

/** Returns all contributed commands for the Command Palette. */
export function usePaletteContributions(): ActiveCommandContribution[] {
  const enabled = useEnabledExtensions()

  return useMemo(() => {
    const results: ActiveCommandContribution[] = []
    for (const ext of enabled) {
      const commands = ext.manifest.contributes?.commands ?? []
      for (const cmd of commands) {
        results.push({ extension: ext, command: cmd })
      }
    }
    return results
  }, [enabled])
}

