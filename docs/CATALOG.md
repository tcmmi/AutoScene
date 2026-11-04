# AutoScene's plugin catalog

The catalog is a signed list of the plugin versions AutoScene reviewed, and
of the versions it withdrew. AutoScene fetches it, checks its signature,
and judges plugins by it: a package whose `files.lock` the catalog lists is
**Reviewed**, and a version it withdrew doesn't run. Browse, under Plugins,
shows what it lists.

This page is for the people who keep the catalog, and for anyone who wants
to check it. Plugin authors start with [PLUGINS.md](PLUGINS.md), which says
what the catalog means for a plugin and how to submit one. The code is
`crates/as-plugin/src/catalog.rs` (the index and its checks),
`crates/as-plugin/src/keys.rs` (the keys and addresses compiled in),
`app/src-tauri/src/plugins/catalog.rs` (fetching, the schedule, getting
packages) and `asp catalog` in `crates/as-plugin/src/bin/asp.rs`.

**The catalog isn't open yet.** This AutoScene has no catalog keys
compiled in (`CATALOG_KEYS` is empty), so it checks no index: Browse says
"The plugin catalog isn't open yet", and every plugin is Pinned or
Unreviewed. Everything below already works with a catalog of one's own
(see [A catalog of one's own](#a-catalog-of-ones-own)).

## Where it is

The catalog's repository on GitHub, `tcmmi/autoscene-plugins`, builds each
plugin version from its source, and its CI writes and signs the index.
AutoScene fetches the index from these addresses, in turn, and takes the
newest one that checks out:

1. `https://tcmmi.github.io/autoscene-plugins/`
2. `https://raw.githubusercontent.com/tcmmi/autoscene-plugins/main/site/`

At each, it gets `index.json` and its signature, `index.json.minisig`.

## The index

`index.json` is one JSON object. The example below is checked by a test
(`the_catalog_example_in_the_docs_checks_out` in
`crates/as-plugin/tests/asp_cli.rs`): it reads as an index, and once `asp
catalog bump` dates it and `asp catalog sign` signs it, `asp catalog
verify` takes it; its own dates hold as printed. The first entry is what
`asp catalog add` writes for `examples/plugins/hello-export` at commit
`3126f1e` (its icon left out, its time of approval set by hand). The
second plugin is made up, and so are its key, hashes and commit.

<!-- index-example -->
```json
{
  "catalogVersion": 1,
  "epoch": 1,
  "seq": 7,
  "published": 1793750400,
  "expires": 1796342400,
  "plugins": [
    {
      "id": "dev.autoscene.hello-export",
      "version": "1.0.0",
      "lock": "00994a388792c4d15dfb4288f2413ccdce9f94e157c317c346fddfac4bcba256",
      "package": {
        "urls": [
          "https://github.com/tcmmi/autoscene-plugins/releases/download/dev.autoscene.hello-export%401.0.0/dev.autoscene.hello-export-1.0.0.asp"
        ],
        "size": 4589,
        "sha256": "6cf12e8b08a858a0d713a7a00ea87c09446ac65a98cbfbcc200eb7d274295c4d"
      },
      "authorKey": null,
      "name": "Hello Export",
      "summary": "Writes a list of an export's clips beside them, and says hello from the command palette. An example plugin.",
      "author": "AutoScene",
      "license": null,
      "platforms": [],
      "runtime": "process",
      "gpu": false,
      "uvScript": true,
      "permissions": ["exports:read", "exports:write"],
      "hosts": [],
      "engines": ">=1.0.0, <2",
      "source": {
        "repo": "https://github.com/tcmmi/AutoScene",
        "commit": "3126f1ea504f7742325ee42a9c3e810fc5da797a",
        "path": "examples/plugins/hello-export"
      },
      "approved": 1793700000
    },
    {
      "id": "dev.example.stem-splitter",
      "version": "2.1.0",
      "lock": "bcb2465259e29f11a3da0544182daa45532c664177f962133ddd78918e8b6220",
      "package": {
        "urls": [
          "https://github.com/tcmmi/autoscene-plugins/releases/download/dev.example.stem-splitter%402.1.0/dev.example.stem-splitter-2.1.0.asp"
        ],
        "size": 48213,
        "sha256": "b8d93ea950c47e13c218256688b8247c44d8b77b103891690fb54b0f9c88dc8f"
      },
      "authorKey": "RWRRSn4/DB6dK9dXmSPOGsm8CC7f+A9dK/G4yCweX0BvsvtmBeITrgxc",
      "name": "Stem Splitter",
      "summary": "Splits each exported clip's sound into voice and music.",
      "author": "Example Audio",
      "license": "Apache-2.0",
      "platforms": ["windows-x86_64", "linux-x86_64"],
      "runtime": "process",
      "gpu": true,
      "uvScript": true,
      "permissions": ["exports:read", "exports:write"],
      "hosts": [],
      "engines": ">=1.0.0, <2",
      "source": {
        "repo": "https://github.com/example-audio/stem-splitter",
        "commit": "6b1f53303a732ccc8c6aae6640399827c15250e3",
        "path": ""
      },
      "approved": 1793740000,
      "resourceBytes": 284123456,
      "resourceHosts": ["huggingface.co"]
    }
  ],
  "withdrawn": [
    {
      "id": "dev.example.stem-splitter",
      "versions": "<2.0.1",
      "reason": "It sent the clips' names to a host it doesn't name.",
      "since": 1793600000
    }
  ]
}
```
<!-- /index-example -->

### The header

| Field | What it is |
|---|---|
| `catalogVersion` | The index format: `1`. One above this AutoScene's makes it say "This catalog needs a newer AutoScene.", before any signature is checked, so the page can tell the person to update. |
| `epoch` | The key epoch the index is in (see [Keys](#keys)): `1` or more. |
| `seq` | Its number in its epoch. |
| `published` | When it was published, in Unix seconds. |
| `expires` | When it runs out, in Unix seconds. |
| `epochCertificate` | From epoch 2 on: the offline key's certificate of its epoch, the four lines of a minisign signature (see [Keys](#keys)). Left out in epoch 1. |
| `plugins` | The plugin versions it lists. |
| `withdrawn` | The versions it withdraws. |

Indexes are ordered by `(epoch, seq)`, the epoch first. AutoScene takes an
index only when its number is higher than that of the index it holds. The
same number again counts only with the same bytes ("Two catalogs carry the
same number." otherwise), and a lower one is refused ("This catalog is
older than the one AutoScene has."). An index of an epoch below the lowest
this build takes (`MIN_EPOCH` in `keys.rs`) is refused the same way.

Its dates must hold at the moment it is checked:

- `expires` is after `published`, by at most 31 days (`MAX_LIFETIME`). The
  catalog's CI makes it 30.
- `published` is at most 3 days ahead of the computer's clock
  (`MAX_CLOCK_SKEW`): "The catalog is dated <day>, which hasn't come yet.
  Check this computer's clock."
- `expires` hasn't passed: "The catalog ran out on <day>."

### A plugin version (`plugins`)

| Field | Required | What it is |
|---|---|---|
| `id` | yes | The plugin's id, as `plugin.json` spells it. |
| `version` | yes | Its version, strict semver. An id lists each version once: versions that differ only in build metadata (`+…`) are the same. |
| `lock` | yes | The SHA-256 of its `files.lock`, 64 lowercase hex digits. A package with that lock is the one reviewed. |
| `package` | yes | `urls`: 1 to 4 `https://` addresses of the package, tried in turn, each at most 2048 characters without spaces or control characters. `size`: its size in bytes, 1 to 1 GiB. `sha256`: its SHA-256, 64 lowercase hex digits. |
| `authorKey` | no | The author's minisign public key (the base64 line of `author.pub`), as the catalog knows it. A reviewed package is pinned to it. |
| `name` | yes | Cut to 40 characters. |
| `summary` | no | Cut to 300 characters. |
| `author` | yes | The author, as the catalog checked them: cut to 60 characters. |
| `license` | no | Cut to 60 characters. |
| `platforms` | no | The platforms it runs on: at most 16, each up to 40 characters of `[a-z0-9_-]`, so a platform a later AutoScene adds reads too. This one knows `windows-x86_64`, `windows-aarch64`, `macos-aarch64`, `macos-x86_64`, `linux-x86_64` and `linux-aarch64`. None: all. |
| `runtime` | no | Its `runtime.type`: up to 16 lowercase letters, so a runtime a later AutoScene runs reads too. |
| `gpu` | no | It uses the graphics card. |
| `uvScript` | no | It runs a Python script through uv. |
| `permissions` | no | At most 16, each `[a-z][a-z0-9:-]` and at most 32 characters, so a later AutoScene's permissions read too. |
| `hosts` | no | Its `network.hosts`: at most 16 host names. |
| `engines` | yes | Its `engines.autoscene`, a version requirement such as `>=1.0.0, <2`. |
| `source` | yes | Where it was built from: `repo`, `https://github.com/<owner>/<repo>`; `commit`, 40 lowercase hex digits; `path`, its folder there (empty or `.` for the top). |
| `icon` | no | Its icon as a `data:image/png;base64,` or `data:image/svg+xml;base64,` URL, at most 8 KiB decoded, which passes the checks a package's icon passes. |
| `approved` | yes | When the catalog approved it, in Unix seconds: not after `published`. |
| `resourceBytes` | no | What the large files its `plugin.json` names take, in bytes: what its first run downloads. At most 8 GiB. Left out when 0. |
| `resourceHosts` | no | The hosts those files come from, lowercase: at most 48, each a name on the internet (no address, no `localhost`). Left out when empty. `asp catalog add` writes each once; AutoScene doesn't check that. |

Texts lose control characters and invisible ones before they are cut; a
`name` or `author` with nothing left refuses the whole index (see below). A
`license` with nothing left is read as none.

### A withdrawal (`withdrawn`)

| Field | What it is |
|---|---|
| `id` | The plugin's id. |
| `versions` | The versions withdrawn, as a version requirement: `<2.0.1`, `>=1.2.0, <1.3.0`, or `*` for all. |
| `reason` | Why, in AutoScene's words: 1 to 200 characters, once cleaned. The plugin's banner shows it. |
| `since` | When, in Unix seconds: not after `published`. |

A pre-release counts as withdrawn when the requirement takes its release or
its place in the order: `<1.3.0` withdraws `1.3.0-rc.1`, and `>=1.2.0,
<1.3.0` withdraws `1.2.5-beta`. Each reading leans toward withdrawing.

### Limits

| What | At most |
|---|---|
| `index.json` | 8 MiB |
| `index.json.minisig` | 4 KiB |
| Plugin versions | 4096 |
| Withdrawals | 1024 |

More refuse the whole index: "The catalog lists more than 4096 plugins."
and "The catalog withdraws more than 1024 plugins."

**One entry that breaks a rule refuses the whole index**, with "The catalog
has an entry AutoScene can't read: <id> <version>.", and so does one
withdrawal: "The catalog has a withdrawal AutoScene can't read: <id>."
Without an id, the message names its place instead ("number 3"). The
catalog's CI wrote it, so a bad entry is a bug there, and a partial index
could hide a withdrawal.

## Three signatures, three comments

Every signature here is minisign's, made the prehashed way; AutoScene takes
no other. Each says in its trusted comment what it signs, so one never
passes for another:

| Signed | Trusted comment | By |
|---|---|---|
| An index (`index.json.minisig`) | `autoscene-catalog;v=1;epoch=<epoch>;seq=<seq>;expires=<expires>` | An online or the offline catalog key |
| An epoch certificate (`epochCertificate`) | `autoscene-catalog-epoch;v=1;epoch=<epoch>;withdraw=<key ids>;online=<keys>` | The offline catalog key |
| A package (`files.lock.minisig`) | `autoscene-plugin;id=<id>;version=<version>;lock=<sha256>` | The plugin's author |

- **The index's comment** names the epoch, number and expiry of the index
  it signs. AutoScene reads them from the comment, in this one spelling
  (decimal, no leading zeros), and refuses an index whose header says
  otherwise ("The catalog's signature names another catalog."). So a
  signature can't be moved to another index, and a package's signature is
  never an index's ("That isn't a catalog signature."). A comment of a
  later format (`v=2`) says "This catalog needs a newer AutoScene."
- **The certificate signs its own line**: the data and the trusted comment
  are the same text. Its lists are sorted, joined by commas, and `-` when
  empty. It never sits beside an index on its own: the index carries it.
- **A package's signature** is the author's, for that one version (see
  "Signing with asp" in [PLUGINS.md](PLUGINS.md#signing-with-asp)). The
  catalog never signs a package: it lists its lock. A package it lists is
  Reviewed whether or not its author signed it.

The untrusted comments, which nothing checks, say `AutoScene plugin
catalog` and `AutoScene plugin catalog epoch certificate`.

## Keys

Two kinds of key sign the catalog (`KeyRole` in `keys.rs`):

| Role | Who holds it | What it may sign |
|---|---|---|
| Online | The catalog's CI, as an Actions secret | Indexes of epoch 1, and of each later epoch whose certificate names it |
| Offline | The owner, on a computer that isn't online | Indexes of any epoch, and alone the epoch certificates |

A key's id is the 16 hex digits minisign shows for it. The keys AutoScene
trusts are compiled in (`CATALOG_KEYS`), each with its role: they are not
the app updater's key. Either role's key may sign an index.

**Epochs.** Epoch 1 needs no certificate: its online keys are the compiled
ones. An index of a later epoch carries the offline key's certificate of
that epoch, which:

- names every online key of the epoch, the compiled one too: a compiled
  online key it leaves out doesn't sign in that epoch;
- may withdraw keys by their ids, for good. AutoScene keeps every withdrawn
  id it has seen (`withdrawnKeys` in its row, which only grows), and each
  later certificate withdraws them again, so an AutoScene that missed an
  epoch learns of them too;
- can't withdraw the offline key: "The catalog's epoch certificate
  withdraws AutoScene's offline key." Only a release changes the offline
  key.

So a leaked online key can sign an index with the highest number there is,
but only in its own epoch. Within it, what it signs is a newer index like
any other: it can leave out a withdrawal, and the plugin runs again, or
drop entries. The owner's next certificate leaves it out (and withdraws
it), and its epoch is higher still, which wins over any number.
That holds also for an AutoScene that never saw the epoch that withdrew it:
the higher epoch's certificate doesn't name the leaked key.

An index of a later epoch without a certificate, or with one the offline
key didn't sign, is refused: "The catalog's new epoch isn't signed by
AutoScene's offline key."

## What AutoScene does with it

**Checks.** AutoScene checks the catalog 10 seconds after it starts, unless
the last good fetch is less than an hour old, and then once a day while it
is open. After a failed check it waits an hour before the next try. It
looks every 10 minutes whether a check is due, so sleep and a day that
passes while it is open don't skip one. A time in the future (written
while the clock ran ahead) counts as long ago, so a corrected clock checks
at once.

- **Check the plugin catalog every day** (Plugins → Browse) turns these
  checks off and on; it is on until the person turns it off. Off,
  "AutoScene doesn't learn of withdrawn plugins."
- **Check now** (Browse), and **Check for plugin updates** in the
  palette, check whatever the setting says.

**Each check** fetches `index.json` (at most 8 MiB) and its signature (at
most 4 KiB) from every address, each within 30 seconds, through the proxy
the environment names. It keeps the newest index that checks out against
the index it holds. When none does, the check failed: the catalog in hand
stays, and its row says why, which Browse shows ("AutoScene couldn't reach
its plugin catalog." when no address answered as it should; the log has
what went wrong).

**What it keeps.** The row `plugins_catalog` of `library.db` holds what
AutoScene took: the epoch, number and SHA-256 of the index held, its dates,
when it was fetched and last tried, why the last try failed, the key that
signed it, the withdrawn key ids, and the index's withdrawals. The index
itself and its signature are in `cache/catalog/` in the data folder. The
row is written before the files, so a crash between them leaves files the
row doesn't name: they aren't used, and the next check fetches the index
again. A lost or changed file loses what the catalog reviewed until then,
never what it withdrew: the row keeps the withdrawals. A row a later
AutoScene wrote (its `v` above 1) turns the catalog off in this one, which
never writes over it; Browse then says "Update AutoScene to use the plugin
catalog".

**Reviewed.** A package is Reviewed when the living index lists its id,
its version and its lock's SHA-256, and doesn't withdraw that version. It
is pinned to the `authorKey` the entry names, or keeps the pin it had. That
holds for a package got from the catalog and for one opened from a file
alike. The review shows where the catalog built it from: the repository,
the commit and the folder.

**Getting a plugin.** Install… and Update… in Browse (and Update… on an
installed plugin) get the package of the newest version the catalog
offers this AutoScene on this computer, and open its review. Only from an
index that hasn't run out, only a version newer than the one installed,
and only the file the entry pins: it comes from the entry's addresses in
turn, resumes after an interruption, and is kept only with the entry's
size and SHA-256. It waits in `cache/plugin-downloads/`, named by its
SHA-256, until it is checked; that folder is emptied at each start. The
package must then be the id and version the entry names, under the lock it
lists, or it is refused ("The catalog's package isn't the one it lists.
Try again later."). The review's dialog shows how far the download came,
with a Cancel. One get of a plugin runs at a time.

**When the index runs out.** Its `expires` passed and no newer one came:

- nothing installs or updates from the catalog ("The plugin catalog is out
  of date. Check it again, then try again."), and Browse lists no plugins:
  a banner says since when AutoScene couldn't check it;
- no package becomes Reviewed by it, also one whose lock it lists;
- installed plugins keep their trust and keep running;
- what it withdrew stays withdrawn.

**When it withdraws a version.** At the check that takes the index:

- the plugin's runs stop, its pages close and its service stops, as for a
  plugin turned off;
- it doesn't run again, nor install, nor load from a folder, whatever its
  trust or source: "AutoScene withdrew <name>. <reason>";
- its banner, wherever its name shows, says "AutoScene withdrew Plugin
  “<name>”" with the reason, offers **Uninstall…**, and **Update…** while
  the catalog offers a version it didn't withdraw;
- it stays withdrawn after the index runs out, and after its file is lost.

A newer index decides anew: its `withdrawn` replaces the one held, so a
withdrawal it leaves out is lifted.

**What the page sees.** `catalog_list` (and `catalog_refresh`, which
checks first) gives the status (whether the
catalog is open, its dates, out of date, the last error, whether a newer
AutoScene is needed) and what it offers, plugin by plugin: the newest
version this AutoScene runs here, or the newest it can't run ("Needs a
newer AutoScene", "Not for this computer"), with what it asks for that the
installed version wasn't granted. A plugin the catalog withdrew every
version of is listed only while it is installed.

## Large files

A plugin may name large files in its `plugin.json` (`resources`, see
"Large files" in [PLUGINS.md](PLUGINS.md#large-files)): AutoScene downloads
them at its first run and checks each by its SHA-256, which the plugin's
lock covers, so the index covers them as it covers the code. `asp catalog
add` fills `resourceBytes` and `resourceHosts` from the built `plugin.json`.

What AutoScene enforces of them:

- at most 16 files, each 1 byte to 4 GiB, at most 8 GiB together;
- each from 1 to 3 plain `https://` addresses on port 443, of a name on the
  internet;
- a plain file each: nothing is unpacked;
- an update whose files come from a host the installed version's didn't is
  never one click: its review marks that host New, and Update all skips
  it. A larger download from known hosts is no new consent: the hashes are
  what was reviewed, and the review still says the size.

What the catalog's policy adds (its `POLICY.md`; its CI checks it before a
person reviews):

- A package holds at most 256 MiB. Files over 100 MB come by URL with a
  hash, from a pinned Hugging Face revision (`resolve/<40-digit
  commit>/`, never a branch) or a release of the author's own repository.
- CI downloads each file from its first address before review and refuses
  it unless its size and SHA-256 match. It holds the file to the rules of
  the files in the folder: no programs, libraries or archives.
- Weights come as ONNX, safetensors or GGUF. A pickle needs a reason, and
  code that loads it without running it (`torch.load(...,
  weights_only=True)`, never `pickle.load`). The same rules hold for files
  in the folder.

Review proves a file's exact bytes and how the plugin's code uses them. It
doesn't prove the bytes are harmless: the reviewer reads the code that
loads them.

## asp catalog

```text
asp catalog new <index.json>
asp catalog add <index.json> <pkg.asp> --url <https://…>… --repo <https://github.com/o/r> --commit <40 hex>
                [--path <dir>] --author <name> [--author-key <RW…>]
asp catalog withdrawn <index.json> <withdrawn.json>
asp catalog certify --epoch <n> --key <offline.key> [--withdraw-key <ID>]… [--online-key <RW…>]…
                    [--previous <epoch.minisig>] --out <epoch.minisig> [--force]
asp catalog epoch <index.json> <epoch.minisig>
asp catalog bump <index.json> [--days 30]
asp catalog sign <index.json> --key-env <NAME> [--password-env <NAME>]
asp catalog verify <index.json> [--previous <old.json>] [--online-key <RW…>]… [--offline-key <RW…>]…
asp catalog diff <index.json> <pkg.asp>
```

- `new` writes an empty index: epoch 1, number 0, published and running
  out at 0, so the first `bump` dates it. It never writes over a file.
- `add` lists a package with what it says of itself (its lock, size and
  SHA-256, name, summary, license, platforms, runtime, permissions, hosts,
  `engines.autoscene`, icon and large files), where it is (`--url`, once
  or more), the source it was built from, and its author (and key) as the
  catalog checked them; approved now. A version the index lists already is
  refused.
- `withdrawn` sets the withdrawals to those of a JSON file:
  `[{"id", "versions", "reason", "since"}]`.
- `certify` signs an epoch certificate with the offline key, on the
  owner's computer. It asks for the key's password. It withdraws again
  what the `--previous` certificate withdrew, and refuses to withdraw the
  key that signs, or a key it also names.
- `epoch` attaches a certificate to the index, which starts that epoch at
  number 0. A lower epoch is refused, and so is another certificate of the
  same one.
- `bump` numbers the index one higher and dates it: published now, running
  out in `--days` (1 to 31; 30 when left out).
- `sign` signs the index with a secret key it reads from the environment
  variable `--key-env` names (and its password from `--password-env`), and
  writes `<index.json>.minisig`. It writes the key nowhere. It refuses an
  index AutoScene would refuse (its dates included), and one asp didn't
  write as it writes one: pretty-printed with a trailing newline. Those
  bytes are what it signs.
- `verify` checks the index and its signature as an AutoScene that holds
  no catalog takes them, or one that holds `--previous`, of which it must
  keep every version under the same lock. It says the epoch, number,
  dates, the key that signed and how many versions and withdrawals it has.
- `diff` says in Markdown, for the CI's job summary, what a package
  changes against the newest version the index lists and hasn't withdrawn:
  its permissions, hosts, platforms, runtime, uv, graphics card, AutoScene
  versions and files.

Each checks entries and withdrawals as AutoScene does. The keys given with
`--online-key` and `--offline-key` are added to AutoScene's own.
`asp verify <pkg.asp> --catalog <index.json>` says the trust a package
would get by an index.

### From new to verify

```bash
asp keygen ~/catalog-keys/online --unencrypted   # a throwaway online key
asp catalog new site/index.json
asp pack examples/plugins/hello-export hello-export.asp
asp catalog add site/index.json hello-export.asp \
  --url https://example.com/hello-export.asp \
  --repo https://github.com/tcmmi/AutoScene --commit <40 hex> \
  --path examples/plugins/hello-export --author AutoScene
asp catalog bump site/index.json
CATALOG_KEY="$(cat ~/catalog-keys/online/asp.key)" asp catalog sign site/index.json --key-env CATALOG_KEY
asp catalog verify site/index.json --online-key "$(cat ~/catalog-keys/online/author.pub)"
asp verify hello-export.asp --catalog site/index.json --online-key "$(cat ~/catalog-keys/online/author.pub)"
```

The last line says `dev.autoscene.hello-export 1.0.0: Reviewed`. Without `--online-key`, `verify` says "The catalog is signed
with a key AutoScene doesn't know.": this AutoScene knows no catalog key
yet.

### A catalog of one's own

A debug build of AutoScene reads `~/.autoscene/catalog.json`, if it is
there:

```json
{
  "urls": ["http://127.0.0.1:8000/"],
  "keys": [{ "role": "online", "key": "RW…" }]
}
```

`urls` replace the compiled addresses (`http://` too), and `keys` are added
to the compiled ones. What it takes is kept apart, in the row
`plugins_catalog.dev` and `cache/catalog-dev/`: debug and release builds
share the data folder, and a release build never sees its numbers or its
withdrawals. Release builds don't have this code.

## Runbook

**The online key leaks or is lost:**

1. Delete the `CATALOG_KEY` secret.
2. Make a new online key.
3. On the offline computer: `asp catalog certify --epoch <n+1>
   --withdraw-key <old id> --online-key <new key> --previous
   epochs/epoch-<n>.minisig --out epochs/epoch-<n+1>.minisig` (in epoch 1,
   which has no certificate, leave out `--previous`).
4. Commit the certificate, and set the new secret.
5. The next publish attaches it (`asp catalog epoch`) and starts the
   epoch. No release of AutoScene is needed.
6. The next release of AutoScene compiles the new online key in, drops the
   old one, and raises `MIN_EPOCH` to the new epoch. Until then, a fresh
   install of an older build can still take an older epoch's index signed
   with the leaked key, but only from the catalog's own addresses, which
   that key alone can't change.

**The offline key leaks:** release AutoScene with a new offline key and
without the old one. Until people update, the leaked key could start
epochs: say so in the release notes. The release doesn't help an AutoScene
that already took an index the leaked key signed: the key can certify an
epoch as high as it likes, and AutoScene keeps that number in its row
whoever signed it. It then refuses every real index ("This catalog is
older than the one AutoScene has."), raising `MIN_EPOCH` doesn't change
that, and it learns of no withdrawal, until a release forgets that row.
No release does that yet: it is the code to write when this happens.

**The offline key is lost:** nothing breaks now. No new epoch can start
until a release brings a new offline key.

**Withdraw a plugin:** a pull request to `withdrawn.toml` in the catalog's
repository, merged by the owner. Its CI signs it into the index at once.
An AutoScene that checks daily learns of it within a day, and at once
with Check now.

## Threats

| Attack | What stops it here | What doesn't |
|---|---|---|
| Rollback: an older signed index, one from before a withdrawal | AutoScene keeps the highest `(epoch, seq)` it took, and refuses a lower one, and the same one with other bytes. | An AutoScene that never took the newer index (a fresh install) takes any living one. |
| Freeze: the last good index, served forever | An index lives at most 31 days, by the computer's clock. Then nothing installs or updates from it, Browse says so, and what it withdrew stays withdrawn. | Up to 31 days of a stale index. A clock set back keeps an index alive. |
| Fast-forward: a leaked online key signs the highest number | The epoch counts first, and only the offline key raises it; the next certificate leaves the leaked key out. | Until the next epoch, the leaked key signs indexes, and an AutoScene that took its highest number takes no lower one of that epoch. Those indexes can also lift withdrawals and drop entries: "stays withdrawn" holds against an older index and one that ran out, not against a newer signed one. |
| Mix and match: entries of different indexes | One signature covers the whole index. An entry binds the id, version, lock, the package's SHA-256 and size. | |
| Endless data | The index is read to 8 MiB and its signature to 4 KiB; a package to the size its entry names, at most 1 GiB. | |
| Slow retrieval | 30 seconds for each index file; a package that sends nothing for 60 seconds fails, and Cancel ends it. | A package that trickles bytes, until the person cancels. |
| Wrong software: a sound package that isn't the one asked for | The entry binds id, version and lock, and the package must be that id and version under that lock. | |
| Malicious mirrors: one address hides updates | Every address is tried, and the newest index wins. | Hiding the catalog at every address: the index then runs out. |
| Key compromise | The offline key is never online, and alone starts epochs and withdraws keys. A compiled key a certificate leaves out stops counting. | A leaked offline key, until a release replaces it, and for an AutoScene that took an index it signed, until a release forgets that row (see the [Runbook](#runbook)); meanwhile it can lift withdrawals. A GitHub account that can change the catalog's CI and its secrets can sign anything with the online key, and publish it: only review of the repository's changes stops that. |
