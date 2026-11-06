// Finds the words written into the UI's code instead of the catalog. Shared
// by the guard (hardcoded.test.ts) and `npm run i18n` (scripts/i18n.ts), so
// keep it to erasable TypeScript: Node runs the script by stripping types.
//
// Two views of one walk over Vite's own parser (oxc):
// - `guarded` hits are what the test refuses: JSX text, text in JSX
//   attributes, toasts, the UI-ish props of objects (`label`, `title`, …), OS
//   dialog filter names, values handed to t(), the two sides of a `n === 1 ?`
//   plural, and locale-bound formatting (`"en-GB"`, `toLocaleString(`, `toFixed(`, `Intl.*Format(`)
//   outside lib/i18n.ts;
// - every hit is the inventory `npm run i18n -- count` prints; `npm run i18n --
//   unguarded` lists the ones the guard lets through.
import { parseSync } from "vite"

/** A path under app/src, with forward slashes. */
export type File = string
export type Hit = { text: string; line: number; ctx: string; guarded: boolean }

/** Files the guard reads: the UI's own code, not tests, mocks, generated bindings, the plugin kit, the catalogs or the lint rules. */
export function inScope(file: File): boolean {
  return (
    /\.(ts|tsx)$/.test(file) &&
    !/\.test\.(ts|tsx)$/.test(file) &&
    !/\.d\.ts$/.test(file) &&
    !/^(test|mocks|plugin-kit|locales|lint)\//.test(file) &&
    file !== "bindings.ts"
  )
}

// Words AutoScene never translates. A text made only of these, digits,
// symbols and episode codes (S01E03) is not a hit.
const NEVER_TRANSLATED = [
  "AutoScene", "AniList", "TVmaze", "Fandom", "Wikidata", "InsightFace", "Scener", "ProRes", "DNxHR", "DNxHD",
  "H.264", "H.265", "HEVC", "AV1", "VP9", "WebM", "MKV", "MOV", "MP4", "Dolby Vision", "Dolby Atmos", "Dolby",
  "HDR10+", "HDR10", "HDR", "SDR", "FFV1", "NVENC", "CUDA", "GitHub",
]
const NEVER_RE = new RegExp(
  `${NEVER_TRANSLATED.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")}|\\bS\\d+E\\d+\\b`,
  "g",
)

const SKIP_CALLEES = new Set([
  "cn", "clsx", "cva", "twMerge", "invoke", "listen", "emit", "require", "import", "querySelector",
  "querySelectorAll", "getElementById", "addEventListener", "removeEventListener", "setAttribute", "getAttribute",
  "getPropertyValue", "setProperty", "matchMedia", "createElement", "startsWith", "endsWith", "includes", "split",
  "replace", "replaceAll", "indexOf", "join", "padStart", "getItem", "setItem", "removeItem", "has", "get", "set",
  "delete", "Symbol", "useQuery", "describe", "it", "test", "expect", "parseInt", "Number", "RegExp", "fetch",
  "postMessage", "dispatchEvent", "CustomEvent", "Event", "KeyboardEvent", "MouseEvent", "t", "useTranslation",
])
// Inventory: object props that never hold words.
const SKIP_PROPS = new Set([
  "className", "class", "variant", "size", "tone", "kind", "id", "key", "type", "role", "side", "align", "to", "href",
  "src", "path", "icon", "queryKey", "mode", "status", "state", "stage", "cursor", "transition", "ease", "easing",
  "style", "color", "background", "fontFamily", "transform", "boxShadow", "gridTemplateColumns", "width", "height",
  "slot", "code", "value", "event", "command", "keys", "combo", "scope", "dir", "objectFit", "objectPosition",
  "mixBlendMode", "maskImage", "WebkitMaskImage", "filter", "backdropFilter", "inset", "top", "left", "right",
  "bottom", "margin", "padding", "display", "position", "zIndex", "opacity", "animation", "fill", "stroke", "d",
  "viewBox", "preset", "encoder", "codec", "container", "ext", "format", "i18nKey", "ns", "context", "lng", "lngs",
  "keyPrefix", "defaultValue", "duration",
])
// Guard: object props that hold words a person reads.
const WORD_PROPS = new Set([
  "label", "title", "description", "hint", "message", "placeholder", "heading", "blurb", "body", "question", "detail",
  "action", "crumb",
])
// JSX attributes that only ever hold words a person reads or hears: there a
// single lower-case word counts too ("episodes"), as in JSX text and toasts.
const WORD_ATTR =
  /^(aria-(label|description|valuetext|roledescription|placeholder)|title|alt|placeholder|label|values|\w*(Label|Title|Text|Hint|Placeholder|Description|Caption|Tooltip|Message|Heading))$/
// JSX attributes that never hold words.
const SKIP_ATTRS = new Set([
  "className", "class", "variant", "size", "tone", "id", "key", "type", "role", "side", "align", "to", "href", "src",
  "htmlFor", "name", "autoComplete", "inputMode", "spellCheck", "d", "viewBox", "fill", "stroke", "strokeWidth",
  "transform", "points", "x", "y", "width", "height", "preserveAspectRatio", "mode", "orientation", "dir",
  "sideOffset", "aria-hidden", "aria-live", "aria-current", "aria-orientation", "aria-haspopup", "aria-controls",
  "aria-describedby", "aria-labelledby", "kind", "slot", "icon", "value", "defaultValue", "accept", "loading",
  "decoding", "crossOrigin", "preload", "rel", "target", "ease", "layout", "initial", "animate", "exit", "cursor",
  "as", "asChild", "draggable", "tabIndex", "i18nKey", "ns", "lang", "style",
])

function isClassLike(s: string): boolean {
  const toks = s.trim().split(/\s+/)
  if (/[A-Z]/.test(s) && !/[a-z]-[A-Z]|\[/.test(s)) return false
  return toks.every((t) => /^[!a-z0-9@:[\]/().%#_&>~*=+,'"-]+$/.test(t)) && toks.some((t) => /[-:[/]/.test(t))
}

/**
 * Whether a literal reads like words for people rather than a class list, key,
 * path or identifier. `loose` is for places that only ever hold words (JSX,
 * toasts, word props and attributes, t() values, plurals): there a single
 * lower-case word ("episodes", " fps") is words too, and in a plural one
 * letter ("s") is.
 */
export function isProse(s: string, ctx: string, loose = false): boolean {
  const t = s.trim()
  const plural = ctx === "plural"
  if (plural ? !/[A-Za-z]/.test(t) : t.length < 2 || !/[A-Za-z]{2,}/.test(t)) return false
  if (!/\p{L}/u.test(t.replace(NEVER_RE, ""))) return false
  if (ctx === "jsx-text") return true
  if (/^(https?:|file:|asset:|data:|\/|\.\/|\.\.\/|@\/|#|--|var\(|rgba?\(|hsla?\(|calc\(|url\(|linear-gradient|radial-gradient|cubic-bezier)/.test(t)) return false
  if (/^\{.*\}$/.test(t) || isClassLike(t)) return false
  if (/^[A-Z0-9_]+$/.test(t) && !plural) return false
  if (loose) {
    // Still not words: a key or a dotted path (`cast:review.title`), camelCase, a CSS length.
    if (/^[A-Za-z][\w-]*([.:][\w-]+)+$/.test(t) || /^[a-z]+[A-Z]\w*$/.test(t) || /^-?[\d.]+(px|rem|em|ms|s|%)$/.test(t)) return false
    return true
  }
  if (/^[a-z][a-zA-Z0-9_.:-]*$/.test(t)) return false
  if (/^(Escape|Enter|Tab|Backspace|Delete|Arrow(Up|Down|Left|Right)|Home|End|Page(Up|Down)|Space|Shift|Control|Alt|Meta|Mod|CmdOrCtrl|Key[A-Z]|Digit\d|F\d+)$/.test(t)) return false
  if (/^\d/.test(t) && !/\s[a-z]{3,}/.test(t)) return false
  if (/^[A-Z][a-zA-Z]+$/.test(t)) return true
  if (/\s/.test(t)) return true
  return /[.!?…]$/.test(t)
}

type Node = { type: string; start: number; end: number; [k: string]: unknown }
const isNode = (v: unknown): v is Node => typeof v === "object" && v !== null && typeof (v as Node).type === "string"

function calleeName(c: unknown): string {
  if (!isNode(c)) return ""
  if (c.type === "Identifier") return c.name as string
  if (c.type === "MemberExpression") {
    const obj = calleeName(c.object)
    const prop = isNode(c.property) && c.property.type === "Identifier" ? (c.property.name as string) : ""
    return obj ? `${obj}.${prop}` : prop
  }
  return ""
}
function attrName(n: unknown): string {
  if (!isNode(n)) return ""
  if (n.type === "JSXIdentifier") return n.name as string
  if (n.type === "JSXNamespacedName") return `${(n.namespace as Node).name}:${(n.name as Node).name}`
  return ""
}
function propKey(k: unknown): string {
  if (!isNode(k)) return ""
  if (k.type === "Identifier") return k.name as string
  if (k.type === "Literal") return String(k.value)
  return ""
}

type Ctx = { ctx: string; guarded: boolean; loose: boolean }
const unguarded = (ctx: string): Ctx => ({ ctx, guarded: false, loose: false })

// `n === 1 ? "episode" : "episodes"`, `count > 1 ? "s" : ""`: an English plural built in the code.
function isPluralTest(test: unknown): boolean {
  if (!isNode(test) || test.type !== "BinaryExpression") return false
  if (!["===", "!==", "==", "!=", ">", ">=", "<", "<="].includes(test.operator as string)) return false
  const one = (n: unknown) => isNode(n) && n.type === "Literal" && n.value === 1
  return one(test.left) || one(test.right)
}

// Classifies a literal by its nearest deciding ancestor (`stack` = root … parent).
// A property whose key decides nothing (`text`, `loading`, `what`) defers to
// what holds the object, so the options of a toast or of t() are guarded too.
// A literal on either side of a `n === 1 ?` is a plural wherever it is, unless
// a technical place (a class list, a comparison, a key) holds it.
function classify(stack: Node[], self: Node): Ctx | null {
  let plural = false
  const found = (c: Ctx | null): Ctx | null => (c === null || !plural ? c : { ctx: "plural", guarded: true, loose: true })
  for (let i = stack.length - 1; i >= 0; i--) {
    const a = stack[i]
    const child = stack[i + 1] ?? self
    switch (a.type) {
      case "ImportDeclaration": case "ExportNamedDeclaration": case "ExportAllDeclaration": case "ImportExpression":
        if (child === a.source) return null
        break
      case "TSLiteralType": case "TSEnumMember": case "TSPropertySignature": case "TSTypeAnnotation":
      case "TSTypeReference": case "TSTypeAliasDeclaration": case "TSIndexedAccessType":
        return null
      case "SwitchCase":
        if (child === a.test) return null
        break
      case "BinaryExpression":
        if (["===", "!==", "==", "!=", "in", "instanceof"].includes(a.operator as string)) return null
        break
      case "ConditionalExpression":
        if (child !== a.test && isPluralTest(a.test)) plural = true
        break
      case "MemberExpression":
        if (a.computed && child === a.property) return null
        break
      case "JSXAttribute": {
        const n = attrName(a.name)
        if (SKIP_ATTRS.has(n) || n.startsWith("data-") || /^on[A-Z]/.test(n)) return null
        return found({ ctx: `attr:${n}`, guarded: true, loose: WORD_ATTR.test(n) })
      }
      case "JSXExpressionContainer": {
        const p = stack[i - 1]
        if (p && (p.type === "JSXElement" || p.type === "JSXFragment")) return found({ ctx: "jsx-expr", guarded: true, loose: true })
        break
      }
      case "CallExpression": case "NewExpression": {
        if (child === a.callee) break
        const n = calleeName(a.callee)
        const last = n.split(".").pop() ?? ""
        if (/^toast(\.|$)/.test(n) || /[tT]oast$/.test(n)) return found({ ctx: "toast", guarded: true, loose: true })
        // t("key", { what: "the whole project" }): a fragment handed to a message as a value.
        if (last === "t") return child === (a.arguments as Node[])[0] ? null : found({ ctx: "t-value", guarded: true, loose: true })
        if (n === "Error" || n.endsWith("Error")) return found(unguarded("error"))
        if (n.startsWith("console.") || SKIP_CALLEES.has(n) || SKIP_CALLEES.has(last)) return null
        return found(unguarded(`call:${last}`))
      }
      case "Property": {
        if (child === a.key) return null
        const k = propKey(a.key)
        if (k === "name" && isNode(stack[i - 2]) && stack[i - 2].type === "ArrayExpression") {
          const owner = stack[i - 3]
          if (isNode(owner) && owner.type === "Property" && propKey(owner.key) === "filters") {
            return found({ ctx: "prop:filters.name", guarded: true, loose: true })
          }
        }
        // `action` also names a command (`action: "undo"`), so only a sentence counts there.
        if (WORD_PROPS.has(k)) return found({ ctx: `prop:${k}`, guarded: true, loose: k !== "action" })
        if (SKIP_PROPS.has(k)) return null
        // The key decides nothing: what holds the object does (a toast, t(), a
        // JSX attribute); held by nothing that guards, it is `prop:<key>`.
        const outer = classify(stack.slice(0, i), a)
        if (outer === null) return null
        return found(outer.guarded ? outer : unguarded(`prop:${k}`))
      }
      case "ThrowStatement":
        return found(unguarded("error"))
      case "ReturnStatement":
        return found(unguarded("return"))
      case "VariableDeclarator":
        return found(unguarded("var"))
      case "AssignmentExpression":
        return found(unguarded("assign"))
      default:
        break
    }
  }
  return found(unguarded("other"))
}

const LOCALE_CALL = /^(toLocale(Date|Time)?String|toFixed|Intl\.\w+Format)$/

/** Every hit in one file's source; `guarded` marks those the test refuses (`i18n-ignore` clears it). */
export function scan(file: File, code: string): Hit[] {
  const { program, comments } = parseSync(file, code, { lang: file.endsWith(".tsx") ? "tsx" : "ts" })
  const starts = [0]
  for (let i = 0; i < code.length; i++) if (code[i] === "\n") starts.push(i + 1)
  const lineOf = (pos: number) => {
    let lo = 0
    let hi = starts.length - 1
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1
      if (starts[mid] <= pos) lo = mid
      else hi = mid - 1
    }
    return lo + 1
  }
  // `// i18n-ignore: <why>` (or `{/* i18n-ignore: … */}`) excuses a hit on the
  // next line, or later on its own line.
  const ignores = comments.filter((c) => /^\s*i18n-ignore:\s*\S/.test(c.value)).map((c) => ({ line: lineOf(c.end), end: c.end }))
  const ignored = (line: number, at: number) => ignores.some((c) => c.line === line - 1 || (c.line === line && c.end <= at))
  const localeFree = file === "lib/i18n.ts"

  const hits: Hit[] = []
  const add = (text: string, at: number, ctx: string, guarded: boolean) => {
    const line = lineOf(at)
    hits.push({ text, line, ctx, guarded: guarded && !ignored(line, at) })
  }
  const stack: Node[] = []
  const visit = (node: unknown): void => {
    if (Array.isArray(node)) {
      for (const n of node) visit(n)
      return
    }
    if (!isNode(node)) return
    if (node.type === "JSXText") {
      const raw = node.value as string
      const text = raw.replace(/\s+/g, " ").trim()
      if (text && isProse(text, "jsx-text")) add(text, node.start + raw.search(/\S/), "jsx-text", true)
      return
    }
    if (node.type === "Literal" && typeof node.value === "string") {
      if (node.value === "en-GB" && !localeFree) add("en-GB", node.start, "locale", true)
      const c = classify(stack, node)
      if (c && isProse(node.value, c.ctx, c.loose)) add(node.value, node.start, c.ctx, c.guarded)
      return
    }
    if (node.type === "TemplateLiteral") {
      const quasis = node.quasis as Node[]
      const exprs = node.expressions as Node[]
      const text = quasis.map((q, i) => (q.value as { cooked: string }).cooked + (i < exprs.length ? "{x}" : "")).join("")
      const words = text.replaceAll("{x}", " ").trim()
      const c = classify(stack, node)
      if (c && words && isProse(words, c.ctx, c.loose)) add(text, node.start, c.ctx, c.guarded)
    }
    if ((node.type === "CallExpression" || node.type === "NewExpression") && !localeFree) {
      const name = calleeName(node.callee)
      const last = name.startsWith("Intl.") ? name : (name.split(".").pop() ?? "")
      if (LOCALE_CALL.test(last)) add(`${last}(`, node.start, "locale", true)
    }
    stack.push(node)
    for (const [k, v] of Object.entries(node)) if (k !== "type" && k !== "start" && k !== "end") visit(v)
    stack.pop()
  }
  visit(program)
  return hits
}

/** What the guard refuses: every guarded hit of a scan, with its file. */
export function refused(hits: Record<File, Hit[]>): (Hit & { file: File })[] {
  return Object.entries(hits).flatMap(([file, list]) => list.filter((h) => h.guarded).map((h) => ({ ...h, file })))
}
