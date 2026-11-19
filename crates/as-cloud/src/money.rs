//! Prices and euro amounts.

use serde::{Deserialize, Serialize};

use crate::{Depth, ScanKind};

/// An amount of money in millionths of a euro: a minute of Fast Scan costs
/// half a cent, which cents can't hold.
pub type Micros = i64;

pub const MICROS_PER_CENT: Micros = 10_000;
pub const MICROS_PER_EURO: Micros = 1_000_000;

/// The top-ups the app offers, in cents (€5, €10, €20). The gateway sells
/// no other amount.
pub const TOP_UPS_CENTS: [i64; 3] = [500, 1000, 2000];

/// What a minute of video costs, by how deep the scan looks. People and the
/// scenes pass are included in every scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prices {
    pub standard: Micros,
    pub fast: Micros,
    pub best: Micros,
}

impl Default for Prices {
    /// €0.01, €0.005 and €0.0125 a minute.
    fn default() -> Self {
        Self { standard: 10_000, fast: 5_000, best: 12_500 }
    }
}

impl Prices {
    pub fn per_minute(&self, kind: &ScanKind) -> Micros {
        match kind.depth {
            Depth::Standard => self.standard,
            Depth::Fast => self.fast,
            Depth::Best => self.best,
        }
    }

    /// The price of scanning `duration_ms` of video, rounded up to the
    /// micro-euro.
    pub fn price(&self, kind: &ScanKind, duration_ms: u64) -> Micros {
        let rate = i128::from(self.per_minute(kind).max(0));
        let total = (i128::from(duration_ms) * rate + 59_999) / 60_000;
        Micros::try_from(total).unwrap_or(Micros::MAX)
    }
}

/// What a gateway's prices follow, shown to anyone who asks: the GPU
/// service's rate and the euro's, as read at `at`. A gateway with fixed
/// prices has none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceBasis {
    /// The dearest GPU a scan may run on, in US dollars an hour.
    pub gpu_usd_per_hour: f64,
    /// US dollars for a euro (the European Central Bank's reference rate).
    pub usd_per_eur: f64,
    /// When both were read (Unix seconds).
    pub at: i64,
}

/// `m` as people read it: euros with two decimals, rounded down, so a
/// balance never shows money that isn't there ("€4.99", "-€0.20").
pub fn euros(m: Micros) -> String {
    let cents = m.div_euclid(MICROS_PER_CENT);
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!("{sign}€{}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Footage;

    fn kind(depth: Depth) -> ScanKind {
        ScanKind { footage: Footage::Anime, depth, people: false, scenes: false }
    }

    #[test]
    fn a_minute_costs_its_rate_and_parts_round_up() {
        let p = Prices::default();
        assert_eq!(p.price(&kind(Depth::Standard), 60_000), 10_000);
        assert_eq!(p.price(&kind(Depth::Fast), 60_000), 5_000);
        assert_eq!(p.price(&kind(Depth::Best), 60_000), 12_500);
        // A 24-minute episode of a standard scan: 24 cents.
        assert_eq!(p.price(&kind(Depth::Standard), 24 * 60_000), 240_000);
        // One millisecond still costs something.
        assert_eq!(p.price(&kind(Depth::Standard), 1), 1);
        assert_eq!(p.price(&kind(Depth::Standard), 0), 0);
    }

    #[test]
    fn euros_round_down_to_the_cent() {
        assert_eq!(euros(0), "€0.00");
        assert_eq!(euros(4_999_999), "€4.99");
        assert_eq!(euros(10 * MICROS_PER_EURO), "€10.00");
        assert_eq!(euros(-200_000), "-€0.20");
        assert_eq!(euros(-1), "-€0.01");
    }
}
