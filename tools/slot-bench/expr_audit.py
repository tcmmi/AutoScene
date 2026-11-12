"""A person's audit of the expressions check sets: a page to answer on, and the comparison.

    uv run expr_audit.py page <out_dir> <name>=<crops_dir>,<check.tsv> [<name>=… …]
    uv run expr_audit.py compare <answers.tsv>

`page` draws 100 labelled crops, spread evenly over the episodes, and writes
`<out_dir>/audit.html`: one file with the crops inside it, to open in a
browser on this computer (the crops are faces, so it stays out of the
repository and is not sent anywhere). Per episode the draw takes up to two
crops of every label, then four that show nothing, four `unclear` and two
`noface`, then fills up at random; no two of one track while others are
left. The page shows one crop at a time and asks what is to be seen, without
showing what the labellers said; the answers stay in the browser as one
goes, and at the end the page lists where they differ and offers them as
`expr_audit_answers.tsv` (`set file owner agents a b c`).
`<out_dir>/sample.tsv` names the crops drawn.

`compare` reads that file: per label how often the person and the labels
say it, how often both, and kappa; how many crops are answered alike.
"""
import base64
import csv
import json
import os
import random
import sys
import zlib

import numpy as np

import expr_scores as es

TOTAL, PER_LABEL, PLAIN = 100, 2, {"none": 4, "unclear": 4, "noface": 2}
MEANING = {
    "smile": "corners of the mouth pulled up; mouth closed or a little open; a grin counts",
    "laugh": "a laughing face: wide open smiling mouth, cheeks up, eyes narrowed or shut (then not also smile)",
    "cry": "tears or wet streaks, or a face screwed up in crying",
    "angry": "brows lowered and drawn together, a hard stare, lips pressed or teeth bared",
    "shout": "mouth wide open in a shout or scream, the face tense (not a laugh, a yawn or plain talking)",
    "surprised": "brows raised and eyes wide open; the mouth often open",
    "downcast": "a sad, dejected look: mouth corners down, inner brows raised, eyes lowered",
    "blush": "drawn or clearly visible red cheeks",
    "eyes_closed": "both eyes shut",
    "mouth_open": "the lips clearly parted, for whatever reason",
}
PAGE = r"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Expression labels: audit</title>
<style>
:root { color-scheme: light dark; --bg: #f4f2ed; --fg: #1d1c1a; --mut: #6a665d; --card: #fffefb; --line: #d8d4c9;
  --on: #1e6b5a; --on-fg: #fff; --alt: #8a4a1c; --alt-fg: #fff; }
@media (prefers-color-scheme: dark) { :root { --bg: #141413; --fg: #ece9e2; --mut: #a09c91; --card: #1e1e1c;
  --line: #383630; --on: #46ab92; --on-fg: #0c1512; --alt: #e39a62; --alt-fg: #1b0f05; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.5 system-ui, "Segoe UI", sans-serif; }
main { max-width: 760px; margin: 0 auto; padding: 24px 16px 64px; }
h1 { font-size: 1.5rem; margin: 0 0 4px; text-wrap: balance; }
h2 { font-size: 1.1rem; margin: 28px 0 8px; }
p { margin: 0 0 12px; max-width: 65ch; }
.mut { color: var(--mut); }
.bar { height: 6px; border-radius: 3px; background: var(--line); overflow: hidden; margin: 12px 0 4px; }
.bar div { height: 100%; width: 0; background: var(--on); transition: width .15s; }
.card { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 16px; margin-top: 16px; }
.pic { display: block; width: min(100%, 384px); aspect-ratio: 1; margin: 0 auto 16px; border-radius: 8px;
  background: #000; image-rendering: auto; }
.chips { display: flex; flex-wrap: wrap; gap: 8px; }
button { font: inherit; color: var(--fg); background: transparent; border: 1px solid var(--line); border-radius: 999px;
  padding: 8px 14px; min-height: 44px; cursor: pointer; }
button:hover { border-color: var(--fg); }
button:focus-visible { outline: 2px solid var(--on); outline-offset: 2px; }
button[aria-pressed="true"] { background: var(--on); color: var(--on-fg); border-color: var(--on); }
.plain button[aria-pressed="true"] { background: var(--alt); color: var(--alt-fg); border-color: var(--alt); }
kbd { font: 12px ui-monospace, Consolas, monospace; opacity: .7; margin-left: 6px; }
.plain { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--line); }
.nav { display: flex; justify-content: space-between; gap: 8px; margin-top: 16px; }
.nav button { border-radius: 8px; min-width: 110px; }
.nav .go { background: var(--fg); color: var(--bg); border-color: var(--fg); }
dl { margin: 0; display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; }
dt { font-weight: 600; } dd { margin: 0; color: var(--mut); }
details { margin-top: 20px; } summary { cursor: pointer; }
.wide { overflow-x: auto; }
table { border-collapse: collapse; font-variant-numeric: tabular-nums; }
th, td { text-align: left; padding: 4px 16px 4px 0; border-bottom: 1px solid var(--line); white-space: nowrap; }
td.n, th.n { text-align: right; }
.diff { display: grid; grid-template-columns: 96px 1fr; gap: 12px; align-items: center; padding: 8px 0;
  border-bottom: 1px solid var(--line); }
.diff img { width: 96px; height: 96px; border-radius: 6px; background: #000; }
textarea { width: 100%; height: 120px; font: 12px ui-monospace, Consolas, monospace; background: var(--card);
  color: var(--fg); border: 1px solid var(--line); border-radius: 8px; padding: 8px; }
[hidden] { display: none !important; }
</style>
</head>
<body>
<main>
<h1>Expression labels: audit</h1>
<p class="mut">__COUNT__ face crops. Tick what you can <strong>see</strong> on the face in the middle of the crop, in this one
picture: not what the person feels, not what the story is. Several at once are normal. Be strict: when in doubt, leave it
out. What the labellers said stays hidden until the end. Your answers are kept in this browser as you go.</p>
<div class="bar"><div id="fill"></div></div>
<p class="mut" id="count"></p>

<section class="card" id="ask">
  <img class="pic" id="pic" alt="A face crop">
  <div class="chips" id="chips"></div>
  <div class="chips plain" id="plain"></div>
  <div class="nav">
    <button id="back">Back</button>
    <button id="end">Finish</button>
    <button class="go" id="next">Next <kbd>Enter</kbd></button>
  </div>
</section>

<details>
  <summary>What the labels mean</summary>
  <dl id="meaning"></dl>
  <p class="mut" style="margin-top:12px"><strong>None of these</strong>: a face that shows none of them (calm, plain talking).
  <strong>Can't judge</strong>: there is a face, but it is too dark, blurred, small, turned away or covered.
  <strong>Not a face</strong>: the middle of the crop holds no face at all. Keys: 1 to 0 for the labels in their order,
  N, U, F for the three below them, Enter or the right arrow for the next crop, the left arrow to go back.</p>
</details>

<section id="result" hidden>
  <h2>Result</h2>
  <p id="alike"></p>
  <div class="wide"><table id="table"></table></div>
  <h2>Save your answers</h2>
  <p>Download the file and tell Claude it is there (it lands in your Downloads folder as
  <code>expr_audit_answers.tsv</code>), or copy the text below into the chat.</p>
  <p><button id="save" style="border-radius:8px">Download answers</button>
  <button id="resume" style="border-radius:8px">Back to the crops</button></p>
  <textarea id="text" readonly aria-label="Your answers as text"></textarea>
  <h2>Where you and the labellers differ</h2>
  <div id="diffs"></div>
</section>
</main>
<script>
const DATA = __DATA__;
const LABELS = DATA.labels, ITEMS = DATA.items, KEY = "expr-audit-" + DATA.id;
const PLAIN = [["none", "None of these", "n"], ["unclear", "Can't judge", "u"], ["noface", "Not a face", "f"]];
let answers = ITEMS.map(() => ({ labels: [], plain: null })), at = 0;
try {
  const kept = JSON.parse(localStorage.getItem(KEY) || "null");
  if (kept && kept.answers && kept.answers.length === ITEMS.length) { answers = kept.answers; at = Math.min(kept.at || 0, ITEMS.length - 1); }
} catch (e) {}
const $ = id => document.getElementById(id);
const keep = () => { try { localStorage.setItem(KEY, JSON.stringify({ answers, at })); } catch (e) {} };
const done = a => a.labels.length > 0 || a.plain !== null;
const said = a => !done(a) ? "" : a.plain || LABELS.filter(l => a.labels.includes(l)).join(",");

function chip(text, key, pressed, act) {
  const b = document.createElement("button");
  b.type = "button"; b.setAttribute("aria-pressed", pressed); b.textContent = text;
  const k = document.createElement("kbd"); k.textContent = key; b.appendChild(k);
  b.onclick = act; return b;
}
function toggle(label) {
  const a = answers[at]; a.plain = null;
  a.labels = a.labels.includes(label) ? a.labels.filter(l => l !== label) : a.labels.concat(label);
  keep(); show();
}
function plain(name) { const a = answers[at]; a.labels = []; a.plain = a.plain === name ? null : name; keep(); show(); }
function show() {
  const a = answers[at], n = answers.filter(done).length;
  $("pic").src = ITEMS[at].img;
  $("chips").replaceChildren(...LABELS.map((l, i) => chip(l.replace("_", " "), String((i + 1) % 10), a.labels.includes(l), () => toggle(l))));
  $("plain").replaceChildren(...PLAIN.map(([name, text, key]) => chip(text, key.toUpperCase(), a.plain === name, () => plain(name))));
  $("fill").style.width = (100 * n / ITEMS.length) + "%";
  $("count").textContent = "Crop " + (at + 1) + " of " + ITEMS.length + " · " + n + " answered";
  $("back").disabled = at === 0;
  $("next").firstChild.textContent = at === ITEMS.length - 1 ? "Finish " : "Next ";
}
function step(by) {
  if (by > 0 && at === ITEMS.length - 1) return finish();
  at = Math.max(0, Math.min(ITEMS.length - 1, at + by)); keep(); show();
}
function sheet() {
  const rows = [["set", "file", "owner", "agents", "a", "b", "c"].join("\t")];
  ITEMS.forEach((it, i) => rows.push([it.set, it.file, said(answers[i]), it.agents, it.a, it.b, it.c].join("\t")));
  return rows.join("\n") + "\n";
}
function finish() {
  const got = ITEMS.map((it, i) => [it, said(answers[i])]).filter(([, s]) => s !== "");
  const alike = got.filter(([it, s]) => s === it.agents).length;
  $("alike").textContent = got.length + " of " + ITEMS.length + " crops answered; " + alike + " of them exactly as the labellers ("
    + (got.length ? Math.round(100 * alike / got.length) : 0) + " %).";
  const has = (s, l) => s === l || s.split(",").includes(l);
  const head = "<tr><th>Answer</th><th class=n>You</th><th class=n>Labellers</th><th class=n>Both</th></tr>";
  $("table").innerHTML = head + LABELS.concat(["none", "unclear", "noface"]).map(l => {
    const you = got.filter(([, s]) => has(s, l)).length, they = got.filter(([it]) => has(it.agents, l)).length;
    const both = got.filter(([it, s]) => has(s, l) && has(it.agents, l)).length;
    return "<tr><td>" + l.replace("_", " ") + "</td><td class=n>" + you + "</td><td class=n>" + they + "</td><td class=n>" + both + "</td></tr>";
  }).join("");
  $("text").value = sheet();
  $("diffs").replaceChildren(...got.filter(([it, s]) => s !== it.agents).map(([it, s]) => {
    const d = document.createElement("div"); d.className = "diff";
    const img = document.createElement("img"); img.src = it.img; img.alt = "";
    const p = document.createElement("div");
    p.innerHTML = "<strong>You:</strong> " + s.replaceAll("_", " ") + "<br><span class=mut><strong>Labellers:</strong> " + it.agents.replaceAll("_", " ") + "</span>";
    d.append(img, p); return d;
  }));
  $("ask").hidden = true; $("result").hidden = false; $("result").scrollIntoView();
}
$("next").onclick = () => step(1); $("back").onclick = () => step(-1); $("end").onclick = finish;
$("resume").onclick = () => { $("result").hidden = true; $("ask").hidden = false; show(); };
$("save").onclick = () => {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([sheet()], { type: "text/tab-separated-values" }));
  a.download = "expr_audit_answers.tsv"; document.body.appendChild(a); a.click(); a.remove();
};
document.addEventListener("keydown", e => {
  if ($("ask").hidden || e.ctrlKey || e.metaKey || e.altKey) return;
  const k = e.key.toLowerCase();
  if (k >= "0" && k <= "9" && k.length === 1) { const i = (Number(k) + 9) % 10; if (i < LABELS.length) toggle(LABELS[i]); }
  else if (k === "n") plain("none"); else if (k === "u") plain("unclear"); else if (k === "f") plain("noface");
  else if (k === "enter" || k === "arrowright") { e.preventDefault(); step(1); }
  else if (k === "arrowleft") step(-1);
});
$("meaning").innerHTML = LABELS.map(l => "<dt>" + l.replace("_", " ") + "</dt><dd>" + DATA.meaning[l] + "</dd>").join("");
show();
</script>
</body>
</html>
"""


def rows_of(path):
    return list(csv.DictReader(open(path, encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE))


def draw(rows, quota, rng):
    """`quota` crops of one check file: some of every label and plain answer first, then any; tracks apart."""
    pool = rows[:]
    rng.shuffle(pool)
    taken, tracks = [], set()

    def take(wanted, most):
        for apart in (True, False):  # another track first; a second crop of a track only if nothing else is left
            for r in pool:
                if most <= 0:
                    return
                if r not in taken and wanted(r) and (r["track"] not in tracks or not apart):
                    taken.append(r)
                    tracks.add(r["track"])
                    most -= 1

    for label in es.LABELS:
        take(lambda r: label in r["labels"].split(","), min(PER_LABEL, quota - len(taken)))
    for name, most in PLAIN.items():
        take(lambda r: r["labels"] == name, min(most, quota - len(taken)))
    take(lambda r: True, quota - len(taken))
    return taken


def page(out, specs):
    rng = random.Random(5)
    items = []
    for k, spec in enumerate(specs):
        name, rest = spec.split("=", 1)
        crops_dir, check = rest.split(",")
        quota = TOTAL // len(specs) + (k < TOTAL % len(specs))
        for r in draw(rows_of(check), quota, rng):
            jpeg = open(os.path.join(crops_dir, r["file"]), "rb").read()
            items.append({"set": name, "file": r["file"], "agents": r["labels"], "a": r["a"], "b": r["b"], "c": r["c"],
                          "img": "data:image/jpeg;base64," + base64.b64encode(jpeg).decode("ascii")})
    rng.shuffle(items)
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "sample.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("set\tfile\n" + "".join(f"{i['set']}\t{i['file']}\n" for i in items))
    data = {"id": f"{len(items)}-{zlib.crc32('|'.join(i['set'] + i['file'] for i in items).encode())}", "labels": es.LABELS,
            "meaning": MEANING, "items": items}
    html = PAGE.replace("__COUNT__", str(len(items))).replace("__DATA__", json.dumps(data).replace("</", "<\\/"))
    with open(os.path.join(out, "audit.html"), "w", encoding="utf-8", newline="\n") as f:
        f.write(html)
    print(f"{len(items)} crops in {os.path.join(out, 'audit.html')} ({len(html) // 1024} KB)")


def compare(path):
    got = [r for r in rows_of(path) if r["owner"]]
    alike = sum(r["owner"] == r["agents"] for r in got)
    print(f"{len(got)} crops answered, {alike} exactly as the labels ({alike / max(len(got), 1):.0%})")
    print("answer\towner\tlabels\tboth\tkappa")
    for name in es.LABELS + ["none", "unclear", "noface"]:
        x = np.array([name in r["owner"].split(",") for r in got])
        y = np.array([name in r["agents"].split(",") for r in got])
        chance = x.mean() * y.mean() + (1 - x.mean()) * (1 - y.mean())
        kappa = ((x == y).mean() - chance) / (1 - chance) if chance < 1 else float("nan")
        print(f"{name}\t{x.sum()}\t{y.sum()}\t{(x & y).sum()}\t{kappa:.2f}")


def main():
    if len(sys.argv) >= 4 and sys.argv[1] == "page":
        page(sys.argv[2], sys.argv[3:])
    elif len(sys.argv) == 3 and sys.argv[1] == "compare":
        compare(sys.argv[2])
    else:
        sys.exit(__doc__)


main()
