//! What Rust says to the person, as a key the UI can say in its language.
//!
//! A [`Msg`] carries a key in the UI's `backend` catalog, the values of its
//! `{{variables}}`, and the English filled in. The English is also what logs
//! and plugin pages read, and what the UI shows for a key it doesn't know.
//! `cargo xtask i18n` collects every [`msg!`] into `en/backend.json`, so the
//! key and its English are written once, side by side.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Words for the person, in any language the UI speaks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Msg {
    /// Key in the UI's `backend` catalog; empty for words AutoScene didn't write (a plugin's).
    pub key: String,
    #[serde(default)]
    pub args: BTreeMap<String, MsgArg>,
    /// The English, filled in: logs, plugins, and a key the UI doesn't know.
    pub text: String,
}

/// A value of a [`Msg`]. A nested one is said first: "Can't write to {{path}}: {{why}}".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum MsgArg {
    /// Exact up to 2^53, as in the UI's numbers: pass a larger one as text.
    Num(f64),
    Text(String),
    Msg(Box<Msg>),
}

/// How many a plural [`msg!`] counts. Only a number converts to it: a count
/// in text would pick no plural form in the UI, and no error would say so.
///
/// ```compile_fail,E0277
/// as_core::msg!("a.b", one = "{{count}} clip", other = "{{count}} clips", count = "3");
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Count(f64);

/// An error the person can read: [`Msg::text`] is the English of its `Display`.
pub trait ToMsg {
    fn msg(&self) -> Msg;
}

impl Msg {
    /// Words AutoScene didn't write, shown as they are in every language.
    pub fn plain(text: impl Into<String>) -> Self {
        Self { key: String::new(), args: BTreeMap::new(), text: text.into() }
    }

    /// What [`msg!`] expands to.
    #[doc(hidden)]
    pub fn filled<const N: usize>(key: &str, english: &str, args: [(&str, MsgArg); N]) -> Self {
        let args: BTreeMap<_, _> = args.into_iter().map(|(name, value)| (name.to_owned(), value)).collect();
        Self { key: key.to_owned(), text: fill(english, &args), args }
    }

    /// What the plural form of [`msg!`] expands to.
    #[doc(hidden)]
    pub fn counted<const N: usize>(
        key: &str,
        one: &str,
        other: &str,
        Count(count): Count,
        args: [(&str, MsgArg); N],
    ) -> Self {
        let english = if count == 1.0 { one } else { other };
        let mut said = Self::filled(key, english, args);
        said.args.insert("count".to_owned(), MsgArg::Num(count));
        said.text = fill(english, &said.args);
        said
    }
}

/// `english` with each `{{name}}` filled in, read once from left to right: a
/// value that looks like a variable (a plugin's name) stays as it is. A
/// variable with no value stays too, which `cargo xtask i18n` refuses.
fn fill(english: &str, args: &BTreeMap<String, MsgArg>) -> String {
    let mut text = String::with_capacity(english.len());
    let mut rest = english;
    while let Some((before, open)) = rest.split_once("{{") {
        let Some((name, after)) = open.split_once("}}") else { break };
        text.push_str(before);
        match args.get(name.split(',').next().unwrap_or_default().trim()) {
            Some(value) => text.push_str(&value.to_string()),
            None => text.push_str(&rest[before.len()..rest.len() - after.len()]),
        }
        rest = after;
    }
    text.push_str(rest);
    text
}

impl fmt::Display for MsgArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(n) => write!(f, "{n}"),
            Self::Text(text) => f.write_str(text),
            Self::Msg(msg) => f.write_str(&msg.text),
        }
    }
}

impl From<&str> for MsgArg {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<&String> for MsgArg {
    fn from(text: &String) -> Self {
        Self::Text(text.clone())
    }
}

impl From<String> for MsgArg {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Msg> for MsgArg {
    fn from(msg: Msg) -> Self {
        Self::Msg(Box::new(msg))
    }
}

macro_rules! number_args {
    ($($number:ty),*) => {$(
        impl From<$number> for MsgArg {
            fn from(n: $number) -> Self {
                Self::Num(n as f64)
            }
        }

        impl From<$number> for Count {
            fn from(n: $number) -> Self {
                Self(n as f64)
            }
        }
    )*};
}
// No `f32`: widened, 0.1 would read 0.10000000149011612.
number_args!(u8, u16, u32, u64, usize, i32, i64, f64);

/// A [`Msg`]: the key and its English side by side, both literals, since
/// `cargo xtask i18n` reads them from the source.
///
/// ```ignore
/// use as_core::msg;
///
/// let said = msg!("project.alreadyIn", "That file is already in the project, as {{name}}.", name = "Pilot");
/// assert_eq!(said.text, "That file is already in the project, as Pilot.");
///
/// // One English form per English plural category; `count` picks it.
/// let said = msg!("job.exporting", one = "Exporting {{count}} clip", other = "Exporting {{count}} clips", count = 3);
/// assert_eq!(said.text, "Exporting 3 clips");
/// ```
#[macro_export]
macro_rules! msg {
    ($key:literal, one = $one:literal, other = $other:literal, count = $count:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::Msg::counted(
            $key,
            $one,
            $other,
            $crate::Count::from($count),
            [$((stringify!($name), $crate::MsgArg::from($value))),*],
        )
    };
    ($key:literal, $english:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::Msg::filled($key, $english, [$((stringify!($name), $crate::MsgArg::from($value))),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_fills_the_english_with_its_values() {
        let said = msg!("a.b", "{{name}} has {{n}} of {{total, number}}.", name = "Poster", n = 3_u32, total = 2.5);
        assert_eq!(said.key, "a.b");
        assert_eq!(said.text, "Poster has 3 of 2.5.");
        assert_eq!(said.args["name"], MsgArg::Text("Poster".into()));
        assert_eq!(said.args["n"], MsgArg::Num(3.0));
        assert_eq!(msg!("a.c", "No values.").args.len(), 0);
    }

    #[test]
    fn a_value_is_read_once_and_a_lone_brace_stays() {
        let said = msg!("a.b", "{{name}} says {{what}}", name = "{{what}}", what = "{{name}}");
        assert_eq!(said.text, "{{what}} says {{name}}");
        assert_eq!(msg!("a.c", "{{gone}} and {{").text, "{{gone}} and {{");
    }

    #[test]
    fn the_plural_form_picks_one_for_1_and_other_for_the_rest() {
        let said = |n: usize| {
            msg!("a.b", one = "{{count}} clip of {{of}}", other = "{{count}} clips of {{of}}", count = n, of = "Pilot")
        };
        assert_eq!(said(1).text, "1 clip of Pilot");
        assert_eq!(said(0).text, "0 clips of Pilot");
        assert_eq!(said(2).text, "2 clips of Pilot");
        assert_eq!(said(2).args["count"], MsgArg::Num(2.0));
        assert_eq!(msg!("a.c", one = "{{count}} hour", other = "{{count}} hours", count = 1.5).text, "1.5 hours");
    }

    #[test]
    fn a_nested_msg_survives_json() {
        let why = msg!("file.diskFull", "The disk is full.");
        let said = msg!("a.b", "Can't write to {{path}}: {{why}}", path = "D:/out", why = why, n = 12);
        assert_eq!(said.text, "Can't write to D:/out: The disk is full.");
        let json = serde_json::to_string(&said).unwrap();
        assert_eq!(serde_json::from_str::<Msg>(&json).unwrap(), said);
    }

    #[test]
    fn plain_words_have_no_key_and_may_come_without_args() {
        let said = Msg::plain("The webhook said no.");
        assert_eq!(said.key, "");
        let json = serde_json::json!({ "key": "", "text": "The webhook said no." });
        assert_eq!(serde_json::from_value::<Msg>(json).unwrap(), said);
    }
}
