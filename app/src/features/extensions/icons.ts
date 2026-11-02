import * as Icons from "@/components/icons"

/** Maps dynamic string icon names from manifests (e.g. "Scissors", "scissors", "FilmStrip") to Phosphor icon components. */
export function getExtensionIcon(name: string | undefined | null) {
  if (!name) return Icons.Package

  const clean = name.trim().toLowerCase().replace(/[-_\s]/g, "")

  // Fast mapping to known AutoScene Phosphor icons
  switch (clean) {
    case "scissors":
    case "cut":
      return Icons.Scissors
    case "filmstrip":
    case "reel":
      return Icons.FilmStrip
    case "filmslate":
      return Icons.FilmSlate
    case "package":
    case "box":
      return Icons.Package
    case "play":
      return Icons.Play
    case "gear":
    case "gearsix":
    case "settings":
      return Icons.GearSix
    case "check":
    case "checkcircle":
      return Icons.CheckCircle
    case "download":
    case "downloadsimple":
      return Icons.DownloadSimple
    case "copy":
      return Icons.Copy
    case "folder":
    case "folderopen":
      return Icons.FolderOpen
    case "user":
    case "userplus":
      return Icons.User
    case "info":
      return Icons.Info
    case "warning":
      return Icons.Warning
    case "scan":
      return Icons.Scan
    default:
      return Icons.Package
  }
}
