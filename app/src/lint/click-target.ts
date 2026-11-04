// autoscene/click-target: every element a click acts on either is a control,
// whose cursor styles/base.css decides (the hand, or the arrow in menus and
// lists), or says with a cursor-* class what the pointer shows over it.
// oxlint JS plugin (alpha API, "not subject to semver"): if an oxlint update
// breaks it, the fallback is jsx-a11y's no-static-element-interactions and
// no-noninteractive-element-interactions with that plugin's other rules off.
// It reads what is written on the element: a handler that comes through {...props}
// or a component, and a key, drag or touch handler, are out of its sight.

type Node = { type: string; [key: string]: any }
type Context = {
  sourceCode: { getText(node: Node): string }
  report(r: { node: Node; messageId: string; data?: Record<string, string> }): void
}

/** What a click does: these handlers make an element a click target. */
export const HANDLERS = ["Click", "DoubleClick", "AuxClick", "MouseDown", "MouseUp", "PointerDown", "PointerUp"].flatMap((e) => [`on${e}`, `on${e}Capture`])
/** Tags whose cursor the browser or base.css decides (fields: the text cursor). */
export const CONTROL_TAGS = ["button", "input", "textarea", "select", "summary"]
/** Roles base.css gives the hand. Must equal the role list of its :where(). */
export const HAND_ROLES = ["button", "link", "tab", "switch", "checkbox", "radio", "slider"]
/** Roles base.css keeps at the arrow. Must equal the role list of its :not(). */
export const ARROW_ROLES = ["option", "menuitem", "menuitemcheckbox", "menuitemradio"]
/** `<motion.div>` renders a div: checked like one. */
const ANIMATED = new Set(["motion", "m"])

const ROLES = new Set([...HAND_ROLES, ...ARROW_ROLES])
const DOUBLE = ["onDoubleClick", "onDoubleClickCapture"]
// A press handler doesn't count here: it may do nothing a person sees, as one that only keeps the focus.
const SINGLE = ["onClick", "onClickCapture"]
/** Tailwind's cursor utilities. A class outside them (a typo) emits nothing, so it says nothing. */
export const CURSORS = [
  ...["auto", "default", "pointer", "wait", "text", "move", "help", "not-allowed", "none", "context-menu", "progress", "cell", "crosshair", "vertical-text"],
  ...["alias", "copy", "no-drop", "grab", "grabbing", "all-scroll", "zoom-in", "zoom-out"],
  ...["col", "row", "n", "e", "s", "w", "ne", "nw", "se", "sw", "ew", "ns", "nesw", "nwse"].map((d) => `${d}-resize`),
]
// One class with no variant in front (hover:, data-[…]:, group-hover:…), important or not.
const CURSOR_CLASS = new RegExp(`^!?cursor-(?:${CURSORS.join("|")}|\\[[^\\]]+\\]|\\([^)]+\\))!?$`)

function tagOf(name: Node): string | null {
  if (name.type === "JSXIdentifier") return /^[a-z]/.test(name.name) ? name.name : null
  if (name.type === "JSXMemberExpression" && name.object.type === "JSXIdentifier" && ANIMATED.has(name.object.name)) {
    return /^[a-z]/.test(name.property.name) ? name.property.name : null
  }
  return null
}

const valueOf = (attr: Node | undefined): Node | undefined =>
  attr?.value?.type === "JSXExpressionContainer" ? attr.value.expression : attr?.value
const isRole = (e: Node | undefined) => (e?.type === "Literal" || e?.type === "StringLiteral") && ROLES.has(e.value)
/** base.css wants a real href: one that can be undefined at runtime doesn't count. */
const isText = (e: Node | undefined) => (e?.type === "Literal" && typeof e.value === "string") || e?.type === "TemplateLiteral"
const isNothing = (e: Node | undefined) => (e?.type === "Identifier" && e.name === "undefined") || (e?.type === "Literal" && e.value === null)

/** Every string in a class attribute: what a literal, a template or a cn() call holds, comments not. */
function strings(n: unknown, out: string[] = []): string[] {
  if (Array.isArray(n)) n.forEach((c) => strings(c, out))
  else if (n && typeof n === "object" && "type" in n) {
    const node = n as Node
    if (node.type === "Literal" && typeof node.value === "string") out.push(node.value)
    else if (node.type === "TemplateElement") out.push(node.value.cooked ?? "")
    else for (const [key, child] of Object.entries(node)) if (key !== "parent") strings(child, out)
  }
  return out
}

/** Does this handler act exactly when the conditional role is a control's? Same test, written the same way. */
function gatedLike(role: Node, handler: Node | undefined, text: (n: Node) => string): boolean {
  if (handler?.type === "LogicalExpression") return handler.operator === "&&" && text(handler.left) === text(role.test) && isNothing(role.alternate)
  if (handler?.type !== "ConditionalExpression" || text(handler.test) !== text(role.test)) return false
  return isNothing(handler.consequent) === isNothing(role.consequent) && isNothing(handler.alternate) === isNothing(role.alternate)
}

/** A control's role, always, or on the same condition as every click handler on the element. */
function controlRole(e: Node | undefined, handlers: (Node | undefined)[], text: (n: Node) => string): boolean {
  if (isRole(e)) return true
  if (e?.type !== "ConditionalExpression") return false
  if (isRole(e.consequent) && isRole(e.alternate)) return true
  const oneSided = (isRole(e.consequent) && isNothing(e.alternate)) || (isNothing(e.consequent) && isRole(e.alternate))
  return oneSided && handlers.every((h) => gatedLike(e, h, text))
}

export default {
  meta: { name: "autoscene" },
  rules: {
    "click-target": {
      meta: {
        type: "suggestion",
        messages: {
          cursor:
            "A click acts on this <{{tag}}>: make it a button or a link (or give it a control's role, on the same condition as the handler), or say with a cursor-* class what the pointer shows over it.",
          doubleClick:
            "Only a double-click acts on this <{{tag}}>: say with a cursor-* class what the pointer shows (cursor-default, as over a file in a native list), or let a single click do something.",
        },
      },
      create(context: Context) {
        return {
          JSXOpeningElement(node: Node) {
            const tag = tagOf(node.name)
            if (!tag) return
            const attrs: Node[] = node.attributes.filter((a: Node) => a.type === "JSXAttribute" && a.name.type === "JSXIdentifier")
            const attr = (name: string) => attrs.find((a) => a.name.name === name)
            const has = (name: string) => !!attr(name)
            if (!HANDLERS.some(has)) return
            const statesCursor = strings(attr("className")?.value).some((s) => s.split(/\s+/).some((c) => CURSOR_CLASS.test(c)))
            if (DOUBLE.some(has) && !SINGLE.some(has)) {
              if (!statesCursor) context.report({ node, messageId: "doubleClick", data: { tag } })
              return
            }
            if (CONTROL_TAGS.includes(tag) || (tag === "a" && isText(valueOf(attr("href"))))) return
            const handlers = HANDLERS.filter(has).map((h) => valueOf(attr(h)))
            if (controlRole(valueOf(attr("role")), handlers, (n) => context.sourceCode.getText(n))) return
            if (!statesCursor) context.report({ node, messageId: "cursor", data: { tag } })
          },
        }
      },
    },
  },
}
