//! What an account may scan: limits an admin sets, and limits a code brings
//! when it is redeemed (a beta pass's end with the pass). The account follows
//! the stricter of the two ([`Limits::stricter`]). The gateway enforces them;
//! the app reads the same rules to say which one a scan would break before
//! uploading anything.
//!
//! A scan counts at its price (on a beta pass too, where it is free): its
//! worth against the day's, the week's and the month's allowance. Periods are
//! calendar ones in UTC: the day from midnight, the week from Monday, the
//! month from the 1st.

use serde::{Deserialize, Serialize};

use crate::{Micros, ScanKind, Switch};

/// Limits on an account. A field left out doesn't limit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Scans worth at most this much a day, a week, a month (micro-euros).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_day: Option<Micros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_week: Option<Micros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_month: Option<Micros>,
    /// Scans started a day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scans_per_day: Option<u32>,
    /// The longest video a scan may take, in minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_minutes: Option<u32>,
    /// Only scans whose switches are all in here (none: every kind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<Switch>>,
}

/// Limits as an admin sends them: a field the gateway doesn't know is an
/// error, never a limit silently left out. (The app reads [`Limits`], which
/// skips fields a later gateway adds.)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_day: Option<Micros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_week: Option<Micros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_month: Option<Micros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scans_per_day: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<Switch>>,
}

impl From<LimitsInput> for Limits {
    fn from(i: LimitsInput) -> Self {
        Limits {
            per_day: i.per_day,
            per_week: i.per_week,
            per_month: i.per_month,
            scans_per_day: i.scans_per_day,
            max_minutes: i.max_minutes,
            kinds: i.kinds,
        }
    }
}

/// What an account used in the periods its limits count, and when each
/// starts again (Unix seconds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub day: Micros,
    pub week: Micros,
    pub month: Micros,
    pub scans_today: u32,
    pub day_ends: i64,
    pub week_ends: i64,
    pub month_ends: i64,
}

/// The limit a scan would break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "over", rename_all = "snake_case")]
pub enum Over {
    /// This kind of scan isn't among the account's.
    Kind,
    /// The video is longer than a scan may be.
    Length {
        max_minutes: u32,
    },
    /// The day's scans are used; more from `until`.
    Scans {
        per_day: u32,
        until: i64,
    },
    /// The scan is worth more than is `left` of the period's allowance.
    Day {
        limit: Micros,
        left: Micros,
        until: i64,
    },
    Week {
        limit: Micros,
        left: Micros,
        until: i64,
    },
    Month {
        limit: Micros,
        left: Micros,
        until: i64,
    },
}

impl Limits {
    /// No limit set.
    pub fn is_empty(&self) -> bool {
        *self == Limits::default()
    }

    /// Limits that can hold: amounts and counts above 0, at least one kind.
    pub fn check(&self) -> Result<(), &'static str> {
        if [self.per_day, self.per_week, self.per_month].iter().flatten().any(|m| *m <= 0) {
            return Err("an allowance must be above 0");
        }
        if self.scans_per_day == Some(0) || self.max_minutes == Some(0) {
            return Err("scans a day and minutes a scan must be above 0");
        }
        if self.kinds.as_ref().is_some_and(Vec::is_empty) {
            return Err("name at least one kind of scan");
        }
        Ok(())
    }

    /// Both at once: the lower of each amount and count, the kinds both
    /// allow.
    pub fn stricter(&self, other: &Limits) -> Limits {
        fn lower<T: Ord + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            }
        }
        Limits {
            per_day: lower(self.per_day, other.per_day),
            per_week: lower(self.per_week, other.per_week),
            per_month: lower(self.per_month, other.per_month),
            scans_per_day: lower(self.scans_per_day, other.scans_per_day),
            max_minutes: lower(self.max_minutes, other.max_minutes),
            kinds: match (&self.kinds, &other.kinds) {
                (Some(a), Some(b)) => Some(a.iter().filter(|k| b.contains(k)).copied().collect()),
                (a, b) => a.clone().or_else(|| b.clone()),
            },
        }
    }

    /// The first limit a scan of `kind`, `duration_ms` long and worth
    /// `worth`, breaks after `used`. `starting`: the scan isn't counted in
    /// `used.scans_today` yet.
    pub fn over(&self, used: &Usage, kind: &ScanKind, duration_ms: u64, worth: Micros, starting: bool) -> Option<Over> {
        if let Some(kinds) = &self.kinds
            && kind.switches().iter().any(|s| !kinds.contains(s))
        {
            return Some(Over::Kind);
        }
        if let Some(max) = self.max_minutes
            && duration_ms > u64::from(max) * 60_000
        {
            return Some(Over::Length { max_minutes: max });
        }
        if starting
            && let Some(n) = self.scans_per_day
            && used.scans_today >= n
        {
            return Some(Over::Scans { per_day: n, until: used.day_ends });
        }
        let room = |limit: Option<Micros>, spent: Micros| limit.filter(|l| spent.saturating_add(worth) > *l);
        if let Some(limit) = room(self.per_day, used.day) {
            return Some(Over::Day { limit, left: (limit - used.day).max(0), until: used.day_ends });
        }
        if let Some(limit) = room(self.per_week, used.week) {
            return Some(Over::Week { limit, left: (limit - used.week).max(0), until: used.week_ends });
        }
        if let Some(limit) = room(self.per_month, used.month) {
            return Some(Over::Month { limit, left: (limit - used.month).max(0), until: used.month_ends });
        }
        None
    }
}

const DAY: i64 = 86_400;

/// Midnight UTC of the day `t` is in.
pub fn day_start(t: i64) -> i64 {
    t.div_euclid(DAY) * DAY
}

/// Monday midnight UTC of the week `t` is in.
pub fn week_start(t: i64) -> i64 {
    let days = t.div_euclid(DAY);
    // 1970-01-01 was a Thursday: day 0 is three days after a Monday.
    (days - (days + 3).rem_euclid(7)) * DAY
}

/// The first of the month `t` is in, midnight UTC.
pub fn month_start(t: i64) -> i64 {
    // Howard Hinnant's days-from-civil and back.
    let days = t.div_euclid(DAY);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    (days - (d - 1)) * DAY
}

/// The first of the month after the one `t` is in.
pub fn next_month(t: i64) -> i64 {
    // 32 days past the 1st is always in the next month.
    month_start(month_start(t) + 32 * DAY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Depth, Footage};

    // 2026-11-19 12:00 UTC, a Thursday.
    const T: i64 = 1_795_089_600;

    fn kind(footage: Footage, depth: Depth) -> ScanKind {
        ScanKind { footage, depth, people: false, scenes: false }
    }

    fn used() -> Usage {
        Usage {
            day: 400_000,
            week: 1_500_000,
            month: 4_000_000,
            scans_today: 2,
            day_ends: day_start(T) + DAY,
            week_ends: week_start(T) + 7 * DAY,
            month_ends: next_month(T),
        }
    }

    #[test]
    fn periods_start_where_people_expect() {
        assert_eq!(day_start(T), 1_795_046_400); // 2026-11-19 00:00
        assert_eq!(week_start(T), 1_794_787_200); // Monday 2026-11-16
        assert_eq!(month_start(T), 1_793_491_200); // 2026-11-01
        assert_eq!(next_month(T), 1_796_083_200); // 2026-12-01
        assert_eq!(next_month(1_798_761_600 + 3600), 1_801_440_000); // 2027-01 → 2027-02-01
        assert_eq!(week_start(0), -3 * DAY, "1970-01-01 was a Thursday");
        assert_eq!(week_start(1_794_787_200), 1_794_787_200, "a Monday starts its own week");
        // 2024-02-29 is in February.
        assert_eq!(month_start(1_709_208_000), 1_706_745_600);
    }

    #[test]
    fn no_limits_never_refuse() {
        let k = kind(Footage::Anime, Depth::Best);
        assert_eq!(Limits::default().over(&used(), &k, 10 * 3_600_000, i64::MAX, true), None);
        assert!(Limits::default().is_empty());
    }

    #[test]
    fn each_limit_refuses_what_breaks_it() {
        let std_live = kind(Footage::Live, Depth::Standard);
        let kinds = Limits { kinds: Some(vec![Switch::Live, Switch::LiveFast]), ..Limits::default() };
        assert_eq!(kinds.over(&used(), &kind(Footage::Anime, Depth::Standard), 60_000, 1, true), Some(Over::Kind));
        assert_eq!(kinds.over(&used(), &std_live, 60_000, 1, true), None);
        let people = ScanKind { people: true, ..std_live };
        assert_eq!(kinds.over(&used(), &people, 60_000, 1, true), Some(Over::Kind), "people need their switch");

        let length = Limits { max_minutes: Some(30), ..Limits::default() };
        assert_eq!(length.over(&used(), &std_live, 30 * 60_000, 1, true), None);
        assert_eq!(length.over(&used(), &std_live, 30 * 60_000 + 1, 1, true), Some(Over::Length { max_minutes: 30 }));

        let scans = Limits { scans_per_day: Some(2), ..Limits::default() };
        let until = used().day_ends;
        assert_eq!(scans.over(&used(), &std_live, 60_000, 1, true), Some(Over::Scans { per_day: 2, until }));
        assert_eq!(scans.over(&used(), &std_live, 60_000, 1, false), None, "a started scan is counted already");

        let day = Limits { per_day: Some(500_000), ..Limits::default() };
        assert_eq!(day.over(&used(), &std_live, 60_000, 100_000, true), None, "exactly the allowance fits");
        assert_eq!(
            day.over(&used(), &std_live, 60_000, 100_001, true),
            Some(Over::Day { limit: 500_000, left: 100_000, until })
        );
        let week = Limits { per_week: Some(1_000_000), ..Limits::default() };
        assert_eq!(
            week.over(&used(), &std_live, 60_000, 1, true),
            Some(Over::Week { limit: 1_000_000, left: 0, until: used().week_ends })
        );
        let month = Limits { per_month: Some(5_000_000), ..Limits::default() };
        assert_eq!(
            month.over(&used(), &std_live, 60_000, 1_000_001, true),
            Some(Over::Month { limit: 5_000_000, left: 1_000_000, until: used().month_ends })
        );
    }

    #[test]
    fn two_sets_of_limits_make_the_stricter_one() {
        let admin =
            Limits { per_day: Some(1_000_000), kinds: Some(vec![Switch::Live, Switch::Anime]), ..Limits::default() };
        let pass = Limits {
            per_day: Some(2_000_000),
            per_month: Some(9_000_000),
            kinds: Some(vec![Switch::Anime, Switch::AnimeBest]),
            ..Limits::default()
        };
        let both = admin.stricter(&pass);
        assert_eq!(both.per_day, Some(1_000_000));
        assert_eq!(both.per_month, Some(9_000_000));
        assert_eq!(both.kinds, Some(vec![Switch::Anime]));
        assert_eq!(both, pass.stricter(&admin), "the order doesn't matter");
        assert_eq!(Limits::default().stricter(&pass), pass);
    }

    #[test]
    fn an_admins_limits_with_a_typo_are_refused() {
        assert!(serde_json::from_str::<LimitsInput>(r#"{"per_days": 100}"#).is_err());
        let ok: LimitsInput = serde_json::from_str(r#"{"per_day": 100}"#).unwrap();
        assert_eq!(Limits::from(ok).per_day, Some(100));
        // The app's side reads a later gateway's new fields without failing.
        assert!(serde_json::from_str::<Limits>(r#"{"per_day": 100, "per_year": 5}"#).is_ok());
    }

    #[test]
    fn limits_that_cant_hold_are_refused() {
        assert!(Limits::default().check().is_ok());
        assert!(Limits { per_day: Some(0), ..Limits::default() }.check().is_err());
        assert!(Limits { scans_per_day: Some(0), ..Limits::default() }.check().is_err());
        assert!(Limits { kinds: Some(vec![]), ..Limits::default() }.check().is_err());
        assert!(Limits { per_month: Some(1), max_minutes: Some(60), ..Limits::default() }.check().is_ok());
    }

    #[test]
    fn limits_travel_without_their_empty_fields() {
        let l = Limits { per_day: Some(2_000_000), kinds: Some(vec![Switch::LiveFast]), ..Limits::default() };
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(json, r#"{"per_day":2000000,"kinds":["live_fast"]}"#);
        assert_eq!(serde_json::from_str::<Limits>(&json).unwrap(), l);
        assert_eq!(serde_json::from_str::<Limits>("{}").unwrap(), Limits::default());
        let over = serde_json::to_string(&Over::Length { max_minutes: 30 }).unwrap();
        assert_eq!(over, r#"{"over":"length","max_minutes":30}"#);
    }
}
