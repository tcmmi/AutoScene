# rusqlite_migration — AutoScene copy for rusqlite 0.41

Upstream: <https://github.com/cljoly/rusqlite_migration> (Apache-2.0, see `LICENSE.txt`), version 2.6.0,
from the published crate (`rusqlite_migration-2.6.0.crate`, SHA-256
`03451d4dc523c0a96d9e412c574670b3f5f184c3988b77a435876be25477bbd3`, as in `Cargo.lock` before this copy).
Wired in through `[patch.crates-io]` in the workspace `Cargo.toml`.

Why: SQLite before 3.54.0 has CVE-2026-119375 (RUSTSEC-2026-0459). The fixed SQLite comes with
`libsqlite3-sys` 0.39, which only `rusqlite` 0.41 uses, and 2.6.0 asks for `rusqlite` 0.40. Only one
`libsqlite3-sys` may be in a build.

Changes against the published crate:

- `Cargo.toml`: depends on `rusqlite` 0.41.1 instead of 0.40.0; dev-dependencies dropped (this copy is not a
  workspace member, so its tests don't run here).

No source was changed: `src/` and `build.rs` are the published files. Upstream's own move to 0.41
(cljoly/rusqlite_migration#370) changes only versions and one doc example. Remove this directory and the patch
once upstream releases a version for `rusqlite` 0.41.
