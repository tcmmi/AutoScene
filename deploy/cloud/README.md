# Deploying the AutoScene Cloud gateway

The gateway runs on the owner's VDS as `autoscene-cloud.service` (user `autoscene-cloud`), behind nginx, on two names:

- **https://api.autoscene.io**, for the app and the GPU worker (`public_base`). It is DNS only, straight to the VDS, with no body or time limit from Cloudflare.
- **https://autoscene.io**, through Cloudflare (proxied, SSL mode Full). It carries the Stripe webhook and the `/paid` and `/cancelled` pages, and a website can take its other paths later.

Both have Let's Encrypt certificates (`certbot certonly --webroot -w /var/www/autoscene.io -d <name>`), renewed by certbot's timer with an nginx reload.

| Path | What |
|---|---|
| `/opt/autoscene-cloud/bin/autoscene-cloud-gateway` | the binary |
| `/opt/autoscene-cloud/toolchain` | its own Rust toolchain (rustup, stable), used only to build it |
| `/etc/autoscene-cloud/gateway.toml` | settings (`gateway.toml.example` here) |
| `/etc/autoscene-cloud/gateway.env` | secrets, root only: `AS_CLOUD_ADMIN_TOKEN`, `AS_CLOUD_PEPPER` (never change it), `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET` |
| `/etc/nginx/sites-available/{api.autoscene.io,autoscene.io}`, `/etc/nginx/snippets/autoscene-cloud.conf` | the sites (copies here: `nginx-*.conf`). Only `autoscene.io` uses `snippets/cloudflare-real-ip.conf`; on `api.autoscene.io` clients connect directly |
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

## Stripe

- The sandbox webhook endpoint `we_1Udc6eBPP05aCPabANLSfKKv` (account "AutoScene sandbox") sends four events to `https://autoscene.io/v1/stripe/webhook`: `checkout.session.completed`, `checkout.session.async_payment_succeeded`, `charge.refunded` and `charge.dispute.created`. Its signing secret is `STRIPE_WEBHOOK_SECRET`.
- Going live means repeating this on the live "AutoScene" account, and swapping the live `sk_live_` key and that endpoint's secret into `gateway.env`.
- Buyers come back to `/paid` or `/cancelled`, two pages nginx serves itself.

## Why two names

Cloudflare's free plan limits a request body to 100 MB and an answer to 100 s. `ready` can take longer on very large films, so the app and the worker use `api.autoscene.io`, which skips Cloudflare.

## Admin

Calls go to `http://127.0.0.1:8741` with `Authorization: Bearer $AS_CLOUD_ADMIN_TOKEN`; see `docs/CLOUD.md`.

- While there is no GPU backend (`[backend] kind = "none"`), scans stay paused: `POST /admin/pause {"paused": true}`.
- Accounts, codes and payments work while paused.
- The launch stage starts hidden: the app shows "not available yet" and the gateway takes no new computer, code, payment or scan. `/cloud stats` in Discord moves it to Beta (codes and passes, no payments) and Open (payments; refused until `gateway.env` has the live `sk_live_` key). Check it with `curl https://api.autoscene.io/v1/info`.
