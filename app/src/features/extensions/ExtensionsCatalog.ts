export interface CatalogExtension {
  id: string
  name: string
  version: string
  description: string
  author: string
  authorGithub?: string
  authorUrl?: string
  kind: "exporter" | "post-processor" | "webhook" | "custom"
  accentColor: string
  bannerGradient: string
  capabilities: string[]
  downloads: string
  rating: string
  featured?: boolean
  readme: string
  templateFiles: {
    name: string
    content: string
  }[]
}

export const EXTENSIONS_CATALOG: CatalogExtension[] = [
  {
    id: "amverge.deadframes.remover",
    name: "AMVerge Deadframe Remover",
    version: "1.0.0",
    description: "Detects and removes motionless holding frames from exported clips using optical flow and frame compaction.",
    author: "AMVerge Team & AutoScene",
    authorGithub: "AMVerge-team",
    authorUrl: "https://github.com/AMVerge-team/AMVerge-CLI",
    kind: "post-processor",
    accentColor: "#6366f1",
    bannerGradient: "from-indigo-900/60 via-purple-900/30 to-black/60",
    capabilities: ["filesystem:write-export"],
    downloads: "1.8k",
    rating: "4.9",
    featured: true,
    readme: `### AMVerge Deadframe Remover

Based on the [AMVerge-CLI](https://github.com/AMVerge-team/AMVerge-CLI) motion analysis pipeline.

#### Features
- **Optical Flow Analysis**: Analyzes dense motion vectors between consecutive frames.
- **Holding Frame Elimination**: Drops frozen frames while maintaining smooth visual continuity.
- **Lossless Compaction**: Reassembles audio and active motion segments into \`_compacted\` files.

#### Capabilities & Permissions
- \`filesystem:write-export\`: Creates compacted versions alongside original exports.`,
    templateFiles: [],
  },
  {
    id: "autoscene.sample.desktop-notifier",
    name: "Desktop Notifier & Sound Alert",
    version: "1.0.0",
    description: "Plays an alert chime and displays a native desktop notification when an export finishes.",
    author: "AutoScene Team",
    authorGithub: "tcmmi",
    authorUrl: "https://github.com/tcmmi/AutoScene",
    kind: "post-processor",
    accentColor: "#f59e0b",
    bannerGradient: "from-amber-900/60 via-orange-900/30 to-black/60",
    capabilities: ["filesystem:write-export"],
    downloads: "2.4k",
    rating: "4.8",
    featured: true,
    readme: `### Desktop Notifier & Sound Alert

A lightweight post-export hook that triggers native notification banners and audible alerts.

#### Features
- Native notification toast with job name and clip counts.
- Audio alert sound so you know when renders finish in the background.
- Zero extra dependencies required on macOS and Linux.`,
    templateFiles: [],
  },
  {
    id: "autoscene.timeline.cmx3600-edl",
    name: "CMX 3600 EDL Timeline Exporter",
    version: "1.0.0",
    description: "Generates industry-standard CMX 3600 Edit Decision Lists (EDL) for Premiere Pro, Final Cut, and DaVinci Resolve.",
    author: "AutoScene Team",
    authorGithub: "tcmmi",
    authorUrl: "https://github.com/tcmmi/AutoScene",
    kind: "exporter",
    accentColor: "#10b981",
    bannerGradient: "from-emerald-900/60 via-teal-900/30 to-black/60",
    capabilities: ["filesystem:write-export"],
    downloads: "3.1k",
    rating: "5.0",
    featured: true,
    readme: `### CMX 3600 EDL Exporter

Export your cut lists and scenepacks directly into professional NLE timelines.

#### Features
- Compatible with Adobe Premiere Pro, DaVinci Resolve, Final Cut Pro, and Avid Media Composer.
- Frame-accurate SMPTE timecode calculation with drop-frame rate support.
- Automatically links with original media reel names.`,
    templateFiles: [
      {
        name: "extension.json",
        content: JSON.stringify(
          {
            id: "autoscene.timeline.cmx3600-edl",
            name: "CMX 3600 EDL Timeline Exporter",
            version: "1.0.0",
            description: "Generates industry-standard CMX 3600 Edit Decision Lists (EDL).",
            author: "AutoScene Team",
            authorGithub: "tcmmi",
            kind: "exporter",
            entrypoint: "run.sh",
            accentColor: "#10b981",
            capabilities: ["filesystem:write-export"],
            triggers: ["post-export"],
          },
          null,
          2,
        ),
      },
      {
        name: "run.sh",
        content: `#!/usr/bin/env sh
echo "CMX 3600 EDL Exporter activated."
`,
      },
    ],
  },
  {
    id: "autoscene.webhook.discord",
    name: "Discord Webhook Publisher",
    version: "1.1.0",
    description: "Sends formatted Discord embed notifications with thumbnail previews and clip statistics when exports complete.",
    author: "Community",
    authorGithub: "moongetsu",
    authorUrl: "https://github.com/moongetsu",
    kind: "webhook",
    accentColor: "#5865F2",
    bannerGradient: "from-blue-900/60 via-indigo-950/40 to-black/60",
    capabilities: ["network:webhook"],
    downloads: "940",
    rating: "4.7",
    featured: false,
    readme: `### Discord Webhook Publisher

Automatically posts an embed to your Discord channel when an export or character scenepack finishes rendering.

#### Configuration
Set your \`WEBHOOK_URL\` in the extension config or environment.`,
    templateFiles: [
      {
        name: "extension.json",
        content: JSON.stringify(
          {
            id: "autoscene.webhook.discord",
            name: "Discord Webhook Publisher",
            version: "1.1.0",
            description: "Sends formatted Discord embed notifications when exports complete.",
            author: "Community",
            authorGithub: "moongetsu",
            kind: "webhook",
            entrypoint: "run.sh",
            accentColor: "#5865F2",
            capabilities: ["network:webhook"],
            triggers: ["post-export"],
          },
          null,
          2,
        ),
      },
      {
        name: "run.sh",
        content: `#!/usr/bin/env sh
echo "Discord webhook triggered."
`,
      },
    ],
  },
]
