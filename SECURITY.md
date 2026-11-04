# Security

## Report a hole privately

If you found a way around one of AutoScene's walls, please don't describe
it in an issue or a pull request: those are public. Report it privately
through GitHub instead:

1. Open the [Security tab](https://github.com/tcmmi/AutoScene/security) of
   this repository.
2. Choose **Report a vulnerability**.
3. Say what you did, what you got, and on which system and AutoScene
   version. A plugin, a page or a package that shows it helps most.

Only you and the maintainers see the report, and you hear back there. It
stays private until a release fixes the hole. Say whether you want to be
named with the fix.

No **Report a vulnerability** button? Then
[open an issue](https://github.com/tcmmi/AutoScene/issues) that asks only
for a private way to reach the maintainers: no details of the hole, no
steps, no plugin that shows it. They answer there and give you one.

Fixes go into the newest release only. Update to it before you report.

## What counts

A way past one of these walls:

- **A plugin page's sandbox and its bridge**: a page that gets out of its
  frame, runs a script that isn't its own, reaches AutoScene's own
  commands, or gets more from a call than the permissions the person agreed
  to give it.
- **The WebAssembly tier**: a plugin of runtime `wasm` that reads or writes
  outside its folders, reaches a host or an address its permissions don't
  give it, starts a program, outlives Stop or its limits, or gets a secret
  it didn't ask for.
- **The signatures of the plugin catalog** and of packages: a package or a
  catalog entry that AutoScene takes as signed by a key that didn't sign
  it, an update that replaces a plugin without its author's key, or a
  changed file that AutoScene doesn't notice.
- **AutoScene's trust states**: a plugin that gets a trust it shouldn't
  (Reviewed, Pinned), or runs while "Allow plugins that AutoScene has not
  reviewed" is off.

`docs/PLUGINS.md` describes each wall, under "What AutoScene enforces".

## What doesn't

- **What a process plugin does with your rights.** A plugin of runtime
  `process` or `service` runs a program with your user rights: it can read,
  change and send anything you can. AutoScene says so before you install
  one, and doesn't confine it. That a program can do this isn't a hole.
- What a plugin sends to the hosts its `plugin.json` names and you agreed
  to, and how much it writes in the folders it was given.
- The side channels of a page that `docs/PLUGINS.md` lists as open.

Other bugs, also crashes: please
[open an issue](https://github.com/tcmmi/AutoScene/issues).
