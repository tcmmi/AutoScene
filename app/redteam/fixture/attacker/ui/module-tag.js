// Check 10: a module of the page's own, by tag, which imports another.
window.moduleTagRan = true
import("./imported.js").then(
  (m) => (window.moduleImport = `ok: ${m.ok}`),
  (e) => (window.moduleImport = `threw: ${e.name}`),
)
