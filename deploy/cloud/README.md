# Deploying the AutoScene Cloud gateway

The gateway runs on the owner's VDS as `autoscene-cloud.service` (user `autoscene-cloud`), behind nginx.

| Path | What |
|---|---|
| `/opt/autoscene-cloud/bin/autoscene-cloud-gateway` | the binary |
| `/opt/autoscene-cloud/toolchain` | its own Rust toolchain (rustup, stable), used only to build it |
| `/etc/autoscene-cloud/gateway.toml` | settings (`gateway.toml.example` here) |
| `/etc/autoscene-cloud/gateway.env` | secrets, root only: `AS_CLOUD_ADMIN_TOKEN`, `AS_CLOUD_PEPPER` (never change it), `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET` |
| `/var/lib/autoscene-cloud` | `gateway.db`, uploads, daily backups (14 kept; copy them off the machine) |

## Build and install a new version

1. On a computer with the repo, pack only what the gateway needs, from a commit:
   ```
   git archive HEAD crates/as-cloud crates/as-cloud-gateway vendor/rusqlite_migration Cargo.lock | tar -x -C gwbuild
   python deploy/cloud/miniws.py . gwbuild/Cargo.toml
   tar -czf gwbuild.tgz -C gwbuild .
   ```
2. Copy `gwbuild.tgz` to `/tmp` on the VDS.
3. On the VDS:
   ```
   B=/opt/autoscene-cloud
   export RUSTUP_HOME=$B/toolchain/rustup CARGO_HOME=$B/toolchain/cargo
   rm -rf $B/src/* && tar -xzf /tmp/gwbuild.tgz -C $B/src
   cd $B/src && CARGO_TARGET_DIR=$B/target $CARGO_HOME/bin/cargo build --release -p as-cloud-gateway
   install -m 0755 $B/target/release/autoscene-cloud-gateway $B/bin/
   systemctl restart autoscene-cloud
   ```

A restart closes open scan sessions and charges each what it had done.

## Admin

Calls go to `http://127.0.0.1:8741` with `Authorization: Bearer $AS_CLOUD_ADMIN_TOKEN`; see `docs/CLOUD.md`.

- While there is no GPU backend (`[backend] kind = "none"`), scans stay paused: `POST /admin/pause {"paused": true}`.
- Accounts, codes and payments work while paused.
