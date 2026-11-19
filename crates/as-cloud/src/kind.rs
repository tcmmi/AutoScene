//! What a scan session is for: the footage, how deep it looks, and the
//! switches the owner can turn each kind off with.

use serde::{Deserialize, Serialize};

/// The footage a scan is made for, as the engine's faces job tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Footage {
    /// Live action, Western 2D animation and mixed footage: SCRFD and ArcFace.
    Live,
    /// 3D animation: the same models looking at more sizes (the faces job's
    /// `animated` flag).
    Animated,
    /// Anime: head and person detectors and CCIP (the `anime` flag).
    Anime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    Standard,
    /// Fast Scan (the `fast` flag).
    Fast,
    /// The Best anime scan (the `best` flag): anime only.
    Best,
}

/// A scan session's kind, said when the session opens: its price follows
/// from it, and the gateway refuses any job that doesn't fit it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScanKind {
    pub footage: Footage,
    pub depth: Depth,
    /// People are looked for too (the faces job's `bodies`, or a later job
    /// with `known` faces).
    #[serde(default)]
    pub people: bool,
    /// The scenes pass runs too.
    #[serde(default)]
    pub scenes: bool,
}

/// A kind of scan the owner can turn off for new paid sessions, each on its
/// own (a licence that stops, a model that misbehaves).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    Live,
    LiveFast,
    Animated,
    AnimatedFast,
    Anime,
    AnimeFast,
    AnimeBest,
    People,
    Scenes,
}

impl Switch {
    pub const ALL: [Switch; 9] = [
        Switch::Live,
        Switch::LiveFast,
        Switch::Animated,
        Switch::AnimatedFast,
        Switch::Anime,
        Switch::AnimeFast,
        Switch::AnimeBest,
        Switch::People,
        Switch::Scenes,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Switch::Live => "live",
            Switch::LiveFast => "live_fast",
            Switch::Animated => "animated",
            Switch::AnimatedFast => "animated_fast",
            Switch::Anime => "anime",
            Switch::AnimeFast => "anime_fast",
            Switch::AnimeBest => "anime_best",
            Switch::People => "people",
            Switch::Scenes => "scenes",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// The flags of an engine faces job (`as_engine::protocol::FacesJob`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FacesFlags {
    pub anime: bool,
    pub fast: bool,
    pub best: bool,
    pub animated: bool,
    pub bodies: bool,
    /// Faces found before were sent: only people are looked for.
    pub known: bool,
}

impl ScanKind {
    /// Why this kind can't be a scan, if it can't: the engine's own rules.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.depth == Depth::Best && self.footage != Footage::Anime {
            return Err("the Best scan is for anime only");
        }
        if self.people && (self.footage == Footage::Anime || self.depth == Depth::Fast) {
            return Err("people are looked for only by the standard scan of live action or 3D animation");
        }
        Ok(())
    }

    /// The switches that must be on for it.
    pub fn switches(&self) -> Vec<Switch> {
        let main = match (self.footage, self.depth) {
            (Footage::Live, Depth::Fast) => Switch::LiveFast,
            (Footage::Live, _) => Switch::Live,
            (Footage::Animated, Depth::Fast) => Switch::AnimatedFast,
            (Footage::Animated, _) => Switch::Animated,
            (Footage::Anime, Depth::Standard) => Switch::Anime,
            (Footage::Anime, Depth::Fast) => Switch::AnimeFast,
            (Footage::Anime, Depth::Best) => Switch::AnimeBest,
        };
        let mut all = vec![main];
        if self.people {
            all.push(Switch::People);
        }
        if self.scenes {
            all.push(Switch::Scenes);
        }
        all
    }

    /// Whether a faces job with these flags belongs to a session of this
    /// kind: a session sold as a Fast Scan can't run the standard one.
    pub fn admits(&self, f: &FacesFlags) -> bool {
        let anime = self.footage == Footage::Anime;
        if f.anime != anime || f.fast != (self.depth == Depth::Fast) || f.best != (self.depth == Depth::Best) {
            return false;
        }
        // The `animated` flag means nothing to anime's models.
        if !anime && f.animated != (self.footage == Footage::Animated) {
            return false;
        }
        !(f.bodies || f.known) || self.people
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn kind(footage: Footage, depth: Depth, people: bool) -> ScanKind {
        ScanKind { footage, depth, people, scenes: false }
    }

    #[test]
    fn the_engines_own_rules_hold() {
        assert!(kind(Footage::Anime, Depth::Best, false).check().is_ok());
        assert!(kind(Footage::Live, Depth::Best, false).check().is_err());
        assert!(kind(Footage::Live, Depth::Standard, true).check().is_ok());
        assert!(kind(Footage::Live, Depth::Fast, true).check().is_err());
        assert!(kind(Footage::Anime, Depth::Standard, true).check().is_err());
    }

    #[test]
    fn a_session_runs_only_the_scan_it_was_sold_as() {
        let fast = kind(Footage::Live, Depth::Fast, false);
        assert!(fast.admits(&FacesFlags { fast: true, ..Default::default() }));
        assert!(!fast.admits(&FacesFlags::default()));
        let anime = kind(Footage::Anime, Depth::Standard, false);
        assert!(anime.admits(&FacesFlags { anime: true, ..Default::default() }));
        assert!(anime.admits(&FacesFlags { anime: true, animated: true, ..Default::default() }));
        assert!(!anime.admits(&FacesFlags { anime: true, best: true, ..Default::default() }));
        let cgi = kind(Footage::Animated, Depth::Standard, false);
        assert!(cgi.admits(&FacesFlags { animated: true, ..Default::default() }));
        assert!(!cgi.admits(&FacesFlags::default()));
        // People cost nothing extra but must have been asked for.
        assert!(!cgi.admits(&FacesFlags { animated: true, bodies: true, ..Default::default() }));
        let people = kind(Footage::Live, Depth::Standard, true);
        assert!(people.admits(&FacesFlags { known: true, ..Default::default() }));
    }

    #[test]
    fn switches_name_every_part() {
        let k = ScanKind { footage: Footage::Live, depth: Depth::Standard, people: true, scenes: true };
        assert_eq!(k.switches(), [Switch::Live, Switch::People, Switch::Scenes]);
        for s in Switch::ALL {
            assert_eq!(Switch::from_key(s.key()), Some(s));
        }
    }
}
