// Checks a catalog's values can render: tags `<Trans>` can fill, no empty
// text, and a plugin can't lead its own toast. The full per-language checks
// (keys, plural forms, variables, meta hashes) join with the Romanian catalog
// (docs/DESIGN.md, Copy).

/** Tag names a message may use for `<Trans components>`. An HTML void name (`<link>`, `<br>`) would render empty. */
export const TRANS_TAGS = ["b", "strong", "em", "mono", "key", "x", "go"] as const

/**
 * Names a message may use only self-closed (`<what/>`), where the code places a word that is not its own to
 * translate and that may hold anything, `{{braces}}` included: a plugin's name, a repository's path. A
 * `{{value}}` would be filled in twice by `<Trans>`, and so change such a word.
 */
export const SLOT_TAGS = ["what", "repo", "commit", "path"] as const

/** Toasts that say a plugin's words: the key, with its namespace first. */
const PLUGIN_TOAST = /^(backend\.plugin\.pageToast$|plugins\.toast\.\w+\.)/

/**
 * What lets a plugin pass for AutoScene in the toast `value`. A plugin may be
 * called anything, so the toast opens with words it can't choose, then its
 * name before anything else of its own. An `anonymous` toast has no name.
 */
function toastProblems(value: string, key: string): string[] {
  const problems: string[] = []
  if (!/^[^{$]*\p{L}/u.test(value)) problems.push(`${key}: starts with a plugin's words, not with fixed text`)
  const [first] = [...value.matchAll(/\{\{\s*(\w+)/g)].map((m) => m[1])
  if (!key.includes(".anonymous") && first !== "name") problems.push(`${key}: {{name}} must come before any other variable`)
  return problems
}

/**
 * Every problem in a catalog's values, as `"key: what"`. Give it a language's
 * whole catalog, `{ backend: …, plugins: … }`, so a key has its namespace.
 */
export function catalogProblems(bundle: unknown, prefix = ""): string[] {
  if (typeof bundle === "string") {
    const problems: string[] = []
    if (bundle.trim() === "") problems.push(`${prefix}: empty`)
    for (const [tag, name] of bundle.matchAll(/<\/?([A-Za-z][\w-]*)[^>]*>/g)) {
      const slot = tag.endsWith("/>") && (SLOT_TAGS as readonly string[]).includes(name)
      if (!slot && !(TRANS_TAGS as readonly string[]).includes(name)) problems.push(`${prefix}: <${name}> is not a Trans tag (${TRANS_TAGS.join(" ")})`)
    }
    if (PLUGIN_TOAST.test(prefix)) problems.push(...toastProblems(bundle, prefix))
    return problems
  }
  if (bundle && typeof bundle === "object") {
    return Object.entries(bundle).flatMap(([k, v]) => catalogProblems(v, prefix ? `${prefix}.${k}` : k))
  }
  return [`${prefix}: not text`]
}
