//! Prices that follow the GPU service: what a minute of video costs is
//! worked out from RunPod's serverless rate for the dearest GPU the endpoint
//! may run on, the European Central Bank's dollar rate, and how many GPU
//! seconds a minute of video takes, times a markup. Read every few hours;
//! the last prices read are kept in the database, so a gateway that can't
//! reach either source goes on with them (or with the fixed `[prices]`).

use as_cloud::{MICROS_PER_EURO, Micros, PriceBasis, Prices};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// RunPod's GPU catalog: every GPU type with its serverless pool and rate.
pub const RUNPOD_CATALOG: &str = "https://api.runpod.io/v2/catalog/gpus";
/// The ECB's euro reference rates of the day.
pub const ECB_DAILY: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

/// A new price within this share of the one in force (or within a step of
/// it, whichever is more) is no change: the dollar's daily drift doesn't move
/// what people see.
const STEADY: f64 = 0.03;

/// Rates outside these are a misread (another unit, a wrong row), never a
/// price.
const GPU_USD_PER_HOUR: (f64, f64) = (0.05, 20.0);
const USD_PER_EUR: (f64, f64) = (0.5, 2.0);

/// A read that moves a rate by more than this share is held back: the
/// prices in force stay until the owner looks (or the rate comes back).
const JUMP: f64 = 0.5;

/// After a failed read, the next try comes this soon.
pub const RETRY_SECS: u64 = 15 * 60;

/// `[pricing]` in the gateway's settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PricingConfig {
    /// The RunPod pools the endpoint may run on (`AMPERE_24`, `ADA_24`):
    /// a scan may land on any of them, so the dearest GPU in them counts.
    pub pools: Vec<String>,
    /// GPU seconds RunPod bills for a minute of video, by depth: the
    /// heaviest footage with people and scenes, and a session's start and
    /// idle time spread over a short episode.
    pub gpu_secs_per_min: SecsPerMin,
    /// Price over cost (at least 1.1: the hold may keep a price up to a step
    /// below the cost it follows). Stripe's fees, the server, scans that fail
    /// and the income all come out of it.
    pub markup: f64,
    /// Prices are rounded up to this many micro-euros (€0.0005): a whole
    /// number of €0.0001, the finest the app shows.
    #[serde(default = "step")]
    pub step: Micros,
    #[serde(default = "refresh_hours")]
    pub refresh_hours: u64,
    /// A RunPod key that may read the catalog (`catalog:read`), or
    /// `RUNPOD_API_KEY`; without either, the HTTP backend's own key.
    #[serde(default)]
    pub runpod_key: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecsPerMin {
    pub standard: f64,
    pub fast: f64,
    pub best: f64,
}

const fn step() -> Micros {
    500
}
const fn refresh_hours() -> u64 {
    6
}

/// The prices in force and what they follow.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub prices: Prices,
    /// What an hour of a worker costs (micro-euros): the GPU-time floor of a
    /// scan's charge and the month's budget.
    pub gpu_per_hour: Micros,
    /// `None` while the fixed prices are in force.
    pub basis: Option<PriceBasis>,
}

impl PricingConfig {
    pub fn check(&self) -> Result<(), &'static str> {
        let s = self.gpu_secs_per_min;
        if self.pools.is_empty() {
            return Err("pricing.pools: name at least one RunPod pool");
        }
        if ![s.standard, s.fast, s.best].iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err("pricing.gpu_secs_per_min: every depth needs a number above 0");
        }
        if !(self.markup.is_finite() && self.markup >= 1.1) {
            return Err("pricing.markup must be 1.1 or more: a lower one can sell below cost");
        }
        if self.step <= 0 || self.step % 100 != 0 || self.step > 100_000 {
            return Err("pricing.step must be a multiple of 100 (€0.0001), at most 100000");
        }
        if !(1..=168).contains(&self.refresh_hours) {
            return Err("pricing.refresh_hours must be 1 to 168");
        }
        if self.runpod_key.is_empty() {
            return Err("pricing needs a RunPod key that may read the catalog (RUNPOD_API_KEY)");
        }
        Ok(())
    }

    /// Each depth's price before rounding: its GPU cost a minute times the
    /// markup, in micro-euros.
    fn raw(&self, basis: PriceBasis) -> [f64; 3] {
        let micros_per_sec = basis.gpu_usd_per_hour / basis.usd_per_eur / 3600.0 * MICROS_PER_EURO as f64;
        let s = self.gpu_secs_per_min;
        [s.standard, s.fast, s.best].map(|secs| secs * micros_per_sec * self.markup)
    }

    /// `raw` rounded up to the step, one step at least.
    fn round(&self, raw: f64) -> Micros {
        let steps = (raw / self.step as f64).ceil().clamp(1.0, 1e9) as Micros;
        steps.saturating_mul(self.step)
    }

    /// The book a read of `basis` makes, after `old`: each depth's price is
    /// its GPU cost a minute times the markup, rounded up to the step, unless
    /// that is within [`STEADY`] (or a step) of the price in force, which
    /// then stays. A misread rate, or one that jumped, makes none.
    pub fn next(&self, old: &Book, basis: PriceBasis) -> Result<Book, String> {
        let within = |v: f64, (lo, hi): (f64, f64)| v.is_finite() && (lo..=hi).contains(&v);
        if !within(basis.gpu_usd_per_hour, GPU_USD_PER_HOUR) || !within(basis.usd_per_eur, USD_PER_EUR) {
            return Err(format!(
                "rates out of bounds (GPU ${}/h, ${}/€): a misread, the prices stay",
                basis.gpu_usd_per_hour, basis.usd_per_eur
            ));
        }
        if let Some(was) = old.basis {
            let jump = |a: f64, b: f64| ((a - b) / b).abs() > JUMP;
            if jump(basis.gpu_usd_per_hour, was.gpu_usd_per_hour) || jump(basis.usd_per_eur, was.usd_per_eur) {
                return Err(format!(
                    "rates jumped (GPU ${}/h to ${}/h, ${}/€ to ${}/€): the prices stay until someone looks",
                    was.gpu_usd_per_hour, basis.gpu_usd_per_hour, was.usd_per_eur, basis.usd_per_eur
                ));
            }
        }
        let held = old.basis.is_some();
        let band = |o: Micros| (STEADY * o as f64).max(self.step as f64);
        let price = |o: Micros, raw: f64| {
            if held && o > 0 && (raw - o as f64).abs() < band(o) { o } else { self.round(raw) }
        };
        let [standard, fast, best] = self.raw(basis);
        Ok(Book {
            prices: Prices {
                standard: price(old.prices.standard, standard),
                fast: price(old.prices.fast, fast),
                best: price(old.prices.best, best),
            },
            gpu_per_hour: (basis.gpu_usd_per_hour / basis.usd_per_eur * MICROS_PER_EURO as f64).round() as Micros,
            basis: Some(basis),
        })
    }
}

impl Book {
    /// A stored book fit to be in force: prices and the GPU rate above 0.
    pub fn sane(&self) -> bool {
        let p = self.prices;
        [p.standard, p.fast, p.best, self.gpu_per_hour].iter().all(|v| *v > 0)
    }
}

/// The dearest serverless rate (US dollars an hour, for one GPU: the
/// endpoint gives a worker one) of the GPUs in `pools`, from RunPod's catalog
/// (`{"gpus": […]}`).
pub fn dearest_rate(catalog: &Value, pools: &[String]) -> Option<f64> {
    let list = catalog["gpus"].as_array()?;
    list.iter()
        .filter(|g| g["pool"].as_str().is_some_and(|p| pools.iter().any(|q| q == p)))
        .filter_map(|g| g["price"]["serverless"].as_f64())
        .filter(|r| r.is_finite() && *r > 0.0)
        .reduce(f64::max)
}

/// US dollars for a euro from the ECB's daily XML (`currency='USD' rate='…'`).
pub fn usd_per_eur(xml: &str) -> Option<f64> {
    let at = xml.find("currency='USD'").or_else(|| xml.find("currency=\"USD\""))?;
    let rest = &xml[at..];
    let start = rest.find("rate=")? + "rate=".len();
    let quoted = &rest[start..];
    let q = quoted.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let end = quoted[1..].find(q)?;
    let rate: f64 = quoted[1..=end].parse().ok()?;
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

/// Read both rates now.
pub fn read_basis(cfg: &PricingConfig, key: &str, now: i64) -> Result<PriceBasis, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let get = |url: &str, auth: Option<&str>| -> Result<String, String> {
        let mut req = agent.get(url).header("User-Agent", "autoscene-cloud-gateway");
        if let Some(key) = auth {
            req = req.header("Authorization", &format!("Bearer {key}"));
        }
        let mut resp = req.call().map_err(|e| format!("{url}: {e}"))?;
        let body = resp.body_mut().read_to_string().map_err(|e| format!("{url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("{url} answered {}: {}", resp.status(), body.chars().take(200).collect::<String>()));
        }
        Ok(body)
    };
    let catalog: Value =
        serde_json::from_str(&get(RUNPOD_CATALOG, Some(key))?).map_err(|e| format!("RunPod's catalog: {e}"))?;
    let gpu_usd_per_hour = dearest_rate(&catalog, &cfg.pools)
        .ok_or_else(|| format!("RunPod's catalog has no serverless rate for {:?}", cfg.pools))?;
    let usd_per_eur = usd_per_eur(&get(ECB_DAILY, None)?).ok_or("the ECB's rates have no US dollar")?;
    Ok(PriceBasis { gpu_usd_per_hour, usd_per_eur, at: now })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> PricingConfig {
        PricingConfig {
            pools: vec!["AMPERE_24".into(), "ADA_24".into()],
            gpu_secs_per_min: SecsPerMin { standard: 20.0, fast: 10.0, best: 40.0 },
            markup: 2.0,
            step: 500,
            refresh_hours: 6,
            runpod_key: "rpa_test".into(),
        }
    }

    fn fixed() -> Book {
        Book { prices: Prices::default(), gpu_per_hour: 740_000, basis: None }
    }

    fn basis(gpu: f64, usd: f64) -> PriceBasis {
        PriceBasis { gpu_usd_per_hour: gpu, usd_per_eur: usd, at: 7 }
    }

    #[test]
    fn the_dearest_gpu_of_the_pools_counts() {
        // The shape of RunPod's `GET /v2/catalog/gpus` (`ListGpuTypesResponse`).
        let catalog = json!({"gpus": [
            {"id": "NVIDIA GeForce RTX 4090", "pool": "ADA_24", "price": {"secure": 0.89, "community": 0.34, "serverless": 1.34}},
            {"id": "NVIDIA RTX A5000", "pool": "AMPERE_24", "price": {"secure": 0.27, "community": 0.16, "serverless": 0.75}},
            {"id": "NVIDIA H100 80GB HBM3", "pool": "HOPPER_141", "price": {"secure": 2.99, "community": 2.69, "serverless": 4.18}},
            {"id": "a pod-only GPU", "pool": null, "price": {"secure": 9.0, "community": 9.0}}
        ]});
        assert_eq!(dearest_rate(&catalog, &cfg().pools), Some(1.34));
        assert_eq!(dearest_rate(&catalog, &["AMPERE_24".into()]), Some(0.75));
        assert_eq!(dearest_rate(&catalog, &["NONE".into()]), None);
        assert_eq!(dearest_rate(&json!({"detail": "forbidden"}), &cfg().pools), None);
        assert_eq!(dearest_rate(&catalog["gpus"], &cfg().pools), None, "a bare list isn't RunPod's answer");
    }

    #[test]
    fn the_ecb_dollar_rate_is_read() {
        let xml =
            "<Cube time='2026-11-19'>\n<Cube currency='USD' rate='1.0812'/>\n<Cube currency='JPY' rate='162.31'/>";
        assert_eq!(usd_per_eur(xml), Some(1.0812));
        assert_eq!(usd_per_eur("<Cube currency=\"USD\" rate=\"1.05\"/>"), Some(1.05));
        assert_eq!(usd_per_eur("<Cube currency='JPY' rate='162.31'/>"), None);
        assert_eq!(usd_per_eur("<Cube currency='USD' rate='x'/>"), None);
        // Unquoted, it isn't read as ".08".
        assert_eq!(usd_per_eur("<Cube currency='USD' rate=1.0812/>"), None);
    }

    #[test]
    fn prices_are_cost_times_markup_rounded_up() {
        // $1.08 an hour at $1.08 a euro: €1 an hour.
        let book = cfg().next(&fixed(), basis(1.08, 1.08)).unwrap();
        // 20 s × €1/3600 s × 2 = €0.01111 → €0.0115.
        assert_eq!(book.prices, Prices { standard: 11_500, fast: 6_000, best: 22_500 });
        assert_eq!(book.gpu_per_hour, 1_000_000);
        assert_eq!(book.basis, Some(basis(1.08, 1.08)));
        // Never below one step, however cheap the GPU.
        let cheap = cfg().next(&fixed(), basis(0.05, 2.0)).unwrap();
        assert_eq!(cheap.prices.fast, 500);
    }

    #[test]
    fn small_moves_keep_the_price_and_big_ones_move_it() {
        let first = cfg().next(&fixed(), basis(1.08, 1.08)).unwrap();
        // Raw 11,111 rounded to 11,500: a drift across the step's edge (raw
        // 11,550) is within a step of 11,500 and changes nothing, back and
        // forth.
        let drift = cfg().next(&first, basis(1.08 * 1.0395, 1.08)).unwrap();
        assert_eq!(drift.prices, first.prices);
        let back = cfg().next(&drift, basis(1.08, 1.08)).unwrap();
        assert_eq!(back.prices, first.prices);
        // 20% dearer moves every price.
        let dearer = cfg().next(&first, basis(1.296, 1.08)).unwrap();
        assert_eq!(dearer.prices, Prices { standard: 13_500, fast: 7_000, best: 27_000 });
        assert_eq!(dearer.gpu_per_hour, 1_200_000);
    }

    #[test]
    fn misread_or_jumping_rates_change_nothing() {
        let first = cfg().next(&fixed(), basis(1.34, 1.08)).unwrap();
        // A rate a second instead of an hour; a euro read as eight cents.
        assert!(cfg().next(&first, basis(0.000_372, 1.08)).is_err());
        assert!(cfg().next(&fixed(), basis(1.34, 0.08)).is_err());
        assert!(cfg().next(&fixed(), basis(f64::NAN, 1.08)).is_err());
        // Half again or more from the rate in force waits for the owner.
        assert!(cfg().next(&first, basis(2.1, 1.08)).is_err());
        assert!(cfg().next(&first, basis(0.6, 1.08)).is_err());
        assert!(cfg().next(&first, basis(1.9, 1.08)).is_ok());
    }

    #[test]
    fn a_stored_book_must_be_sane() {
        assert!(fixed().sane());
        assert!(!Book { gpu_per_hour: 0, ..fixed() }.sane());
        assert!(!Book { prices: Prices { standard: 0, ..Prices::default() }, ..fixed() }.sane());
    }

    #[test]
    fn settings_that_sell_below_cost_are_refused() {
        assert!(cfg().check().is_ok());
        assert!(PricingConfig { markup: 1.05, ..cfg() }.check().is_err());
        assert!(PricingConfig { pools: vec![], ..cfg() }.check().is_err());
        let mut c = cfg();
        c.gpu_secs_per_min.fast = 0.0;
        assert!(c.check().is_err());
        assert!(PricingConfig { step: 0, ..cfg() }.check().is_err());
        assert!(PricingConfig { step: 250, ..cfg() }.check().is_err(), "the app shows four decimals");
        assert!(PricingConfig { refresh_hours: 0, ..cfg() }.check().is_err());
        assert!(PricingConfig { refresh_hours: 1_000_000, ..cfg() }.check().is_err());
        assert!(PricingConfig { runpod_key: String::new(), ..cfg() }.check().is_err());
    }
}
