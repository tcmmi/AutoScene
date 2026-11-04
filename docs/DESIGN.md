# AutoScene design system — Cinematic

AutoScene turns series and movies into scenepacks of the characters an editor chooses. The UI should feel like a
premium film app — Apple TV, Infuse, a colour suite in a dark room — not like a web dashboard: the pictures (episode
stills, faces, shots) carry the screen, the chrome stays quiet and dark around them.

Tokens live in `app/src/styles/tokens.css` and are mapped into Tailwind in `app/src/styles/index.css`. Never use raw
hex values or Tailwind palette colours (`amber-500`, `zinc-800`…) in components; white at an opacity
(`bg-white/[0.06]`) is how surfaces step up on the dark and over pictures.

## Brand

- **Mark:** the gold "A" — `app/src/components/brand/AutoSceneMark.tsx` (also `app/public/autoscene-mark.svg`,
  `app/src-tauri/icons/*`). Geometry and colours are fixed: tile `#1C1A17`, chevron and dot `#C4A35A`.
- The mark sits at the top of the rail, in About and in the empty library. A soft gold drop shadow may light it;
  never outlines or rings.

## Colour

| Role | Token | Value |
|---|---|---|
| The theatre (window canvas) | `--bg-canvas` | `#09090B` |
| Wells, cards, raised controls, overlays | `--bg-inset` / `--bg-panel` / `--bg-raised` / `--bg-overlay` | `#050506` / `#111114` / `#18181C` / `#202025` |
| The rail | `--bg-rail` | solid `#0C0C0E`; translucent over the window material |
| Text | `--fg-1/2/3` | warm white `#F5F3EF` / `#B4B1AC` / `#8A8783` (≥ 4.5:1 on panels) |
| **The main action** | `--cta`, `--fg-on-cta` | warm white pill, near-black text |
| **Accent** | `--accent` | `#E0B45C` — the logo gold, brightened for dark screens |
| Errors, destructive | `--danger` | `#EF5F57` |
| Verified, done | `--ok` | `#5FC58F` (export checks only) |
| Over pictures | `--scrim-media`, `--fg-on-media`, `Badge tone="media"`, `tone="picture"` | black scrims, white text, glass chips; on a card's picture the small dark chip (`size="sm"`) |

Rules:

- One **white pill** per screen is the main action (Build a scenepack, Export, Review): `Button variant="primary"`.
- `Button variant="cta"`, the solid warm-white pill, for the answer most people give on a surface that leads with a
  picture (the first-start notice); elsewhere the main action stays `primary`.
- **Gold** marks what is selected, running or in focus: the open project in the rail, progress (with a soft glow),
  in/out points, the focused character lane, toggles, focus rings. `variant="accent"` only for a confirmation
  that belongs to a selection ("All are Walter").
- Pictures bring the colour. A hero or the player takes its glow from the picture itself (`useAmbient`, the
  player's ambilight) — never a fixed decorative colour.

## Type

- **Inter Variable** (optical sizes) for the UI, base 14px. Scale: 11 / 12 / 13 / 14 / 16 / 20 / 26 / 34 / 48 / 64 /
  84. Section titles 20px semibold; labels in sentence case (`eyebrow`, 12px semibold) — no all-caps.
- **Instrument Serif** (`display` utility) for the big titles only: series and movie titles, character names,
  page titles (Cast, Episodes…), empty states, and the first-start notice's title, which stands in for the welcome.
  Unnamed characters are set in italic.
- **Geist Mono Variable** (`timecode` utility) for timecodes, frame numbers, paths, sizes.

## Shape, depth, light

- Pills for buttons, chips and switches; cards 14–20px radius, portraits and heroes 20–32px.
- Pictures sit like prints: `picture-edge` (a fine light edge and a deep shadow); on hover a card rises 4px and
  its picture slowly zooms (`shadow-lift`).
- **Glass** (`glass`, `glass-strong`) for everything that floats: menus, tooltips, dialogs, the palette, the
  activity drawer, the top bar once a page scrolls, buttons over pictures (`variant="glass"`).
- **Heroes** (`components/ui/hero.tsx`): the picture bleeds to the window's edges and fades into the page; text on
  its dark side. Projects use a still, characters their face blurred into a backdrop.

## Icons

- Phosphor, regular weight; `fill` marks the selected place. Import icons only from `app/src/components/icons.ts`
  — oxlint rejects `lucide-react` and direct Phosphor imports elsewhere.
- An icon says what the control does. No decorative "AI" clichés: sparkles, lightning, magic wands, robots, stars
  (banned in `icons.ts` by lint).

## Motion

- 120ms hovers, 200ms layers, 320ms drawers and page arrivals (`animate-rise-in`). Ease `cubic-bezier(.2,.8,.2,1)`;
  switches spring.
- Animate opacity and transform only; pictures fade in when loaded. Respect `prefers-reduced-motion`.
- Progress glides linearly at the pace of its updates and never runs backwards (a new stage starts its bar at once).
  A gap under a second and a half doesn't turn it into a sweep. Waiting work stands still: an empty bar, a ring with
  no arc, a clock instead of a spinner. Under reduced motion, busy things fade instead of moving.
- A button whose action runs says so with `busy`: the spinner takes its first icon's place, or stands over the label
  when it has none, and the button keeps its width. `ConfirmDialog` asks before something goes and can't be left while
  it runs.

## Window & navigation

- A desktop app, not a web page: no site header or breadcrumbs, the hand on everything a click acts on (buttons, links,
  tabs, switches, sliders), the arrow on disabled controls, on the entries of menus and lists, on the player's picture
  and on the window's caption buttons; text not selectable outside fields and paths.
- **Rail** (76px, `components/shell/Rail.tsx`) on the window's left edge, on the native material where there is
  one: the mark, Library, each recent project as its picture (gold ring when open), New project, and at the bottom
  Search (`Ctrl/⌘ K`), Activity (a ring fills as work runs), Plugins (lit on Plugins and on a plugin's settings) and
  Settings. Plugins' tile menu offers Install from a file…, and Load a folder… while plugins AutoScene hasn't reviewed
  are allowed; the tile spins while a plugin is read, and the review shows over whatever is open. `Ctrl/⌘ B`
  hides it. Under Library sit up to four plugin page tiles, each with the plugin's icon, AutoScene's plugin mark in
  its corner and a tooltip that starts with "Plugin"; the other pages are in the palette.
- **Top bar** (56px, `components/ui/topbar.tsx`) per view: back and title on the left, a project's sections
  (Overview · Episodes · Cast · Exports) as a pill in the middle, actions on the right. It is the drag region
  (double-click maximises); over a hero it is clear until the page scrolls, then glass with the title. A project's
  pill gains a fifth item, More, when plugins add pages to projects; it never grows past five.
- **Plugin pages** (`features/plugins/PluginPageView.tsx`) fill the view under AutoScene's top bar, whose title
  shows the plugin's icon, the page's title and a "Plugin" badge no plugin can word. What is inside the frame is the
  plugin's; its ⋯ menu and right-click are AutoScene's. A page of a plugin AutoScene withdrew has no frame: the red
  `Banner` (Uninstall…, and Update… when the catalog has a version that isn't withdrawn) takes its place.
- **Native chrome** per platform: Windows 11 Mica behind the rail with our own caption buttons over the top-right
  corner (`--caption-inset`); macOS vibrancy with the traffic lights over the rail's top (`--traffic-inset`);
  Linux and Windows 10 solid. The material is only claimed where it exists (`detectMaterial`), so nothing turns
  see-through without it.
- The native window title follows the view (taskbar, Alt+Tab): "Cast · Walter — Dark — AutoScene".
- Background work is never page-bound: it shows in the rail's Activity ring and the Activity drawer (`Ctrl/⌘ J`).
- The palette (`Ctrl/⌘ K`) finds projects, places and commands, and in a project its characters and episodes
  (by name, `S01E03`, `1x03` or file). Back and Forward return a page to where it was scrolled.

## Menus & tooltips

- Nothing shows the browser's own chrome: no native tooltips, context menus, form controls, autofill, find bar or
  link drags. oxlint refuses `title` and native controls (`select`, `progress`, range and checkbox inputs…);
  browser keys the app has no use for are swallowed (`lib/browser-guards.ts`). Inside a plugin page, native controls
  (a select, a checkbox, a range) are the page's own and may show; these rules hold for everything outside its frame.
- **Tooltips** (`ui/tooltip.tsx`) are glass: the label, an optional second line in `--fg-3` (`hint`) and the key
  that does the same (`shortcut`). Text cut by its box (`TruncatedText`) shows the rest only when it is cut.
- **One context menu** for the window (`shell/AppMenu.tsx`). An element says what a right-click offers with
  `ContextArea`. The menu lists the clicked element's own entries first, innermost first: a shot's before its
  card's. The short tail follows: Command palette, Settings.
  - On a page's empty space (`Page menu`) the menu shows the page's entries, then the full app list: Back, Command
    palette, New project, Library, Activity, Plugins, the rail, Keyboard shortcuts, Settings, Reload.
  - Plugins' own `Page menu` offers what its rail tile's menu offers (Install from a file…, and Load a folder… while
    allowed) above the full app list.
  - Dialogs and the drawer show only what they offer, never the app's entries, and no menu when they offer
    nothing.
  - On Windows the top bar's empty parts and its title are the window's caption: Minimize, Maximize, Close.
  - Selected text in a path or an error offers Copy first.
  - An entry runs once its menu has closed, so a field or a dialog it opens keeps the focus.
- Entries say what happens to the thing clicked ("Mark it as the clip", "Not Walter") and show the key that does
  the same.
  - An entry that can't apply to this thing is left out; one that can't apply yet is disabled.
  - Where the pointer picks the subject (a frame, a shot, an appearance on the timeline), a label names it first:
    "Shot 5 · 00:00:09:04".
  - A ⋯ menu and the right-click list the same actions from one list (the kits in `ui/menu-actions.tsx`).
- **Text fields** have their own menu (`shell/TextMenu.tsx`), in the same glass: Undo, Redo, Cut, Copy, Paste,
  Delete, Select all.
  - It never takes the focus from the field, and Escape closes only the menu.
  - Paste reads the clipboard natively, so the webview never asks for permission.
- While developing, Shift+right-click still opens the browser's menu (for Inspect).
- A **tick box** (`ui/checkbox.tsx`) only for an option that matters once, such as Don't show this again; settings
  use switches.

## Views

- **Library:** the project touched last as a hero ("Continue"), then every project as a 16:9 picture card.
- **New project:** title and type, then footage and Fast Scan side by side (the switch's line says what Fast Scan
  costs and, where it is on because of this computer's graphics, why), then the videos. The list shrinks on short
  windows so Create project stays on screen.
- **First start:** where new projects start with Fast Scan because of this computer's graphics, a notice says so
  over the library at each start until Don't show this again is ticked or a default is chosen — a still with the
  faces Fast Scan finds (one only half sure), the graphics by name, what Fast Scan gives and costs — with Turn off
  Fast Scan and a warm-white Continue. It never opens over another layer, steps aside for New Project, and the update
  dialog waits for it.
- **Project overview:** the hero, then shelves (`Section` + `Shelf`) of the cast as portraits and the episodes as
  stills. Files that can't be found show as a gold banner with Locate folder… here and on Episodes (the shared
  `Banner`, `components/ui/banner.tsx`: gold for what the person can fix, red for what is wrong or was taken away,
  `neutral` for how things stand when neither is true).
- **Cast:** portrait cards (3:4), Everyone else, hidden characters folded away; Review as the white pill. From six
  characters a row under the title finds someone: a search, All · Named · To review, and the order. Ctrl/⌘-click
  selects cards, Shift-click every one between; a glass bar at the bottom hides them (a toast takes them back).
- **Character:** the project's artwork as the hero (their blurred face without any) with the portrait (click to choose it from their clearest faces), the name
  (click to rename) and facts as chips; Build a scenepack as the white pill; shots scrub on hover. Shots close
  together stack on one card: hover scrubs them all, a click opens them in a band under the row, Enter or the Play
  button watches from the first.
- **Scenepack:** the character's portrait and name, a glass bar that stays under the top bar (the order; Filters with
  the number that are on, plugin filters counted; the handles; a plugin's filters as chips with the plugin mark: a ring
  fills while one checks, a warning mark once its check failed, a filter of a plugin that is off is dimmed; Reset
  filters once anything differs). Filters is one menu that stays open while filters are chosen: shot sizes, then length,
  episodes and limit each a step further with the choice beside its name, then "only them" and flash cuts. Then every
  shot of the
  character: the ones the pack takes bright with a gold tick, the rest dimmed with the reason (a filter's own words
  for the shots it leaves out). A click takes a shot in or out,
  Shift every shot since the last. A glass bar floats at the bottom with the count, length and size, the picks by
  hand, how far a plugin's check is (Export waits for it) or that one failed (Try again; the shots it didn't check keep
  its word from before, as the grid shows, and Export goes on) and Export as the white pill, with a ⋯ beside it: what
  can be done to every shot at once (Take every shot, Leave every shot out, Invert the pack, each with its keys, and
  Hand every shot back to the filters while there are picks; never buttons beside the tabs, they would push the hint
  to a line of its own; each says what it did in a toast with Undo, which puts the picks back as they were), then, when a plugin saves the pack as its own timeline file, those entries in a group of their
  own (in a menu that scrolls inside the window, each with the plugin's mark and name; the group's heading says what
  it waits for: the check, a shot in the pack, the pack itself; the same heading in both menus). The grid's right-click menu lists the same entries after Export; the export dialog makes one file
  with a chapter per shot (a switch at the top, on by default) or a file per shot, named from templates: the parts
  stand out in the field as quiet pills, a mistyped one in red where it is; the name-part buttons are one Tab stop,
  and each says what it gives.
- **Review:** one face against the suggestion, both big, the face lighting the room; answers on the keyboard.
- **Player:** a dark room — the picture as large as the stage allows at the file's own shape (a preview copy or a
  small file grows too), on a rounded screen with its own ambilight. Every control has one place; a right-click
  may list it again.
  - **Transport:** one row under the picture, grouped round the white play button. Volume sits at the left edge,
    then the speed slot (a gold chip while J/K/L shuttle), the large timecode and the frame counter against the
    buttons. On their other side In and Out (gold once set), the clip's length (behind a quiet |↔| mark, so it
    never reads as a third timecode) and ✕, and Export at the right edge.
    In, Out and Export show only here. Nothing in the row moves while the episode plays or shuttles: the slot is
    always there and the timecode keeps one width per display mode. A narrower row drops extras in steps (the
    In/Out timecodes and the counter, then the volume slider and Export's label, then the rest of the volume, the
    clip's length and ✕), each still in a tooltip, a menu or a key.
  - **Details:** one panel on the right without cards. Cast · File as a pill, remembered. Cast holds the shot
    under the playhead, what the analysis still has to find (with the button that finds it), who is in the shot
    first, then the episode's characters and Open the cast. File holds the file, its length and frame count, and
    the audio track. The top bar shows or hides the panel (remembered too); it opens at once, and its content
    fades in.
  - **Player keys dialog:** opens from the panel's header and the player menu. It lists the player's keys as set,
    with Change keys… for Settings → Shortcuts, so someone can try a key without leaving the playhead.
  - **Timeline:** on the page, without a card (ruler, scrubber, shots, character lanes), each row drawn at its
    real size. A character highlighted in the panel always gets a lane. It zooms (Ctrl/⌘ + wheel, pinch, + and −,
    Shift+Z for the whole episode) and scrolls with the wheel; zoomed in, a thin overview of the episode shows the
    part in view.
  - Only the timecode, the playhead and the glow follow every video frame; shots and lanes are drawn on canvases.
- **Plugins** (`/plugins`): Installed · Browse as a pill under the title (`?view=browse` keeps Browse), the search
  beside it. Installed: every installed plugin as a 2:1 card in the Library's grid. The art is the plugin's banner with its
  name on the dark bottom; else its icon blurred into a backdrop, the sharp icon on glass and the name; else its name
  set large as the picture (no glyph: two such plugins differ by their names). The caption under the art is
  AutoScene's: the trust badge, "Says it is by …" ("By …" only for a package AutoScene reviewed) and the version,
  the switch; then why it doesn't run, when it is broken (red) or not allowed (calm, pointing at the setting below).
  A plugin turned off is dimmed. The title's line counts the plugins and how many run. ⋯ shows on hover and lists what
  the right-click does; a broken loaded folder offers Load its folder again… only while unreviewed plugins are
  allowed. Install from a file… sits back in dark glass (Load a folder… beside it while unreviewed plugins are
  allowed): the catalog is the way in, so with nothing installed Browse plugins is the white pill. From six plugins a
  search finds one by its name, author, description or id; the allow switch sits below the cards. When the catalog
  has newer versions, they sit above the rest under Updates, each card with Update…, and Update all (n) takes those
  that ask for nothing new; one whose review marks anything New (a permission, host, page, program, a program kept
  running, higher limits in the sandbox, or a host its large files come from), that leaves the sandbox it ran in, or
  that replaces a loaded folder or a plugin AutoScene hadn't reviewed, opens that review instead (a larger download
  from hosts it already named stays quiet: the files' hashes are what was reviewed). After that review, an update that
  may do more than the person agreed to (a permission, host or program, one kept running, one out of the sandbox) also
  takes a yes in a dialog of the system's own, which the page can't give or skip for an update (uninstalling and
  installing again is a first install, which it doesn't ask for). A
  plugin AutoScene withdrew has a red `Banner` at the top of both views (at most three, then "and n more"), "AutoScene withdrew Plugin
  “name”" with the catalog's reason, Update… while the catalog has a version that isn't withdrawn, and Uninstall….
  Its card says "Withdrawn" in place of the trust badge, shows the reason under the facts, and its switch is off and
  can't be turned on; a toast says so once per session when a check withdraws it.
- **Plugins → Browse:** the catalog first (when it was checked, Check now, and the daily switch; a gold banner when
  it is out of date, and then no cards and no search), then each plugin AutoScene's catalog offers as a card in the
  same art language, its icon as the art (the catalog has no banners) or its name. The caption: "Reviewed by
  AutoScene", "By … · version …", its summary, what it may do in one line, and Install… or Update… (or Installed,
  Not for this computer, Needs a newer AutoScene); a plugin that names large files adds "Downloads 312 MB at its
  first run" under it, while it is to be installed (an update says only "Downloads from a new host: …", when its
  files come from one: the review knows what is still to get). No white pill: each card installs its own. While a package comes, the review dialog shows how far, with Cancel; the review
  then says where the catalog built it from and, under "How it runs", how much a plugin's first run downloads and from
  which hosts ("It downloads 312 MB from huggingface.co at its first run…", an update's "at its next run"), a host
  the installed version didn't name on a line of its own under it, with the New badge before the sentence. A
  plugin's settings list "Downloaded files" with their size in the Files group once it has some.
- **A plugin's settings** (`/plugins/<id>`, "Its settings" in menus; Settings lists only the app's own sections): its
  picture as the hero, always blurred (the banner, else the icon, else the fallback), the sharp banner or icon
  framed beside the name, the trust badge, the version and the licence as chips, then "Says it is by …" ("By …"
  only for a package AutoScene reviewed) and what it says it does. On the right the switch and ⋯. Trust text never
  sits on a plugin's own picture. Below, a banner only when something needs fixing, with the control that fixes it
  (Allow them…, Load its folder again…; a withdrawn plugin's red banner with Uninstall…, and its badge says "Withdrawn"),
  then its settings, after every export, pages, what it may use and files.
- **Settings → Shortcuts:** the keymap first — Keys like in AutoScene · Premiere Pro · DaVinci Resolve as a pill
  (a card under each option lists where its keys differ) and Reset all — then every shortcut by place. A click on
  a key listens for the new one (a gold pill, held modifiers shown, Esc stops); a key another shortcut in reach
  had is taken from it (a toast says so, with Undo), one that can't be a shortcut there says why in red. Keys the
  person chose are gold, with a reset beside them. Every tooltip, menu entry and the Player keys dialog shows the
  keys as set (`lib/shortcuts.ts`); on macOS the menu bar gets them too.
- **Settings → General → Hardware acceleration** (`HardwareGroup.tsx`): "Where analysis runs on this computer." Two
  rows, the second only where it applies.
  - **Row 1, Graphics card** ("Chip" on a Mac): a select and one sentence that says what analysis runs on, and is
    the only place the device is named. Automatic, then each card when there are two or more, then Processor only;
    a Mac and a Windows computer with one card get Automatic and Processor only, Linux and a computer without a
    card no select. While it looks, the select is disabled and reads "Looking for graphics cards…" ("Checking the
    chip…"); if the check fails, a line says "Couldn't check the graphics cards." ("Couldn't check the chip.") with
    Try again. The choice applies from the next analysis, and the sentence says so until then. Other
    sentences: the chosen card isn't connected (Automatic stands in), some steps ran on the processor in the last
    analysis, or analysis runs on the processor and is much slower. While NVIDIA acceleration is in use it ends
    "with NVIDIA acceleration". Fast Scan's line in New project follows this choice without naming the card.
  - **Row 2, NVIDIA acceleration:** only on Windows (x64) where analysis runs on a GeForce RTX 30 card or newer,
    and wherever NVIDIA's files are, also when the card changed. It is the download's only home: no Activity job,
    no dialog; a toast says how it ended to someone who left Settings. One state at a time, the first that applies:
    - **Downloading:** a gold `Progress` bar, "45 of 111 MB" under it in the timecode face, and Cancel.
    - **Failed:** why, in plain words, and Try again, which fetches only what is missing. Five texts, from the
      code's messages: no internet connection; not enough free space (with how much to free); files that did not
      arrive as they should; NVIDIA having changed its files, so AutoScene needs an update; a cancelled repair
      ("analysis runs without NVIDIA acceleration"). Cancel during a repair keeps the verified files and shows
      Failed; Cancel during a first download or an update removes what was fetched and returns to Available.
    - **Removing:** "Removing after the current analysis." Windows can't delete a library an engine holds.
    - **Available:** what it is for, the size, that the Python Package Index sees the IP address, a link to
      NVIDIA's licence, and Agree and download as the one primary button. A new licence asks again here.
    - **Not used:** analysis is not running on a card the files serve, whichever way it got there (another card
      or the processor is chosen, Automatic resolves to another card, or the card changed); the switch is disabled
      and keeps the person's choice.
    - **Stopped working:** it failed twice in a row or doesn't load; analysis runs without it. Try again shows
      that it is checking.
    - **Needs a newer driver:** the card isn't listed by NVIDIA's engine; Get the driver opens NVIDIA's page.
    - **Ready:** a switch, on after a first download and the person's from then on. An update or a repair never
      flips it.
  - **Remove NVIDIA's files** is a quiet text button under the row's text, wherever files are except while
    downloading or removing. Its dialog asks first and says what it costs ("Analysis keeps running on this
    graphics card, more slowly") where the row is ready and the switch is on.
  - **Copy:** the row says NVIDIA, never TensorRT, CUDA or DirectML. Row 1 names no engine either. About → AI
    engine names ONNX Runtime with its version and, once downloaded, TensorRT for RTX with NVIDIA's licence and
    its Acknowledgements.

- **Settings → Discord:** one switch, Show on Discord, off until the person turns it on, then the two statuses people
  would see (in an episode, in a scenepack) as plain cards in AutoScene's own look, not a copy of Discord's, under
  "What people see", or "What people would see" while it is off. Once it is on, Names lists Project, Episode or file and
  Character: each off by default, and without one the status says what the person is doing ("Reviewing an episode", in
  a movie "Reviewing a file"), never on what. Names kept from before stay listed while it is off: turning it on sends
  them at once, so they can be checked and changed first. There is nothing else to set: the wording, the picture and
  the link are fixed. Discord not running shows nothing. Discord's mark is `brand/DiscordMark.tsx`, unaltered and only in colour,
  black or white, so it is white in the section list and black on the open section's gold.

## Copy

- Plain, confident, editor vocabulary: shot, cut, scene, cast, pack, export, handles, timecode.
- Never expose engine names in primary UI (CUDA, ONNX, TransNet, InsightFace, NVENC, ffmpeg). They belong in
  Settings → About only.
- No marketing tone, no emoji, no exclamation marks.

### Where the words live

The UI's words are in a catalog, not in the code, so AutoScene can speak more than English (issue #47).

- English is the source: `app/src/locales/en/<namespace>.ts`, `export default { … } as const`. Keys are
  `ns:area.item` in lowerCamelCase that name the role, not the wording; `_` is kept for plural forms
  (`_one`, `_few`, `_other`). A comment above a key is a note for the translator (button or title, what a
  `{{variable}}` holds, where it shows). `t()` is typed by these files: an unknown key, a missing or misspelled
  `{{variable}}` doesn't compile.
- Namespaces follow ownership: `common` (shared words and the `fmt` templates; changed only by phase 0), `shell`,
  `ui`, `shortcuts`, `settings`, `cast`, `project`, `library`, `media`, `player`, `pack`, `export`, `plugins`. All
  are registered in `locales/index.ts`; a sweep group fills its own file and never edits that one, nor `common`
  (a shared word it misses goes into its own namespace).
- Other languages are JSON next to it (`locales/ro/<namespace>.json`), loaded when chosen. English starts
  synchronously, so the first paint and every test have it with no provider.
- Components call `useTranslation(ns)`; code outside React (toasts, palette data, `lib/`) calls `t` from
  `@/lib/i18n` when it shows the words. Numbers, sizes, dates, lists and times go through `fmt` (`@/lib/i18n`):
  English reads as it always did (en-GB dates, "14,200", "1.5 KB"), another language writes them its way.

Rules for every word on screen:

- No translated text at module scope, in state or in query caches: keep keys and data, translate at render.
- A component that shows words calls `useTranslation()`, also when the words come from a helper.
- No sentence built from fragments (`` `Couldn't ${verb.toLowerCase()}` ``), no `n === 1 ?`: a whole message per
  case, plural keys for counts.
- No `"en-GB"`, `toLocaleString()` or `Intl.*Format` outside `lib/i18n.ts`: use `fmt`.
- React elements inside a sentence go through `<Trans>` with tags from `b strong em mono key x go`, never an HTML
  void name (`<link>`, `<br>`, `<img>`…), which renders empty.
- Plugin words and the user's data stay values inside a translated sentence; brand and product names (AniList,
  ProRes…) stay as they are. Nothing about a plugin is translated.
- No `t()` in per-frame paths: cache the words, keyed on the language.

The guard (`locales/hardcoded.test.ts`) refuses new words in the code: JSX text, text in JSX attributes, toast
arguments and options, the `label`/`title`/`description`/`hint`/`message`/`placeholder`/`heading`/`blurb`/`body`/
`question`/`detail`/`action`/`crumb` of objects, OS dialog filter names, text handed to `t()` as a value, both sides
of a `n === 1 ?`, calls of the deprecated `plural(`/`formatScreenTime(`, and locale-bound formatting. Where only
words go, one lower-case word counts too (`` `${n} shots` ``, `"faces"`). A word that is never translated gets
`// i18n-ignore: <why>` (or `{/* i18n-ignore: … */}`) on the line before. While the sweep moves today's words,
`locales/hardcoded/g1.json` … `g8.json` list what each group still has to move. A list only shrinks: the test fails
on a word no list has, on an entry the code no longer has, and on a list longer than it was when the guard came in;
each group deletes its file when done. `lib/format.ts`, `lib/ipc.ts` and `lib/i18n.ts` have no list: no word there.

`npm run i18n -- count` prints the inventory. `-- baseline` shrinks the lists to what the code still has and never
adds: a word it refuses goes into the catalog. (`-- baseline --init` rewrites them from the code; it is for a change
to the guard itself, never for a sweep or a feature.) `-- status` shows, per group, what its list still has and what
the guard lets through (returns, variables, call arguments, label maps); `-- unguarded <group>` lists the latter by
line, for the group to check before it deletes its file. `-- status --compare <rev>` lists English that left the
code since `<rev>` but is in no catalog as it was, which a reviewer checks byte for byte. `?lang=pseudo`
(development and the mock) shows every catalog word accented, longer and in brackets, and the values a message gets
plain: whatever reads plain is still in the code or is the person's data, whatever is cut off will be cut in a
longer language.
