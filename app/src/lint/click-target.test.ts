// autoscene/click-target on its cases, and against the cursor rule in styles/base.css it stands in for.
import baseCss from "@/styles/base.css?raw"
import { RuleTester } from "oxlint/plugins-dev"
import { describe, expect, it } from "vitest"
import { compile } from "tailwindcss"
import plugin, { ARROW_ROLES, CONTROL_TAGS, CURSORS, HAND_ROLES } from "./click-target.ts"

RuleTester.describe = describe
RuleTester.it = it

const filename = "a.tsx"
const flagged = (code: string, messageId: "cursor" | "doubleClick" = "cursor") => ({ code, filename, errors: [{ messageId }] })

new RuleTester({ languageOptions: { parserOptions: { ecmaFeatures: { jsx: true } } } }).run(
  "click-target",
  plugin.rules["click-target"] as never,
  {
    valid: [
      "<button onClick={f}>Go</button>",
      "<a href='#/x' onClick={f}>Go</a>",
      "<a href={`#/${id}`} onClick={f}>Go</a>",
      "<select onClick={f} />",
      "<summary onClick={f}>x</summary>",
      "<input onClick={f} />",
      "<textarea onClick={f} />",
      "<li role={ok ? 'link' : undefined} onClick={ok ? f : undefined}>Go</li>",
      "<li role={ok ? 'link' : null} onClick={ok ? f : null}>Go</li>",
      "<li role={ok ? undefined : 'link'} onClick={ok ? undefined : f}>Go</li>",
      "<li role={ok ? 'link' : undefined} onClick={ok && f}>Go</li>",
      "<li role={ok ? 'link' : 'button'} onClick={f}>Go</li>",
      "<div role='menuitem' onClick={f}>Copy</div>",
      "<div className='cursor-default' onClick={stop}>x</div>",
      "<div className={cn('h-5', hovered && 'cursor-pointer')} onClick={f}>x</div>",
      "<div className={cn({ 'cursor-pointer': hovered })} onClick={f}>x</div>",
      "<div className={`h-5 ${x} cursor-grab`} onClick={f}>x</div>",
      "<div className='cursor-[grab]' onClick={f}>x</div>",
      "<div className='cursor-(--hand)' onClick={f}>x</div>",
      "<div className='!cursor-default' onClick={f}>x</div>",
      "<div className='cursor-default!' onClick={f}>x</div>",
      "<button className='block cursor-default' onDoubleClick={f}>x</button>",
      "<button onClick={pick} onDoubleClick={open}>x</button>",
      "<button onClickCapture={pick} onDoubleClick={open}>x</button>",
      "<Item onClick={f} />",
      "<motion.Item onClick={f} />",
      "<motion.div layout>x</motion.div>",
    ].map((jsx) => ({ code: `const a = ${jsx}`, filename })),
    invalid: [
      flagged("const a = <div onClick={f}>x</div>"),
      flagged("const a = <li onClick={ready ? open : undefined}>x</li>"),
      flagged("const a = <div role='group' onDoubleClick={f} onClick={g}>x</div>"),
      ...["Click", "AuxClick", "MouseDown", "MouseUp", "PointerDown", "PointerUp"]
        .flatMap((e) => [`on${e}`, `on${e}Capture`])
        .map((h) => flagged(`const a = <span ${h}={f}>x</span>`)),
      flagged("const a = <a onClick={f}>x</a>"),
      flagged("const a = <a href={undefined} onClick={f}>x</a>"),
      flagged("const a = <a href={url} onClick={f}>x</a>"),
      flagged("const a = <label onClick={f}>x</label>"),
      flagged("const a = <motion.div onClick={f}>x</motion.div>"),
      flagged("const a = <m.div onClick={f}>x</m.div>"),
      flagged("const a = <li role={ok ? 'link' : undefined} onClick={f}>x</li>"),
      flagged("const a = <li role={ok ? 'link' : undefined} onClick={other ? f : undefined}>x</li>"),
      flagged("const a = <li role={ok ? 'link' : undefined} onClick={ok ? f : g}>x</li>"),
      flagged("const a = <li role={ok ? 'link' : undefined} onClick={ok ? undefined : f}>x</li>"),
      flagged("const a = <li role={ok ? 'link' : undefined} onClick={ok || f}>x</li>"),
      flagged("const a = <li role={ok ? undefined : 'link'} onClick={ok && f}>x</li>"),
      flagged("const a = <li role={ok ? 'link' : undefined} onClick={ok ? f : undefined} onMouseUp={g}>x</li>"),
      flagged("const a = <li role={ok ? 'link' : 'group'} onClick={ok ? f : undefined}>x</li>"),
      flagged("const a = <li role={ok ? 'group' : undefined} onClick={ok ? f : undefined}>x</li>"),
      flagged("const a = <div className='data-[x]:cursor-default group-hover:cursor-pointer' onClick={f}>x</div>"),
      flagged("const a = <div className={cn(/* cursor-default */ 'x')} onClick={f}>x</div>"),
      flagged("const a = <div className='cursor-foo' onClick={f}>x</div>"),
      flagged("const a = <div className='cursor-pionter' onClick={f}>x</div>"),
      flagged("const a = <button onDoubleClick={open}>x</button>", "doubleClick"),
      flagged("const a = <button onDoubleClickCapture={open}>x</button>", "doubleClick"),
      flagged("const a = <button onDoubleClick={open} onMouseDown={keepFocus}>x</button>", "doubleClick"),
      flagged("const a = <button onDoubleClick={open} onPointerDown={keepFocus}>x</button>", "doubleClick"),
    ],
  },
)

/** The items of a selector list: its top-level commas only. */
function items(list: string): string[] {
  const out = [""]
  let depth = 0
  for (const c of list) {
    if ("([".includes(c)) depth++
    if (")]".includes(c)) depth--
    if (c === "," && depth === 0) out.push("")
    else out[out.length - 1] += c
  }
  return out.map((s) => s.trim()).filter(Boolean)
}

const roles = (list: string[]) => list.flatMap((s) => s.match(/^\[role="([a-z]+)"\]$/)?.[1] ?? []).sort()

describe("the rule and base.css agree", () => {
  const css = baseCss.replace(/\/\*[\s\S]*?\*\//g, "")
  const rule = css.match(/:where\(([^{]*)\):not\(([^{]*)\)\s*\{\s*cursor:\s*pointer;/)
  if (!rule) throw new Error("the hand rule in base.css isn't where this test reads it: reshaped, this reader goes with it")
  const hand = items(rule[1])

  it("on which roles show the hand and which the arrow", () => {
    expect(roles(hand)).toEqual([...HAND_ROLES].sort())
    expect(roles(items(rule[2]))).toEqual([...ARROW_ROLES].sort())
  })

  it("on which tags are controls", () => {
    const heads = new Set(hand.flatMap((s) => s.match(/^[a-z]+/)?.[0] ?? []))
    // `a` counts only with its href, in the rule too. A label counts only around a control,
    // so the rule doesn't take one as a control. Both ways: a tag on one side only fails.
    expect(hand).toContain("a[href]")
    expect(heads).toContain("label")
    heads.delete("a")
    heads.delete("label")
    // A field keeps the browser's text cursor, so base.css has no item for it.
    expect([...heads].sort()).toEqual(CONTROL_TAGS.filter((t) => t !== "textarea").sort())
  })
})

describe("the rule and Tailwind agree", () => {
  it("on which cursor-* classes emit a cursor", async () => {
    const { build } = await compile("@tailwind utilities;")
    const css = build([...CURSORS.map((c) => `cursor-${c}`), "cursor-foo"])
    for (const c of CURSORS) expect(css).toContain(`.cursor-${c} {`)
    expect(css).not.toContain("cursor-foo")
  })
})
