//! Which scan a new project starts with (design-i52 §4.5).
//!
//! On Automatic it follows the owner's rule of 2026-11-03: Fast Scan where
//! the graphics the analysis runs on aren't built for AI work (a separate
//! card without matrix units, built-in graphics, every Mac, the processor),
//! the full scan where they are or where AutoScene can't tell. Two kinds of
//! footage keep the full scan on Automatic, because Fast Scan would cost
//! them most: 3D animation, and live action while people without a visible
//! face are found. On and Off apply to every footage.
//!
//! Where Automatic gives Fast Scan, the first-start notice says so until the
//! person ticked "Don't show this again" or chose a default ([`ScanDefaults::notice`]).

use as_api::{ContentKind, GraphicsClass, ScanAdvice, ScanChoice, ScanDefaults, ThisComputer};
use as_infer::computer::{Class, Computer};

/// Every footage a new project can be, in the order [`defaults`] lists them.
const FOOTAGE: [ContentKind; 4] = [ContentKind::Live, ContentKind::Mixed, ContentKind::Animation, ContentKind::Anime];

/// What new projects of each footage start with on `computer`, when the
/// person chose `choice` and finds people without a visible face or not;
/// and whether the first-start notice is still due (`notice_done`: the
/// person ticked "Don't show this again" or met the setting).
pub fn defaults(computer: &Computer, choice: ScanChoice, find_bodies: bool, notice_done: bool) -> ScanDefaults {
    let footage = FOOTAGE
        .into_iter()
        .map(|content| ScanAdvice { content, fast: fast(computer.class, choice, find_bodies, content) })
        .collect();
    ScanDefaults {
        computer: ThisComputer { class: class(computer.class), name: computer.name.clone() },
        choice,
        footage,
        // Anime starts with Fast Scan on Automatic wherever the class gets it,
        // so the notice has something to say even while faceless people are found.
        notice: choice == ScanChoice::Auto && computer.class.fast_by_default() && !notice_done,
    }
}

/// Whether a new project of `content` starts with Fast Scan.
pub fn advice(defaults: &ScanDefaults, content: ContentKind) -> bool {
    defaults.footage.iter().find(|a| a.content == content).is_some_and(|a| a.fast)
}

fn fast(class: Class, choice: ScanChoice, find_bodies: bool, content: ContentKind) -> bool {
    match choice {
        ScanChoice::Fast => true,
        ScanChoice::Full => false,
        ScanChoice::Auto => {
            class.fast_by_default()
                && match content {
                    // Fast Scan puts 0.586 of a 3D film's main cast on their
                    // character, the full scan 0.937 (bench/identity).
                    ContentKind::Animation => false,
                    // Fast Scan finds no one without a visible face.
                    ContentKind::Live | ContentKind::Mixed => !find_bodies,
                    // Anime keeps its own character models in Fast Scan, and
                    // no scan follows its characters by their bodies.
                    ContentKind::Anime => true,
                }
        }
    }
}

fn class(class: Class) -> GraphicsClass {
    match class {
        Class::ForAi => GraphicsClass::ForAi,
        Class::NotForAi => GraphicsClass::NotForAi,
        Class::Integrated => GraphicsClass::Integrated,
        Class::Processor => GraphicsClass::Processor,
        Class::Unknown => GraphicsClass::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(class: Class) -> Computer {
        Computer { class, name: "Graphics".into(), luid: Some(1) }
    }

    /// Live, mixed, 3D animation, anime: Fast Scan or not.
    fn fast_for(class: Class, choice: ScanChoice, find_bodies: bool) -> [bool; 4] {
        let d = defaults(&on(class), choice, find_bodies, false);
        assert_eq!(d.footage.iter().map(|a| a.content).collect::<Vec<_>>(), FOOTAGE, "every footage, in order");
        FOOTAGE.map(|content| advice(&d, content))
    }

    #[test]
    fn graphics_built_for_ai_work_keep_the_full_scan() {
        for class in [Class::ForAi, Class::Unknown] {
            for bodies in [false, true] {
                assert_eq!(fast_for(class, ScanChoice::Auto, bodies), [false; 4], "{class:?}, bodies {bodies}");
            }
        }
        let d = defaults(&on(Class::ForAi), ScanChoice::Auto, false, false);
        assert_eq!(d.computer, ThisComputer { class: GraphicsClass::ForAi, name: "Graphics".into() });
        assert_eq!(d.choice, ScanChoice::Auto);
    }

    #[test]
    fn other_graphics_and_the_processor_start_live_action_mixed_and_anime_with_fast_scan() {
        for (class, named) in [
            (Class::NotForAi, GraphicsClass::NotForAi),
            (Class::Integrated, GraphicsClass::Integrated),
            (Class::Processor, GraphicsClass::Processor),
        ] {
            assert_eq!(fast_for(class, ScanChoice::Auto, false), [true, true, false, true], "{class:?}");
            assert_eq!(defaults(&on(class), ScanChoice::Auto, false, false).computer.class, named);
        }
    }

    #[test]
    fn animation_keeps_the_full_scan_on_automatic() {
        for class in [Class::ForAi, Class::NotForAi, Class::Integrated, Class::Processor, Class::Unknown] {
            for bodies in [false, true] {
                let d = defaults(&on(class), ScanChoice::Auto, bodies, false);
                assert!(!advice(&d, ContentKind::Animation), "{class:?}, bodies {bodies}");
            }
        }
    }

    #[test]
    fn finding_faceless_people_keeps_live_action_on_the_full_scan() {
        for class in [Class::NotForAi, Class::Integrated, Class::Processor] {
            // Anime finds no bodies in any scan, so it keeps Fast Scan.
            assert_eq!(fast_for(class, ScanChoice::Auto, true), [false, false, false, true], "{class:?}");
        }
    }

    #[test]
    fn on_and_off_win_for_every_footage() {
        for class in [Class::ForAi, Class::NotForAi, Class::Integrated, Class::Processor, Class::Unknown] {
            for bodies in [false, true] {
                // On: 3D animation too, and live action while faceless people are found.
                assert_eq!(fast_for(class, ScanChoice::Fast, bodies), [true; 4], "{class:?}, bodies {bodies}");
                assert_eq!(fast_for(class, ScanChoice::Full, bodies), [false; 4], "{class:?}, bodies {bodies}");
            }
        }
        assert_eq!(defaults(&on(Class::Integrated), ScanChoice::Full, false, false).choice, ScanChoice::Full);
    }
}
