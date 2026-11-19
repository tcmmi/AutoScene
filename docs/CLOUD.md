# AutoScene Cloud

Scans on a rented GPU instead of the user's computer, paid from a prepaid balance in euros. Three programs:

| Program | Crate | Where it runs |
|---|---|---|
| Gateway (`autoscene-cloud-gateway`) | `crates/as-cloud-gateway` | The owner's VDS, behind a TLS proxy |
| Worker (`autoscene-cloud-worker`) | `crates/as-cloud-worker` | A serverless GPU service, one per scan session, beside `autoscene-engine` |
| The app's client | `crates/as-cloud` (feature `client`) | The desktop app |

`crates/as-cloud` also holds the wire types, the prices and the top-up code format that all three share.

## How a scan runs

1. The app asks `GET /v1/info` how far the cloud is open (see [Launch stages](#launch-stages)). Unless it is hidden, the app registers its computer with `POST /v1/devices`.
   - It sends a salted hash of its hardware ids and gets back a device token.
   - The same computer gets the same account back, so a reinstall keeps the balance.
   - People see a balance and a Support ID, never a key.
2. The app opens a session with `POST /v1/sessions`.
   - It names the scan kind, the file's size and SHA-256, and its own duration measurement.
   - The gateway holds the estimate from the balance. A beta pass holds nothing.
3. The app uploads the original file in 8 MiB chunks: `PUT /v1/sessions/{id}/upload?offset=N`. A broken upload resumes.
4. The app calls `POST /v1/sessions/{id}/ready`.
   - The gateway checks the SHA-256 and measures the length with ffprobe.
   - It prices the scan from that length, never from the app's numbers, and holds the price.
   - It then starts a worker.
5. The app sends engine requests (`as_engine::protocol::Request`) to `POST /v1/sessions/{id}/jobs` and reads the engine's replies from `GET /v1/sessions/{id}/events?after=N&wait=25`.
   - Only `shots`, `faces` and `scenes` jobs are accepted, and a faces job must match the kind the session was opened for.
6. The app calls `POST /v1/sessions/{id}/close`. The charge depends on how the session ended:

   | How it ended | Charge |
   |---|---|
   | The faces pass finished | The whole price |
   | Anything else (the app stopped, a job failed, the cloud failed) | The share the faces pass reached as the engine reported it; 10% if only shots or scenes moved |

   - A charge is never less than the GPU time the session held (`guard.gpu_per_hour` × seconds since the worker's launch) and never more than the price. A job failure is never free, because the app can cause one.
   - The rest of the hold is given back, and the upload is deleted.
   - The app gets the replies the charge saw and no later ones.
   - A gateway restart charges each open session the share saved in the database.

The worker fetches the file in ranges (`/v1/worker/{id}/file`) and checks its SHA-256. It then reads the file itself with `as_media::probe` and `build_frame_index`, the app's own code, and refuses any job whose `info` or `index` isn't that file's. So the app can't pick another stream than the one priced, nor shrink the frames a job's progress counts against.

For that check to pass, the worker image must carry the same pinned FFmpeg build as the app. It is given with `--ffmpeg DIR`, and `--no-verify` is for tests only.

The worker points each job at its local copy, feeds the engine, and posts every reply line back. Both directions carry cursors (`after`, `first`), so a lost answer neither loses nor repeats lines. It ends in any of these cases:
- the app closes the session;
- `worker_idle_secs` (15 minutes by default) pass without a job while no job is open (an engine `fatal` frees open jobs);
- the session's deadline arrives: 30 minutes plus four times the video's length.

Its token is good for its own session only.

## In the app

- **Settings → Cloud** shows the balance (never a key), about how many minutes it pays for, the Support ID, the prices, "Add funds" (Stripe's page in the browser) and "Redeem a code". A switch sends new analyses to the cloud. It can always be turned off, even when the account can't be read. What shows depends on the [launch stage](#launch-stages).
- **Connecting:** the gateway's address is the library setting `cloud.gateway`, else `AUTOSCENE_CLOUD_URL`, else the built-in `https://api.autoscene.io`. Only https is accepted, or plain http to this computer.
- **The device token** lives in the system's keychain (service "AutoScene Cloud"). The hardware id the gateway sees is a SHA-256 of the machine id, salted with the gateway's address.
- **A cloud analysis** (`app/src-tauri/src/cloud.rs`, `work.rs`):
  1. A quick account check refuses what the gateway would refuse (blocked, paused, kind off, balance).
  2. The file is hashed, uploaded (resuming after a break) and measured.
  3. The usual passes run on an `Engine::remote` whose jobs go to the session.
  4. The session is closed when the analysis ends, however it ends, and on quit.
  5. A second connect after a failed job resumes the replies where they were.
- **Session scope:** the scan kind is fixed when the session opens. Changing Fast Scan, Best, 3D, people or scenes during a cloud analysis makes the later jobs refused.

## Launch stages

The owner moves the cloud through three stages with the Discord bot (`/cloud stats`, `POST /admin/stage`). The stage is the gateway's setting `stage`; a gateway that never had one is hidden. Anyone can ask it with `GET /v1/info` (`{"stage": "hidden" | "beta" | "open"}`), and every account reply carries it.

| Stage | The app shows | The gateway takes |
|---|---|---|
| `hidden` | "isn't available yet", and the switch only while it is still on (to turn it off) | No new computer, code, payment or scan (`closed`); a file uploaded before the switch starts no GPU and costs nothing (`ready` refuses it). An account that exists can still be read. The Stripe webhook and running workers go on. |
| `beta` | Balance, "Redeem a code", the switch and a closed-beta note; no "Add funds" | Codes (beta passes, balance an admin gave) and scans; no payment page (`closed`) |
| `open` | Everything | Everything |

- **Opening needs real money:** `open` is refused unless Stripe's key is live (`sk_live_…` or `rk_live_…`), and a stored `open` acts as the beta if the key is later swapped for a test one. A staging gateway can allow test mode with `stripe.test_mode_ok = true`.
- **Before the launch the app makes no account:** it asks `GET /v1/info` first and registers only when the cloud isn't hidden.
- **An analysis in the beta** without a pass and without enough balance is refused with "redeem a beta code", never "add funds".
- **Going back to hidden** after people paid hides their balance in the app. Use the pause instead to stop scans for a while.

## Prices and money

- Prices per video-minute: €0.01 standard, €0.005 Fast Scan, €0.0125 Best (`Prices`). They can be overridden in the config.
- Top-ups are €5, €10 and €20 (`TOP_UPS_CENTS`). Checkout sells the Stripe catalog's price for the amount (lookup key `cloud_topup_<cents>`, product "AutoScene Cloud balance" with its picture) when there is one in euros for exactly that amount, else the amount itself. The live account and the sandbox both have the three prices.
- The ledger counts micro-euros and only ever grows: `topup`, `code`, `charge`, `refund`, `adjust`, `transfer_in`, `transfer_out`.
- What a user can spend is the ledger sum less the holds of running scans.
- Payments, codes, charges and refunds each count once (unique indexes), so a replayed Stripe event changes nothing.
- A refund or dispute for a payment not seen yet is answered with an error, and Stripe sends it again later.
- The price comes from the longest video stream in the file: stated lengths, the last packet's time, and the packets counted at no more than 60 fps. A forged header can't shorten it.
- Stripe events:

  | Event | What the gateway does |
  |---|---|
  | `checkout.session.completed` or `checkout.session.async_payment_succeeded` (euros, paid) | Credits the account |
  | `charge.refunded` | Debits the refunded part not taken yet |
  | `charge.dispute.created` | Blocks the account |

## Running the gateway

```toml
# /etc/autoscene-cloud/gateway.toml
listen = "127.0.0.1:8740"
admin_listen = "127.0.0.1:8741"
public_base = "https://cloud.example.com"
data_dir = "/var/lib/autoscene-cloud"
trust_proxy = true
ffprobe = "/usr/bin/ffprobe"

[guard]
alert = 25_000_000        # €25 of GPU this month: post to the alert webhook
stop = 75_000_000         # €75: open no more sessions
gpu_per_hour = 740_000    # what an hour of a worker costs, in micro-euros

[stripe]
success_url = "https://cloud.example.com/paid"
cancel_url = "https://cloud.example.com/cancelled"
# invoice_footer = "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet."
# test_mode_ok = true     # staging only: let the cloud open without a live key

[backend]
kind = "http"             # or "local" (worker = "/path", args = [...]) or "none"
url = "https://api.example-gpu.com/v2/<endpoint>/run"
authorization = "Bearer …"
wrap = "input"
```

Secrets can stay out of the file, in the environment:

| Variable | What it is |
|---|---|
| `AS_CLOUD_ADMIN_TOKEN` | The bot's token |
| `AS_CLOUD_PEPPER` | Keys every stored hash of a code or a computer. Set it once and never change it. |
| `STRIPE_SECRET_KEY` | Stripe's secret key |
| `STRIPE_WEBHOOK_SECRET` | Stripe's webhook signing secret |
| `AS_CLOUD_ALERT_WEBHOOK` | A Discord webhook for the budget alert |

The proxy (Caddy):

```
cloud.example.com {
    reverse_proxy 127.0.0.1:8740
    request_body { max_size 100MB }
}
```

Run it as a systemd service: `ExecStart=/opt/autoscene-cloud/autoscene-cloud-gateway /etc/autoscene-cloud/gateway.toml`.

- The database is `data_dir/gateway.db` (SQLite, WAL), with a daily copy in `data_dir/backups/`. The last 14 copies are kept; copy them off the machine.
- A restart closes the open sessions without charge and deletes their uploads.

In Stripe, add a webhook endpoint `https://cloud.example.com/v1/stripe/webhook` with these events:
- `checkout.session.completed`
- `checkout.session.async_payment_succeeded`
- `charge.refunded`
- `charge.dispute.created`

Its signing secret is `STRIPE_WEBHOOK_SECRET`. PayPal shows up in Checkout once it is switched on under Settings → Payment methods.

## Admin API (the Discord bot)

Admin calls go to `admin_listen` only. Each call carries:
- `Authorization: Bearer <AS_CLOUD_ADMIN_TOKEN>`
- `X-Actor: <discord user>`, for the audit log

| Call | Body → reply |
|---|---|
| `POST /admin/codes` | `{"worth": {"kind": "credit", "cents": 500} \| {"kind": "beta", "until": <unix>}, "count": 1..500, "expires": <unix>?, "note": "…"?}` → `{"codes": ["XXXX-XXXX-XXXX-XXXX"]}`. The codes are shown once and stored only hashed. |
| `POST /admin/codes/revoke` | `{"code": "…"}` → `{"revoked": bool}` |
| `GET /admin/accounts/{support_id}` | → balance, held, beta pass, block, device, the last 25 ledger rows and the last 10 sessions |
| `POST /admin/accounts/{id}/adjust` | `{"amount": micros, "note": "…"}` |
| `POST /admin/accounts/{id}/block` | `{"blocked": bool, "reason": "…"?}` |
| `POST /admin/accounts/{id}/reset_device` | The device token stops working; the computer's next visit gets a new one |
| `POST /admin/transfer` | `{"from": "AS-…", "to": "AS-…", "note": "receipt cs_…"}` → `{"moved": micros}`. Moves the whole balance and the beta pass. Refused while a scan runs. |
| `GET` or `POST /admin/switches` | `{"switch": "live" \| "live_fast" \| "animated" \| "animated_fast" \| "anime" \| "anime_fast" \| "anime_best" \| "people" \| "scenes", "on": bool}` |
| `POST /admin/pause` | `{"paused": bool}` → stats. Paused, no session opens; accounts, codes and payments still work |
| `POST /admin/stage` | `{"stage": "hidden" \| "beta" \| "open"}` → stats. `open` is refused (400) without Stripe's live key unless `stripe.test_mode_ok`. See [Launch stages](#launch-stages). |
| `GET /admin/stats` | This month: paid in, refunded, charged, sessions, GPU seconds and spend, guard thresholds, paused, open sessions; the stage as it acts (`stage`) and whether Stripe's key is live (`live_payments`) |

## Abuse limits

| What | Limit |
|---|---|
| Wrong codes | 5 an hour per account, 20 an hour per address |
| Registrations | 30 an hour per address |
| Payment pages | 30 an hour per account |
| Open sessions | One per account, two per address |
| Upload size | `max_upload_bytes` per file and `max_uploads_total` in all |
| Upload holding | The estimate holds at least the price of one second per 2 MB of file, so an empty balance can't park big files |
| Stalled uploads | End after 30 minutes without bytes; asking how far an upload is doesn't keep it alive |
| Jobs per session | 64 lines, 64 MB per request, 256 MB in all |
| Unread replies | Past 512 MB the scan stops (what it did is still charged) |
| Measuring | ffprobe runs 5 minutes at most per pass; a session measuring for over an hour ends |
| Request bodies | Read only after the token is checked |

Codes are 75 random bits with a check symbol. They and the computer hashes are stored only as keyed hashes (`pepper`).

Known limits:
- A computer's hardware hash also finds its account again after a reinstall. Someone who knows another computer's hardware ids could take its account over. This is the price of device-only accounts without a visible key.
- The worker trusts the engine's own progress reports. The engine runs on the owner's image, not the user's computer.

## Tests

- `cargo test -p as-cloud -p as-cloud-gateway -p as-cloud-worker --all-features` runs the end-to-end test: the gateway on a local port, the real worker with `--fake-engine`, and the app's client.
- `STRIPE_SECRET_KEY=sk_test_… cargo test -p as-cloud-gateway live_tests -- --ignored` makes a real Checkout page in Stripe's test mode.
