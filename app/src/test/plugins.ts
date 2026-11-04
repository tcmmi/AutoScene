import type { CatalogPlugin, CatalogStatus, CatalogView, PluginInfo, PluginReview, PluginUpdate } from "@/bindings"

/** An installed plugin as `plugins_list` gives it: reviewed, on and with nothing it adds, unless `o` says otherwise. */
export function pluginInfo(o: Partial<PluginInfo> & Pick<PluginInfo, "id" | "name">): PluginInfo {
  return {
    version: "1.0.0",
    description: "",
    author: "Someone",
    license: null,
    repository: null,
    trust: "reviewed",
    authorKeyId: null,
    source: "package",
    folder: `C:\\AutoScene\\plugins\\${o.id}`,
    enabled: true,
    active: true,
    blockedReason: null,
    broken: null,
    withdrawn: null,
    permissions: [],
    networkHosts: [],
    gpu: false,
    uvScript: true,
    exportActions: [],
    commands: [],
    settings: [],
    runtime: "process",
    sandbox: null,
    protocol: 1,
    pages: [],
    secretsSet: [],
    icon: null,
    bannerHash: null,
    logPath: `C:\\AutoScene\\logs\\plugins\\${o.id}.log`,
    serviceLogPath: null,
    warnings: [],
    resourceBytes: 0,
    ...o,
  }
}

/** What the catalog offers of a plugin: reviewed, new here and runs here, unless `o` says otherwise. */
export function catalogPlugin(o: Partial<CatalogPlugin> & Pick<CatalogPlugin, "id" | "name">): CatalogPlugin {
  return {
    summary: "",
    author: "AutoScene",
    license: "MIT",
    version: "1.0.0",
    platforms: [],
    permissions: [],
    hosts: [],
    gpu: false,
    uvScript: true,
    runtime: "process",
    repository: "https://github.com/tcmmi/AutoScene",
    commit: "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a090",
    path: `examples/plugins/${o.id.split(".").pop()}`,
    icon: null,
    sizeBytes: 48_213,
    approved: 1_762_128_000_000,
    installed: null,
    update: false,
    addsPermissions: [],
    addsHosts: [],
    runsHere: true,
    needsNewerApp: false,
    resourceBytes: 0,
    addsResourceHosts: [],
    ...o,
  }
}

/** The catalog as `catalog_list` gives it: living, checked at `checkedAt`, offering `plugins`. */
export function catalogView(plugins: CatalogPlugin[], status: Partial<CatalogStatus> = {}): CatalogView {
  return {
    status: {
      ready: true,
      daily: true,
      published: Date.UTC(2026, 10, 3, 9),
      expires: Date.UTC(2026, 11, 3, 9),
      fetchedAt: Date.UTC(2026, 10, 3, 10),
      checkedAt: Date.UTC(2026, 10, 3, 10),
      outOfDate: false,
      error: null,
      needsNewerApp: false,
      ...status,
    },
    plugins,
  }
}

/** A review of `plugin` as the catalog gives it (`catalog_get`): reviewed, an update when `update` says so. */
export function catalogReview(plugin: PluginInfo, update: Partial<PluginUpdate> | null = null): PluginReview {
  return {
    ticketId: `t-${plugin.id}`,
    plugin,
    update: update && {
      fromVersion: "1.0.0",
      trustBefore: "reviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      addedResourceHosts: [],
      ...update,
    },
    needsUnreviewed: false,
    allowed: true,
    uvMissing: false,
    uvBytes: null,
    resourceBytes: 0,
    resourceHosts: [],
    componentWarnings: [],
    catalog: {
      published: Date.UTC(2026, 10, 3, 9),
      repository: "https://github.com/tcmmi/AutoScene",
      commit: "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a090",
      path: `examples/plugins/${plugin.id.split(".").pop()}`,
    },
  }
}
