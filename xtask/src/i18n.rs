//! `cargo xtask i18n`: writes `app/src/locales/en/backend.json`, the English
//! the UI says for a `Msg`, from every `msg!` in the app's Rust.
//!
//! The key and its English are written once, side by side, in the code that
//! says them; the JSON is generated and never edited. The file is read as
//! tokens, so a comment or a string that mentions `msg!` doesn't count, and
//! neither does a test's: whatever `#[cfg(test)]` holds, the files of its
//! modules, and test folders.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use proc_macro2::{Delimiter, Group, Spacing, TokenStream, TokenTree};

const CATALOG: &str = "app/src/locales/en/backend.json";

/// Where `msg!`s live, and the folders in them that hold no app code.
const SOURCES: [&str; 2] = ["crates", "app/src-tauri/src"];
const NOT_THE_APP: [&str; 4] = ["tests", "benches", "examples", "target"];

/// A `msg!`'s English: `"…"`, or `one = "…", other = "…"`.
#[derive(Clone, Debug, PartialEq)]
enum English {
    Text(String),
    Plural { one: String, other: String },
}

impl English {
    fn forms(&self) -> Vec<&str> {
        match self {
            Self::Text(text) => vec![text],
            Self::Plural { one, other } => vec![one, other],
        }
    }
}

/// Every key, with its English and the file it was first seen in.
#[derive(Default)]
struct Catalog(BTreeMap<String, (English, String)>);

pub fn generate(root: &Path) -> Result<()> {
    std::fs::write(root.join(CATALOG), catalog_of(root)?.json()?).with_context(|| format!("writing {CATALOG}"))
}

fn catalog_of(root: &Path) -> Result<Catalog> {
    let mut sources = vec![];
    for source in SOURCES {
        for path in rust_files(&root.join(source))? {
            let file = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            let tests = test_modules(&path, &text).with_context(|| format!("in {file}"))?;
            sources.push((path, file, text, tests));
        }
    }
    let tests: BTreeSet<&PathBuf> = sources.iter().flat_map(|(.., tests)| tests).collect();
    let mut catalog = Catalog::default();
    for (path, file, text, _) in &sources {
        if !tests.contains(path) {
            catalog.read(text, file).with_context(|| format!("in {file}"))?;
        }
    }
    Ok(catalog)
}

/// The files of the modules that only tests have, declared in the file at
/// `path`: `#[cfg(test)] mod bench;`. Its top level is enough to read, as a
/// file this misses only adds a test's words.
fn test_modules(path: &Path, source: &str) -> Result<Vec<PathBuf>> {
    let tokens: Vec<TokenTree> = parsed(source)?.into_iter().collect();
    let root = path.file_stem().is_some_and(|stem| ["lib", "main", "mod"].iter().any(|name| stem == *name));
    let folder = if root { path.parent().unwrap_or(path).to_owned() } else { path.with_extension("") };
    let mut files = vec![];
    for i in 0..tokens.len() {
        if is_cfg_test(&tokens[i..]) {
            let item = &tokens[i..i + item_length(&tokens[i..])];
            if let [.., TokenTree::Ident(keyword), TokenTree::Ident(name), TokenTree::Punct(end)] = item
                && keyword == "mod"
                && end.as_char() == ';'
            {
                files.push(folder.join(format!("{name}.rs")));
            }
        }
    }
    Ok(files)
}

fn parsed(source: &str) -> Result<TokenStream> {
    TokenStream::from_str(source).map_err(|e| anyhow::anyhow!("not Rust: {e}"))
}

/// The `.rs` files under `dir`, in a stable order. `tests.rs` is a test
/// module kept in its own file.
fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let (path, name) = (e.path(), e.file_name());
        if path.is_dir() {
            if !NOT_THE_APP.iter().any(|skip| name == *skip) {
                files.extend(rust_files(&path)?);
            }
        } else if path.extension().is_some_and(|x| x == "rs") && name != "tests.rs" {
            files.push(path);
        }
    }
    Ok(files)
}

impl Catalog {
    /// Add the `msg!`s of one file's source.
    fn read(&mut self, source: &str, file: &str) -> Result<()> {
        self.walk(parsed(source)?, file)
    }

    fn walk(&mut self, tokens: TokenStream, file: &str) -> Result<()> {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let mut i = 0;
        while i < tokens.len() {
            if is_cfg_test(&tokens[i..]) {
                i += item_length(&tokens[i..]);
                continue;
            }
            if let [TokenTree::Ident(name), TokenTree::Punct(bang), TokenTree::Group(args), ..] = &tokens[i..]
                && name == "msg"
                && bang.as_char() == '!'
            {
                let (key, english) = said(args.stream()).with_context(|| format!("msg!({})", args.stream()))?;
                self.add(key, english, file)?;
                i += 3;
                continue;
            }
            if let TokenTree::Group(group) = &tokens[i] {
                self.walk(group.stream(), file)?;
            }
            i += 1;
        }
        Ok(())
    }

    fn add(&mut self, key: String, english: English, file: &str) -> Result<()> {
        if !key.split('.').all(is_segment) {
            bail!("the key '{key}' isn't lowerCamelCase segments joined by dots");
        }
        if let Some((known, first)) = self.0.get(&key) {
            if *known != english {
                bail!("the key '{key}' has two English texts: {known:?} in {first} and {english:?} here");
            }
            return Ok(());
        }
        self.0.insert(key, (english, file.to_owned()));
        Ok(())
    }

    /// `en/backend.json`: nested as the keys are, sorted, plural forms as
    /// `_one` and `_other`, so the same source gives the same bytes.
    fn json(&self) -> Result<String> {
        let mut root = serde_json::Map::new();
        for (key, (english, _)) in &self.0 {
            match english {
                English::Text(text) => put(&mut root, key, text)?,
                English::Plural { one, other } => {
                    put(&mut root, &format!("{key}_one"), one)?;
                    put(&mut root, &format!("{key}_other"), other)?;
                }
            }
        }
        Ok(serde_json::to_string_pretty(&root)? + "\n")
    }
}

fn is_segment(segment: &str) -> bool {
    segment.starts_with(|c: char| c.is_ascii_lowercase()) && segment.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Whether `tokens` starts with a `#[cfg(…)]` that holds only in tests.
fn is_cfg_test(tokens: &[TokenTree]) -> bool {
    let [TokenTree::Punct(hash), TokenTree::Group(attribute), ..] = tokens else { return false };
    if hash.as_char() != '#' || attribute.delimiter() != Delimiter::Bracket {
        return false;
    }
    let attribute: Vec<TokenTree> = attribute.stream().into_iter().collect();
    let [TokenTree::Ident(cfg), TokenTree::Group(condition)] = attribute.as_slice() else { return false };
    cfg == "cfg" && only_in_tests(&condition.stream().into_iter().collect::<Vec<_>>())
}

/// Whether a `cfg` condition holds in tests and nowhere else: `test`, `all`
/// with such a condition, `any` of only those. `not(test)` is the app's.
fn only_in_tests(condition: &[TokenTree]) -> bool {
    let each = |list: &Group| -> Vec<Vec<TokenTree>> {
        split_at_commas(list.stream()).into_iter().filter(|part| !part.is_empty()).collect()
    };
    match condition {
        [TokenTree::Ident(name)] => name == "test",
        [TokenTree::Ident(name), TokenTree::Group(list)] if name == "all" => {
            each(list).iter().any(|part| only_in_tests(part))
        }
        [TokenTree::Ident(name), TokenTree::Group(list)] if name == "any" => {
            let parts = each(list);
            !parts.is_empty() && parts.iter().all(|part| only_in_tests(part))
        }
        _ => false,
    }
}

/// How many tokens what follows the `cfg` at the start of `tokens` takes: up
/// to its `;`, its `{ … }`, or the `,` after a field or a match arm. A comma
/// in `fn a<T, U>` ends it too soon, which can only keep a test's words: the
/// other way, an app's would be gone with nobody told.
fn item_length(tokens: &[TokenTree]) -> usize {
    let ends = |t: &TokenTree| match t {
        TokenTree::Punct(p) => matches!(p.as_char(), ';' | ','),
        TokenTree::Group(g) => g.delimiter() == Delimiter::Brace,
        _ => false,
    };
    tokens.iter().position(ends).map_or(tokens.len(), |end| end + 1)
}

/// The key and the English of a `msg!`'s arguments, whose `{{variables}}`
/// each need a value: the UI would show one without as it is written.
fn said(args: TokenStream) -> Result<(String, English)> {
    let mut parts = split_at_commas(args).into_iter();
    let key = text(&parts.next().unwrap_or_default())?;
    let first = parts.next().unwrap_or_default();
    let english = match named(&first) {
        Some((name, one)) if name == "one" => {
            let second = parts.next().unwrap_or_default();
            let Some((_, other)) = named(&second).filter(|(name, _)| name == "other") else {
                bail!("`one` comes with `other`");
            };
            English::Plural { one: text(one)?, other: text(other)? }
        }
        _ => English::Text(text(&first)?),
    };
    let given: BTreeSet<String> = parts.filter_map(|part| Some(named(&part)?.0)).collect();
    for form in english.forms() {
        for variable in variables(form) {
            if !given.contains(variable) {
                bail!("{{{{{variable}}}}} of '{key}' has no value");
            }
        }
    }
    Ok((key, english))
}

fn split_at_commas(tokens: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut parts = vec![vec![]];
    for token in tokens {
        match &token {
            TokenTree::Punct(p) if p.as_char() == ',' => parts.push(vec![]),
            _ => parts.last_mut().expect("starts with one").push(token),
        }
    }
    parts
}

/// `name = …`, as its name and value.
fn named(part: &[TokenTree]) -> Option<(String, &[TokenTree])> {
    let [TokenTree::Ident(name), TokenTree::Punct(equals), value @ ..] = part else { return None };
    (equals.as_char() == '=' && equals.spacing() == Spacing::Alone).then(|| (name.to_string(), value))
}

/// The string a lone literal says.
fn text(tokens: &[TokenTree]) -> Result<String> {
    let [TokenTree::Literal(literal)] = tokens else { bail!("the key and the English are literals") };
    Ok(syn::parse_str::<syn::LitStr>(&literal.to_string()).context("the key and the English are strings")?.value())
}

/// The names in `{{name}}` and `{{name, format}}`.
fn variables(english: &str) -> Vec<&str> {
    let after = english.split("{{").skip(1);
    after.filter_map(|s| s.split_once("}}")).map(|(v, _)| v.split(',').next().unwrap_or(v).trim()).collect()
}

/// Put `text` at the dotted `key`. A key can't be text and hold others.
fn put(root: &mut serde_json::Map<String, serde_json::Value>, key: &str, text: &str) -> Result<()> {
    let clash = || anyhow::anyhow!("'{key}' is both a text and a group of keys");
    let mut segments: Vec<&str> = key.split('.').collect();
    let leaf = segments.pop().expect("split gives one");
    let mut at = root;
    for segment in segments {
        let next = at.entry(segment).or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        at = next.as_object_mut().ok_or_else(clash)?;
    }
    match at.insert(leaf.to_owned(), text.into()) {
        Some(_) => Err(clash()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(sources: &[&str]) -> Result<String> {
        let mut catalog = Catalog::default();
        for (i, source) in sources.iter().enumerate() {
            catalog.read(source, &format!("{i}.rs"))?;
        }
        catalog.json()
    }

    fn value(sources: &[&str]) -> serde_json::Value {
        serde_json::from_str(&json(sources).unwrap()).unwrap()
    }

    #[test]
    fn keys_nest_and_the_english_is_the_literals() {
        let said = value(&[r#"fn a() {
            Err(AppError::new(Code, msg!("file.gone", "It\u{2019}s “gone”\n",)));
            as_core::msg!("internal", "Something went wrong.");
        }"#]);
        assert_eq!(
            said,
            serde_json::json!({ "file": { "gone": "It’s “gone”\n" }, "internal": "Something went wrong." })
        );
    }

    #[test]
    fn two_english_texts_for_one_key_fail() {
        let a = r#"fn a() { msg!("a.b", "One."); }"#;
        let b = r#"fn b() { msg!("a.b", "Another."); }"#;
        let error = format!("{:#}", json(&[a, b]).unwrap_err());
        assert!(error.contains("two English texts") && error.contains("0.rs"), "{error}");
        // The same words in two places are one key.
        assert!(json(&[a, a]).is_ok());
        let plural = r#"fn c() { msg!("a.b", one = "One.", other = "Others.", count = n); }"#;
        assert!(json(&[a, plural]).is_err(), "a text and a plural");
    }

    #[test]
    fn the_plural_form_writes_one_and_other() {
        let said = value(&[r#"fn a() {
            msg!("job.exporting", one = "Exporting {{count}} clip", other = "Exporting {{count}} clips", count = n);
        }"#]);
        assert_eq!(
            said,
            serde_json::json!({ "job": {
                "exporting_one": "Exporting {{count}} clip",
                "exporting_other": "Exporting {{count}} clips",
            }})
        );
    }

    #[test]
    fn generation_is_stable() {
        let a = r#"fn a() { msg!("z.b", "Z."); msg!("a.b", "A."); }"#;
        let b = r#"fn b() { msg!("m", "M."); msg!("a.a", "B."); }"#;
        let (one, other) = (json(&[a, b]).unwrap(), json(&[b, a]).unwrap());
        assert_eq!(one, other);
        assert_eq!(
            one,
            "{\n  \"a\": {\n    \"a\": \"B.\",\n    \"b\": \"A.\"\n  },\n  \"m\": \"M.\",\n  \"z\": {\n    \"b\": \"Z.\"\n  }\n}\n"
        );
    }

    #[test]
    fn it_finds_a_msg_inside_other_macros_and_only_real_ones() {
        let said = value(&[r##"
            //! msg!("doc.module", "In a comment.");
            /// msg!("doc.item", "In a comment.");
            macro_rules! msg { ($k:literal) => {} }
            fn a(msg: bool) -> bool {
                // msg!("comment.line", "In a comment.");
                /* msg!("comment.block", "In a comment."); */
                let _ = r#"msg!("string.raw", "In a string.");"#;
                log::warn!("{}", vec![msg!("in.macro", "Inside two macros.")][0].text);
                msg != (true)
            }
        "##]);
        assert_eq!(said, serde_json::json!({ "in": { "macro": "Inside two macros." } }));
    }

    #[test]
    fn a_tests_words_stay_out() {
        let said = value(&[r#"
            fn a() { msg!("app.words", "The app's."); }
            #[cfg(test)]
            mod tests {
                #[test]
                fn b() { msg!("test.words", "A test's."); }
            }
            #[cfg(test)]
            use something::msg;
            fn c() { msg!("app.more", "Still the app's."); }
        "#]);
        assert_eq!(said, serde_json::json!({ "app": { "more": "Still the app's.", "words": "The app's." } }));
    }

    #[test]
    fn a_cfg_on_a_field_or_an_arm_takes_only_that_field_or_arm() {
        let said = value(&[r#"
            fn a() -> E {
                E { #[cfg(test)] seen: 1, why: msg!("real.field", "A field's.") }
            }
            fn b(x: X) -> Msg {
                match x {
                    #[cfg(test)]
                    X::T => 1,
                    X::A => msg!("real.arm", "An arm's."),
                    #[cfg(test)]
                    X::B => { msg!("test.arm", "A test's.") }
                    X::C => msg!("real.other", "Another arm's."),
                }
            }
        "#]);
        assert_eq!(
            said,
            serde_json::json!({ "real": { "arm": "An arm's.", "field": "A field's.", "other": "Another arm's." } })
        );
    }

    #[test]
    fn a_cfg_counts_as_a_tests_only_if_it_holds_nowhere_else() {
        let words = |cfg: &str| value(&[&format!(r#"#[cfg({cfg})] fn a() {{ msg!("a.b", "Words."); }}"#)]);
        let kept = serde_json::json!({ "a": { "b": "Words." } });
        for cfg in ["test", "all(test, windows)", "all(windows, any(test,))", "any(test, all(test, unix))"] {
            assert_eq!(words(cfg), serde_json::json!({}), "{cfg}");
        }
        for cfg in ["not(test)", "any(test, windows)", "windows", "any()", r#"feature = "test""#] {
            assert_eq!(words(cfg), kept, "{cfg}");
        }
        assert_eq!(value(&[r#"#[other(test)] fn a() { msg!("a.b", "Words."); }"#]), kept, "another attribute");
    }

    #[test]
    fn the_files_of_a_tests_modules_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str, source: &str| {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source).unwrap();
        };
        let words = |key: &str| format!(r#"fn a() {{ msg!("{key}", "Words."); }}"#);
        write("app/src-tauri/src/main.rs", "#[cfg(test)] mod bench; mod pack;");
        write("app/src-tauri/src/bench.rs", &words("test.lib"));
        write("app/src-tauri/src/pack.rs", &format!("#[cfg(test)]\npub mod bench;\nmod real;\n{}", words("app.pack")));
        write("app/src-tauri/src/pack/bench.rs", &words("test.nested"));
        write("app/src-tauri/src/pack/real.rs", &words("app.real"));
        // A module's file sits under the file that declares it: this `bench` isn't `other`'s.
        write("crates/a/src/other.rs", "#[cfg(test)] mod bench;");
        write("crates/a/src/bench.rs", &words("app.sibling"));

        let said: serde_json::Value = serde_json::from_str(&catalog_of(dir.path()).unwrap().json().unwrap()).unwrap();
        let app = serde_json::json!({ "pack": "Words.", "real": "Words.", "sibling": "Words." });
        assert_eq!(said, serde_json::json!({ "app": app }));
    }

    #[test]
    fn a_variable_with_no_value_fails() {
        let error = |source| format!("{:#}", json(&[source]).unwrap_err());
        assert!(error(r#"fn a() { msg!("a.b", "{{name}} waits."); }"#).contains("{{name}} of 'a.b' has no value"));
        assert!(error(r#"fn a() { msg!("a.b", "{{name}}", nam = x); }"#).contains("no value"));
        let plural = r#"fn a() { msg!("a.b", one = "{{count}} of {{all}}", other = "{{count}}", count = n); }"#;
        assert!(error(plural).contains("{{all}} of 'a.b' has no value"));
        assert!(json(&[r#"fn a() { msg!("a.b", "{{n, number}} {{x}}", n = call(1, 2), x = y == z); }"#]).is_ok());
        assert!(json(&[r#"fn a() { msg!("a.b", one = "1", other = "{{count}}", count = n); }"#]).is_ok());
    }

    #[test]
    fn a_key_must_be_able_to_live_in_the_catalog() {
        for key in ["a_b", "A.b", "a..b", "", "a.b-c"] {
            let source = format!(r#"fn a() {{ msg!("{key}", "Words."); }}"#);
            assert!(json(&[&source]).is_err(), "{key}");
        }
        let error = |sources: &[&str]| format!("{:#}", json(sources).unwrap_err());
        let both = r#"fn a() { msg!("a", "Text."); msg!("a.b", "In a group."); }"#;
        assert!(error(&[both]).contains("both a text and a group"));
    }

    #[test]
    fn test_folders_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str| {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        };
        for path in ["a/src/b.rs", "a/src/tests.rs", "a/tests/c.rs", "a/examples/d.rs", "a/benches/e.rs", "a/f.toml"] {
            write(path);
        }
        let files = rust_files(dir.path()).unwrap();
        assert_eq!(files, [dir.path().join("a/src/b.rs")]);
    }

    /// `cargo test` says so before CI's diff does.
    #[test]
    fn the_catalog_in_the_tree_is_up_to_date() {
        let root = crate::workspace_root();
        let written = std::fs::read_to_string(root.join(CATALOG)).unwrap();
        assert_eq!(written, catalog_of(&root).unwrap().json().unwrap(), "run `cargo xtask i18n`");
    }
}
