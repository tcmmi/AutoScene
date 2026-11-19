"""A workspace of just the gateway and its shared crate, from the repo's own
manifest: the same pinned versions, nothing else to build."""
import re
import sys
import tomllib

root, out = sys.argv[1], sys.argv[2]
text = open(f"{root}/Cargo.toml", encoding="utf-8").read()
ws = tomllib.loads(text)["workspace"]
wanted = {"serde", "serde_json", "thiserror", "ureq", "sha2", "parking_lot", "rusqlite", "rusqlite_migration",
          "tokio", "toml", "tracing", "uuid", "tempfile", "as-cloud"}
lines = text.splitlines()
deps = []
inside = False
for line in lines:
    if line.strip() == "[workspace.dependencies]":
        inside = True
        continue
    if inside and line.startswith("["):
        break
    if inside:
        m = re.match(r"^([A-Za-z0-9_-]+)\s*=", line)
        if m and m.group(1) in wanted:
            deps.append(line)
pkg = ws["package"]
manifest = f'''[workspace]
resolver = "3"
members = ["crates/as-cloud", "crates/as-cloud-gateway"]

[workspace.package]
version = "{pkg["version"]}"
edition = "{pkg["edition"]}"
rust-version = "{pkg["rust-version"]}"
license = "{pkg["license"]}"
repository = "{pkg["repository"]}"
authors = {pkg["authors"]!r}

[workspace.dependencies]
{chr(10).join(deps)}

[profile.release]
lto = "thin"
codegen-units = 4
strip = "debuginfo"

[patch.crates-io]
rusqlite_migration = {{ path = "vendor/rusqlite_migration" }}
'''.replace("'", '"')
open(out, "w", encoding="utf-8", newline="\n").write(manifest)
print(manifest)
