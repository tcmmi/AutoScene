# AutoScene Extension Developer Guide & SDK Reference

AutoScene features an out-of-process, declarative Extension UI and Hook system. Extensions can process media files, export cut lists to NLEs, post webhooks, and **inject custom buttons and menus directly into the AutoScene desktop UI**.

---

## 1. Extension Manifest (`extension.json`)

Every extension is placed in its own folder inside AutoScene's extensions directory:
* macOS: `~/Library/Application Support/AutoScene/extensions/<your-extension>/`
* Windows: `%APPDATA%\AutoScene\extensions\<your-extension>\`
* Linux: `~/.config/autoscene/extensions/<your-extension>/`

### Manifest Schema & Structure

```json
{
  "id": "com.example.my-extension",
  "name": "My Custom Extension",
  "version": "1.0.0",
  "description": "Short summary of what this extension does.",
  "author": "Your Name",
  "authorGithub": "your-github-username",
  "authorUrl": "https://yourwebsite.com",
  "kind": "post-processor",
  "entrypoint": "run.sh",
  "icon": "icon.svg",
  "banner": "banner.svg",
  "accentColor": "#6366f1",
  "homepage": "https://github.com/your-username/my-extension",
  "repository": "https://github.com/your-username/my-extension",
  "capabilities": [
    "filesystem:write-export"
  ],
  "triggers": [
    "post-export"
  ],
  "contributes": {
    "navButtons": [
      {
        "id": "my.nav.action",
        "label": "My Action",
        "icon": "Scissors",
        "tooltip": "Perform custom workflow",
        "when": "route == exports || route == timeline"
      }
    ],
    "railButtons": [
      {
        "id": "my.rail.tool",
        "label": "My Tool",
        "icon": "FilmStrip",
        "tooltip": "Quick launcher"
      }
    ],
    "menus": [
      {
        "target": "exports:clip",
        "label": "Process Clip with My Tool",
        "icon": "Scissors"
      },
      {
        "target": "exports:card",
        "label": "Batch Process with My Tool"
      }
    ],
    "settings": [
      {
        "id": "threshold",
        "label": "Sensitivity",
        "type": "number",
        "defaultValue": 0.5,
        "description": "Detection sensitivity threshold."
      }
    ]
  }
}
```

---

## 2. Declarative UI Contributions (`contributes`)

### A. Navbar Buttons (`navButtons`)
Injected into AutoScene's top navigation bar (`TopBar`).
* `id` *(string)*: Unique identifier for the button.
* `label` *(string)*: Button text.
* `icon` *(optional string)*: Phosphor icon name (`"Scissors"`, `"FilmStrip"`, `"Package"`, `"Play"`, etc.).
* `tooltip` *(optional string)*: Tooltip shown on hover.
* `when` *(optional string)*: Route condition for visibility (e.g. `"route == exports"`, `"route == timeline"`, `"route == pack"`).

### B. Navigation Rail Items (`railButtons`)
Injected into the left-hand navigation sidebar (`Rail`) above Settings.
* `id` *(string)*: Unique identifier.
* `label` *(string)*: Label displayed beneath the icon.
* `icon` *(optional string)*: Phosphor icon name.
* `tooltip` *(optional string)*: Tooltip shown on hover.

### C. Context Menus (`menus`)
Injected into AutoScene's right-click context menus.
* `target` *(string)*: The menu slot to inject into:
  - `"exports:clip"`: Right-click on individual clip rows in the Exports page.
  - `"exports:card"`: Right-click on the export card (batch action).
  - `"player:timeline"`: Right-click on the player timeline.
  - `"pack:card"`: Right-click on pack character shot cards.
* `label` *(string)*: Menu item text.
* `icon` *(optional string)*: Icon shown in the menu.

---

## 3. Execution & Payloads

When an extension runs (whether triggered automatically via `post-export`, manually via topbar button, or via context menu), AutoScene creates a temporary JSON metadata payload and passes its path through the `AUTOSCENE_META` environment variable.

### Metadata Payload (`AUTOSCENE_META`)
```json
{
  "exportId": "export-1700000000000",
  "projectId": "project-slug",
  "projectTitle": "My Series",
  "outDir": "/path/to/exports",
  "clipCount": 3,
  "outputFiles": [
    "/path/to/exports/Clip_01.mov",
    "/path/to/exports/Clip_02.mov",
    "/path/to/exports/Clip_03.mov"
  ],
  "completedAt": 1700000000000
}
```

### Script Execution Wrapper (`run.sh`)
```bash
#!/usr/bin/env sh
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
exec python3 "$DIR/main.py"
```

In your script (Python, Node.js, Bash, Rust, etc.):
```python
import os, json, sys
from pathlib import Path

meta_path = os.environ.get("AUTOSCENE_META")
if meta_path and os.path.exists(meta_path):
    with open(meta_path, "r") as f:
        payload = json.load(f)
    print(f"Processing {payload['clipCount']} clips from {payload['outDir']}")
```

