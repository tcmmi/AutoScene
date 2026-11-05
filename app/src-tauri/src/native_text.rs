//! The words Rust draws itself: the macOS menu, the system's dialogs and the
//! names written into exported files. Everything else Rust says reaches the
//! UI as a [`as_core::Msg`], and the UI says it.
//!
//! The words live in `app/src/locales/<language>/native.json`, read here at
//! build time, nowhere else: a test below refuses a dialog's or a menu's
//! words in the code. What a plugin or a person wrote goes in as a value, and
//! a value is never read for `{{variables}}` itself: a plugin called
//! "{{url}}" can't put another link in a question.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use as_api::Lang;
use as_project::naming::FileWords;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// The catalog of each language AutoScene speaks, English first.
const CATALOGS: &[(Lang, &str)] = &[
    (Lang::En, include_str!("../../src/locales/en/native.json")),
    (Lang::Ro, include_str!("../../src/locales/ro/native.json")),
];

/// A language's words, by key (`dialog.hung.title`). A key its catalog lacks
/// has the English.
#[derive(Debug)]
pub struct Words(BTreeMap<String, String>);

impl Words {
    /// The words of a `native.json`, over `english`'s.
    fn of(json: &str, english: Option<&Words>) -> serde_json::Result<Self> {
        let mut words = english.map_or_else(BTreeMap::new, |e| e.0.clone());
        flatten(&serde_json::from_str(json)?, "", &mut words);
        Ok(Self(words))
    }

    /// The words of `key`, with `vars` for its `{{variables}}`.
    pub fn say(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let Some(text) = self.0.get(key) else {
            log::error!("no native text for {key}");
            return key.to_owned();
        };
        fill(text, vars)
    }
}

impl FileWords for Words {
    fn unnamed_character(&self, number: u32) -> String {
        self.say("file.unnamedCharacter", &[("n", &number.to_string())])
    }

    fn pack_label(&self, character: &str) -> String {
        self.say("file.packLabel", &[("character", character)])
    }
}

/// `value`'s strings into `words`, each under its dotted key.
fn flatten(value: &serde_json::Value, prefix: &str, words: &mut BTreeMap<String, String>) {
    match value {
        serde_json::Value::Object(entries) => {
            for (key, value) in entries {
                let key = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(value, &key, words);
            }
        }
        serde_json::Value::String(text) => {
            words.insert(prefix.to_owned(), text.clone());
        }
        other => log::error!("native text {prefix} isn't text: {other}"),
    }
}

/// `text` with `vars` for its `{{variables}}`, in one pass; one `vars` doesn't
/// name stays as it is.
fn fill(text: &str, vars: &[(&str, &str)]) -> String {
    let mut filled = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once("{{") {
        filled.push_str(before);
        let value = after.split_once("}}").and_then(|(name, tail)| {
            let (_, value) = vars.iter().find(|(var, _)| *var == name)?;
            Some((*value, tail))
        });
        match value {
            Some((value, tail)) => {
                filled.push_str(value);
                rest = tail;
            }
            None => {
                filled.push_str("{{");
                rest = after;
            }
        }
    }
    filled.push_str(rest);
    filled
}

/// Every language's words, read once.
fn catalogs() -> &'static [(Lang, Words)] {
    static WORDS: OnceLock<Vec<(Lang, Words)>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let english = Words::of(CATALOGS[0].1, None).expect("en/native.json is held by its tests");
        let others: Vec<_> = CATALOGS[1..]
            .iter()
            .map(|(lang, json)| (*lang, Words::of(json, Some(&english)).expect("native.json is held by its tests")))
            .collect();
        std::iter::once((CATALOGS[0].0, english)).chain(others).collect()
    })
}

/// The words of `lang`.
pub fn words(lang: Lang) -> &'static Words {
    let catalogs = catalogs();
    catalogs.iter().find(|(l, _)| *l == lang).map_or(&catalogs[0].1, |(_, words)| words)
}

/// The words of the language AutoScene speaks now.
pub fn current(app: &AppHandle) -> &'static Words {
    app.state::<AppState>().words()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use syn::punctuated::Punctuated;
    use syn::visit::{self, Visit};
    use syn::{
        Attribute, Block, Expr, ExprCall, ExprClosure, ExprField, ExprLit, ExprMethodCall, FnArg, ImplItemFn, ItemFn,
        ItemMod, Lit, Local, Macro, Member, Pat, ReturnType, Signature, Stmt, Token, Type,
    };

    use super::*;

    /// What the tests of the rest of the app ask in.
    pub(crate) fn english() -> &'static Words {
        words(Lang::En)
    }

    /// `json` over the English, as a language's catalog is: a fixture
    /// Romanian that lives as long as the words of a real one.
    pub(crate) fn fixture(json: &str) -> &'static Words {
        Box::leak(Box::new(Words::of(json, Some(english())).unwrap()))
    }

    /// Every word of the English in `«»`, as a language of its own: what is
    /// left outside them was never looked up.
    pub(crate) fn marked() -> &'static Words {
        let mark = |(key, text): (&String, &String)| (key.clone(), format!("«{text}»"));
        Box::leak(Box::new(Words(english().0.iter().map(mark).collect())))
    }

    /// `text` without what [`marked`] marked.
    pub(crate) fn unmarked(text: &str) -> String {
        let mut left = String::new();
        let mut rest = text;
        while let Some((before, after)) = rest.split_once('«') {
            left.push_str(before);
            rest = after.split_once('»').map_or("", |(_, tail)| tail);
        }
        left + rest
    }

    /// The `{{variables}}` of `text`, sorted.
    fn variables(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> =
            text.split("{{").skip(1).filter_map(|part| part.split_once("}}").map(|(name, _)| name)).collect();
        found.sort_unstable();
        found
    }

    /// Each catalog as it is written, no key filled in from the English.
    fn written() -> Vec<(Lang, Words)> {
        CATALOGS.iter().map(|(lang, json)| (*lang, Words::of(json, None).unwrap())).collect()
    }

    /// Where `catalog` isn't `english`'s: a key one of them lacks, other
    /// `{{variables}}`, a text with nothing in it.
    fn same_shape(catalog: &Words, english: &Words) -> Vec<String> {
        let missing = english.0.keys().filter(|key| !catalog.0.contains_key(*key)).map(|key| format!("{key}: missing"));
        let others = catalog.0.iter().filter_map(|(key, text)| match english.0.get(key) {
            None => Some(format!("{key}: not in the English")),
            Some(_) if text.trim().is_empty() => Some(format!("{key}: empty")),
            Some(theirs) if variables(text) != variables(theirs) => Some(format!("{key}: other variables")),
            Some(_) => None,
        });
        missing.chain(others).collect()
    }

    #[test]
    fn every_language_has_every_key_and_the_same_variables() {
        for (lang, catalog) in written() {
            assert_eq!(same_shape(&catalog, english()), Vec::<String>::new(), "{lang:?}");
        }
    }

    #[test]
    fn a_catalog_of_another_shape_is_refused() {
        let words = |json: &str| Words::of(json, None).unwrap();
        let theirs = words(r#"{"open": "Open {{url}} for {{name}}?", "close": "Close"}"#);
        let problems = |json: &str| same_shape(&words(json), &theirs);

        assert_eq!(problems(r#"{"open": "Deschide {{name}}: {{url}}?", "close": "Închide"}"#), Vec::<String>::new());
        assert_eq!(problems(r#"{"open": "Deschide {{url}} {{name}}?"}"#), ["close: missing"]);
        assert_eq!(
            problems(r#"{"open": "Deschide {{url}} {{name}}?", "close": "Închide", "more": "Mai mult"}"#),
            ["more: not in the English"]
        );
        assert_eq!(
            problems(r#"{"open": "Deschide {{link}} {{name}}?", "close": "Închide"}"#),
            ["open: other variables"]
        );
        assert_eq!(problems(r#"{"open": "Deschide {{name}}?", "close": "Închide"}"#), ["open: other variables"]);
        assert_eq!(problems(r#"{"open": "Deschide {{url}} {{name}}?", "close": " "}"#), ["close: empty"]);
    }

    #[test]
    fn every_language_autoscene_speaks_has_its_words() {
        for (code, lang) in crate::locale::SPOKEN {
            assert!(CATALOGS.iter().any(|(l, _)| l == lang), "no native.json for {code}");
        }
    }

    /// Whether `text`, in a question about a plugin, opens with words a
    /// plugin can't choose and has its name before anything else of its own:
    /// a plugin may be called anything, as `app/src/locales/check.ts` says of
    /// its toasts.
    fn opens_with_fixed_words(text: &str) -> bool {
        let Some((opening, after)) = text.split_once("{{") else { return true };
        !text.contains("{{name}}") || (opening.chars().any(char::is_alphabetic) && after.starts_with("name}}"))
    }

    #[test]
    fn a_question_about_a_plugin_opens_with_words_it_cant_choose() {
        assert!(!opens_with_fixed_words("{{name}} asks to open"));
        assert!(!opens_with_fixed_words("“{{name}}” asks to open"));
        assert!(!opens_with_fixed_words("Open {{url}} for {{name}}"));
        for (lang, catalog) in written() {
            for (key, text) in catalog.0.iter().filter(|(key, _)| key.starts_with("dialog.")) {
                assert!(opens_with_fixed_words(text), "{lang:?} {key}: {text}");
            }
        }
    }

    #[test]
    fn variables_are_filled_in_once() {
        let said = fill("{{a}} and {{b}}", &[("a", "{{b}}"), ("b", "{{a}}")]);
        assert_eq!(said, "{{b}} and {{a}}");
        assert_eq!(fill("{{unknown}} {{a}} {{", &[("a", "1")]), "{{unknown}} 1 {{");
        assert_eq!(fill("no variables", &[("a", "1")]), "no variables");

        let asked = english().say("dialog.openLink.page", &[("name", "{{url}}"), ("url", "https://a.example/")]);
        assert_eq!(asked, "Plugin “{{url}}” asks to open:\n\nhttps://a.example/");
    }

    #[test]
    fn a_key_its_language_lacks_has_the_english() {
        let romanian = fixture(r#"{"dialog": {"cancel": "Anulează"}}"#);
        assert_eq!(romanian.say("dialog.cancel", &[]), "Anulează");
        assert_eq!(romanian.say("dialog.hung.close", &[]), "Close it");
    }

    #[test]
    fn exported_names_come_from_the_catalog() {
        assert_eq!(english().unnamed_character(12), "Character 12");
        assert_eq!(english().pack_label("Character 12"), "Character 12 · scenepack");

        let romanian =
            fixture(r#"{"file": {"unnamedCharacter": "Personaj {{n}}", "packLabel": "Pachet {{character}}"}}"#);
        assert_eq!(romanian.unnamed_character(12), "Personaj 12");
        assert_eq!(romanian.pack_label("Personaj 12"), "Pachet Personaj 12");
    }

    /// Every `.rs` file of the app but this one, without its tests.
    fn app_sources() -> Vec<(PathBuf, String)> {
        fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, files);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    files.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let read = |path: PathBuf| {
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]\nmod tests").next().unwrap().to_owned();
            (path, code)
        };
        files.into_iter().filter(|p| !p.ends_with("native_text.rs")).map(read).collect()
    }

    /// Who draws words a person reads, and which arguments hold them; a
    /// leading `.` is a method, a trailing `::` any function of that type. A
    /// function named here may pass on what it was given: its calls are held.
    const DRAWN: &[(&str, &[usize])] = &[
        (".title", &[0]),
        (".message", &[0]),
        (".set_title", &[0]),
        ("OkCustom", &[0]),
        ("OkCancelCustom", &[0, 1]),
        ("YesNoCancelCustom", &[0, 1, 2]),
        ("MessageDialogBuilder::new", &[1, 2]),
        ("confirm", &[1, 2, 3]),
        (".confirm", &[0, 1, 2]),
        (".save_to", &[0]),
        (".pick", &[0]),
        (".file_dialog", &[0]),
        // `(sv.ask)`, and `save_dialog`, whose closure it is.
        ("ask", &[0]),
        ("save_dialog", &[]),
        ("MenuItemBuilder::new", &[0]),
        ("MenuItemBuilder::with_id", &[1]),
        ("CheckMenuItemBuilder::new", &[0]),
        ("CheckMenuItemBuilder::with_id", &[1]),
        ("IconMenuItemBuilder::new", &[0]),
        ("IconMenuItemBuilder::with_id", &[1]),
        ("MenuItem::new", &[1]),
        ("MenuItem::with_id", &[2]),
        ("CheckMenuItem::new", &[1]),
        ("CheckMenuItem::with_id", &[2]),
        ("IconMenuItem::new", &[1]),
        ("IconMenuItem::with_id", &[2]),
        ("Submenu::new", &[1]),
        ("Submenu::with_id", &[2]),
        ("SubmenuBuilder::new", &[1]),
        ("SubmenuBuilder::with_id", &[2]),
        ("PredefinedMenuItem::", &[1]),
    ];

    /// `id, words` of a menu builder. Other things are called this too, so
    /// only words found in them count, not what can't be followed.
    const MENU_ENTRIES: &[&str] = &[".text", ".check", ".icon"];

    /// Methods that hand on what their receiver holds.
    const SAME_WORDS: &[&str] = &["into", "to_owned", "to_string", "clone", "as_str", "as_ref", "join", "trim"];

    /// What the arguments of the call `name` must have come from the catalog.
    fn drawn_arguments(name: &str) -> &'static [usize] {
        if name.ends_with("_with_text") {
            return &[0];
        }
        let last = name.rsplit("::").next().unwrap_or(name);
        let is_it = |call: &&(&str, &[usize])| {
            call.0 == name || call.0 == last || (call.0.ends_with("::") && name.starts_with(call.0))
        };
        DRAWN.iter().find(is_it).map_or(&[], |(_, arguments)| arguments)
    }

    /// Whether a function called `name` only passes its arguments on.
    fn passes_words_on(name: &str) -> bool {
        DRAWN.iter().any(|(call, _)| call.trim_start_matches('.') == name)
    }

    /// Where a value came from, the worst last.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Held {
        /// The catalog, or nothing a person reads.
        Looked,
        /// Nothing the guard can follow to the catalog.
        Unknown,
        /// Words in the code: a string, a `format!`, a constant.
        Words,
    }

    /// Whether `args` hand a call the words: it says what it says in them.
    fn gets_the_words<'a>(mut args: impl Iterator<Item = &'a Expr>) -> bool {
        args.any(|arg| match stripped(arg) {
            Expr::Path(path) => path.path.is_ident("words") || path.path.is_ident("w"),
            Expr::Field(field) => matches!(&field.member, Member::Named(name) if name == "words"),
            _ => false,
        })
    }

    /// `value` without the `&`, `*` and parentheses around it.
    fn stripped(value: &Expr) -> &Expr {
        match value {
            Expr::Reference(inner) => stripped(&inner.expr),
            Expr::Unary(inner) => stripped(&inner.expr),
            Expr::Paren(inner) => stripped(&inner.expr),
            Expr::Group(inner) => stripped(&inner.expr),
            other => other,
        }
    }

    /// What `template`, a `format!`'s, says outside its `{}`, and the
    /// variables named inside them.
    fn template_parts(template: &str) -> (String, Vec<String>) {
        let template = template.replace("{{", "").replace("}}", "");
        let (mut outside, mut named, mut rest) = (String::new(), Vec::new(), template.as_str());
        while let Some((before, after)) = rest.split_once('{') {
            outside.push_str(before);
            let (inside, tail) = after.split_once('}').unwrap_or((after, ""));
            let name = inside.split(':').next().unwrap_or("");
            if name.chars().next().is_some_and(|c| !c.is_ascii_digit()) {
                named.push(name.to_owned());
            }
            rest = tail;
        }
        outside.push_str(rest);
        (outside, named)
    }

    /// Whether `ty` is a `Question`, which `install_question` builds.
    fn is_question(ty: &Type) -> bool {
        matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Question"))
    }

    /// Reads a file and notes each drawn argument that didn't come from the
    /// catalog: it follows `let`s, in the function they are in. Past that it
    /// doesn't look: what a `push_str` adds, or what a helper that was handed
    /// the words returns, is for the tests with every word marked.
    #[derive(Default)]
    struct Guard {
        scopes: Vec<HashMap<String, Held>>,
        function: String,
        passes_on: bool,
        found: Vec<String>,
    }

    impl Guard {
        fn named(&self, name: &str) -> Held {
            self.scopes.iter().rev().find_map(|scope| scope.get(name).copied()).unwrap_or(Held::Unknown)
        }

        fn worst<'a>(&self, values: impl Iterator<Item = &'a Expr>) -> Held {
            values.map(|value| self.held(value)).max().unwrap_or(Held::Looked)
        }

        /// The tail of `block`, if it has one.
        fn tail(&self, block: &Block) -> Held {
            match block.stmts.last() {
                Some(Stmt::Expr(tail, None)) => self.held(tail),
                _ => Held::Unknown,
            }
        }

        fn held(&self, value: &Expr) -> Held {
            match stripped(value) {
                Expr::Lit(ExprLit { lit: Lit::Str(text), .. }) if text.value().chars().any(char::is_alphabetic) => {
                    Held::Words
                }
                Expr::Lit(_) => Held::Looked,
                Expr::Path(path) => match path.path.get_ident() {
                    Some(name) if !is_constant(&name.to_string()) => self.named(&name.to_string()),
                    _ if path.path.segments.last().is_some_and(|s| is_constant(&s.ident.to_string())) => Held::Words,
                    _ => Held::Unknown,
                },
                Expr::Macro(call) => self.formatted(&call.mac),
                Expr::MethodCall(call) if call.method == "say" => self.vars(call.args.iter().nth(1)),
                Expr::MethodCall(call) if SAME_WORDS.iter().any(|same| call.method == same) => {
                    self.held(&call.receiver).max(self.worst(call.args.iter()))
                }
                Expr::MethodCall(call) if gets_the_words(call.args.iter()) => Held::Looked,
                Expr::Call(call) if gets_the_words(call.args.iter()) => Held::Looked,
                Expr::Call(call) => match &*call.func {
                    Expr::Path(path) if call.args.is_empty() && path_name(&path.path) == "String::new" => Held::Looked,
                    Expr::Path(path) if ["Some", "String::from"].contains(&path_name(&path.path).as_str()) => {
                        self.worst(call.args.iter())
                    }
                    _ => Held::Unknown,
                },
                Expr::Block(block) => self.tail(&block.block),
                Expr::If(choice) => {
                    let otherwise = choice.else_branch.as_ref().map_or(Held::Looked, |(_, e)| self.held(e));
                    self.tail(&choice.then_branch).max(otherwise)
                }
                Expr::Match(choice) => self.worst(choice.arms.iter().map(|arm| &*arm.body)),
                _ => Held::Unknown,
            }
        }

        /// What a `format!` puts together. Other macros aren't followed.
        fn formatted(&self, call: &Macro) -> Held {
            let parsed = call.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated);
            let is_format = call.path.segments.last().is_some_and(|s| s.ident == "format");
            let (Ok(args), true) = (parsed, is_format) else { return Held::Unknown };
            let mut args = args.iter();
            let Some(Expr::Lit(ExprLit { lit: Lit::Str(template), .. })) = args.next() else { return Held::Unknown };
            let (outside, named) = template_parts(&template.value());
            if outside.chars().any(char::is_alphabetic) {
                return Held::Words;
            }
            let inline = named.iter().map(|name| self.named(name)).max().unwrap_or(Held::Looked);
            inline.max(self.worst(args))
        }

        /// What the `{{variables}}` of a `say` are given: a value may come
        /// from anywhere, but not be words of ours.
        fn vars(&self, vars: Option<&Expr>) -> Held {
            let Some(Expr::Array(list)) = vars.map(stripped) else { return Held::Looked };
            let value = |var: &Expr| match var {
                Expr::Tuple(pair) => pair.elems.last().map_or(Held::Looked, |value| self.held(value)),
                _ => Held::Looked,
            };
            match list.elems.iter().any(|var| value(var) == Held::Words) {
                true => Held::Words,
                false => Held::Looked,
            }
        }

        /// `value` under the names of `pat`, each `otherwise` if it isn't
        /// given one of its own.
        fn bind(&mut self, pat: &Pat, value: Option<&Expr>, otherwise: Held) {
            match pat {
                Pat::Ident(name) => {
                    let held = value.map_or(otherwise, |value| self.held(value));
                    self.scopes.last_mut().unwrap().insert(name.ident.to_string(), held);
                }
                Pat::Type(typed) => self.bind(&typed.pat, value, otherwise),
                Pat::Tuple(names) => {
                    let values = match value {
                        Some(Expr::Tuple(values)) if values.elems.len() == names.elems.len() => Some(&values.elems),
                        _ => None,
                    };
                    for (i, name) in names.elems.iter().enumerate() {
                        self.bind(name, values.map(|values| &values[i]), otherwise);
                    }
                }
                _ => {}
            }
        }

        fn note(&mut self, call: &str, i: usize) {
            self.found.push(format!("{}: {call}, argument {i}", self.function));
        }

        fn check(&mut self, name: &str, args: &Punctuated<Expr, Token![,]>) {
            for (i, arg) in args.iter().enumerate() {
                let held = self.held(arg);
                let drawn = drawn_arguments(name).contains(&i) && held != Held::Looked;
                if drawn || (i == 1 && MENU_ENTRIES.contains(&name) && held == Held::Words) {
                    self.note(name, i);
                }
            }
        }

        fn function(&mut self, sig: &Signature, body: &Block) {
            let outer = (std::mem::replace(&mut self.function, sig.ident.to_string()), self.passes_on);
            self.passes_on = passes_words_on(&self.function);
            self.scopes.push(HashMap::new());
            let params = if self.passes_on { Held::Looked } else { Held::Unknown };
            for input in &sig.inputs {
                if let FnArg::Typed(input) = input {
                    self.bind(&input.pat, None, params);
                }
            }
            for statement in &body.stmts {
                self.visit_stmt(statement);
            }
            // A question's words are drawn where it is asked: they are held
            // here, where it is put together.
            if let (ReturnType::Type(_, ty), Some(Stmt::Expr(Expr::Tuple(question), None))) =
                (&sig.output, body.stmts.last())
                && is_question(ty)
            {
                for (i, words) in question.elems.iter().enumerate() {
                    if self.held(words) != Held::Looked {
                        self.note("a Question", i);
                    }
                }
            }
            self.scopes.pop();
            (self.function, self.passes_on) = outer;
        }
    }

    /// Whether `name` is written as a constant.
    fn is_constant(name: &str) -> bool {
        name.len() > 2 && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    }

    /// The last two segments of `path`, as a drawn call is written.
    fn path_name(path: &syn::Path) -> String {
        let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        names[names.len().saturating_sub(2)..].join("::")
    }

    impl<'a> Visit<'a> for Guard {
        fn visit_item_mod(&mut self, module: &'a ItemMod) {
            let cfg_test =
                |a: &Attribute| a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|cfg| cfg == "test");
            if !module.attrs.iter().any(cfg_test) {
                visit::visit_item_mod(self, module);
            }
        }

        fn visit_item_fn(&mut self, item: &'a ItemFn) {
            self.function(&item.sig, &item.block);
        }

        fn visit_impl_item_fn(&mut self, item: &'a ImplItemFn) {
            self.function(&item.sig, &item.block);
        }

        fn visit_block(&mut self, block: &'a Block) {
            self.scopes.push(HashMap::new());
            visit::visit_block(self, block);
            self.scopes.pop();
        }

        fn visit_local(&mut self, local: &'a Local) {
            visit::visit_local(self, local);
            self.bind(&local.pat, local.init.as_ref().map(|init| &*init.expr), Held::Unknown);
        }

        fn visit_expr_closure(&mut self, closure: &'a ExprClosure) {
            self.scopes.push(HashMap::new());
            for input in &closure.inputs {
                let question = matches!(input, Pat::Type(typed) if is_question(&typed.ty));
                let params = if self.passes_on || question { Held::Looked } else { Held::Unknown };
                self.bind(input, None, params);
            }
            self.visit_expr(&closure.body);
            self.scopes.pop();
        }

        fn visit_expr_method_call(&mut self, call: &'a ExprMethodCall) {
            // What a run says of itself in Activity is no dialog's.
            let of_a_run = call.method == "message"
                && matches!(&*call.receiver, Expr::Path(receiver) if receiver.path.is_ident("ctx"));
            if !of_a_run {
                self.check(&format!(".{}", call.method), &call.args);
            }
            visit::visit_expr_method_call(self, call);
        }

        fn visit_expr_call(&mut self, call: &'a ExprCall) {
            let name = match stripped(&call.func) {
                Expr::Path(path) => path_name(&path.path),
                Expr::Field(ExprField { member: Member::Named(name), .. }) => name.to_string(),
                _ => String::new(),
            };
            self.check(&name, &call.args);
            visit::visit_expr_call(self, call);
        }
    }

    /// Each drawn argument of `source` that didn't come from the catalog.
    fn drawn_words(source: &str) -> Vec<String> {
        let mut guard = Guard::default();
        guard.visit_file(&syn::parse_file(source).unwrap());
        guard.found
    }

    #[test]
    fn no_dialog_or_menu_has_its_words_in_the_code() {
        let sources = app_sources();
        assert!(sources.iter().any(|(path, _)| path.ends_with("mac_menu.rs")));
        for (path, code) in sources {
            assert_eq!(drawn_words(&code), Vec::<String>::new(), "{}", path.display());
        }
    }

    /// `statements` in a function that has looked its words up.
    fn drawn_words_in_a_function(statements: &str) -> Vec<String> {
        let looked = ["title", "text", "yes", "cancel", "asked", "answer"]
            .map(|name| format!("let {name} = w.say(\"key\", &[]);"))
            .concat();
        drawn_words(&format!(
            "fn shown() {{ {looked}
{statements}
; }}"
        ))
    }

    #[test]
    fn the_guard_finds_words_in_a_dialog() {
        for fixture in [
            r#"dialog.message("Sure?").title(title)"#,
            r#"dialog.message(text).title("AutoScene")"#,
            r#"dialog.message(format!("Sure, {name}?"))"#,
            r#"dialog.buttons(MessageDialogButtons::OkCancelCustom("Yes".into(), w.say("dialog.cancel", &[])))"#,
            r#"confirm(&app, ALLOW_TITLE, text, yes)"#,
            r#"confirm(&app, consts::ALLOW_TITLE, text, yes)"#,
            r#"io.save_to(&format!("Plugin “{}” saves a file", m.name), &name, &filters)"#,
            r#"io.pick("Choose", &filters)"#,
            r#"(sv.ask)(&format!("Save {file}"), &name, &[kinds], folder)"#,
            r#"dialog.set_title("Choose")"#,
            r#"SubmenuBuilder::new(app, "File")"#,
            r#"builder.maximize_with_text("Zoom")"#,
            r#"MenuItemBuilder::new("Open")"#,
            r#"MenuItemBuilder::with_id("id", "Open")"#,
            r#"MenuItem::with_id(app, "id", "Open", true, None::<&str>)"#,
            r#"tauri::menu::MenuItem::new(app, "Open", true, None::<&str>)"#,
            r#"CheckMenuItem::new(app, "Mute", true, false, None::<&str>)"#,
            r#"IconMenuItem::with_id(app, "id", "Open", true, None, None::<&str>)"#,
            r#"PredefinedMenuItem::quit(app, Some("Quit"))"#,
            r#"builder.text("id", "Open")"#,
            r#"builder.check("id", "Mute")"#,
            r#"builder.icon("id", "Open", icon)"#,
            r#"MessageDialogBuilder::new(dialog, "Title", text)"#,
            r#"MessageDialogBuilder::new(dialog, title, "Text")"#,
            // Words that reach the call by another way.
            r#"let asked = format!("Plugin “{}” asks to open:\n\n{url}", m.name); io.confirm(&title, &asked, &answer)"#,
            r#"let title = "Are you sure?"; dialog.title(title)"#,
            r#"let title = String::from("Are you sure?"); dialog.title(&title)"#,
            r#"let cancel = "Cancel".to_owned(); MessageDialogButtons::OkCancelCustom(yes, cancel)"#,
            r#"let (title, yes) = ("Sure?", w.say("key", &[])); dialog.title(title)"#,
            r#"let title = if plugin { "Sure?" } else { w.say("key", &[]) }; dialog.title(title)"#,
            r#"let title = match plugin { true => w.say("key", &[]), false => "Sure?".into() }; dialog.title(title)"#,
            r#"let title = "Sure?"; let asked = format!("{title}\n"); dialog.message(asked)"#,
            r#"let shown = "Sure?"; builder.text("id", shown)"#,
            r#"dialog.message(w.say("key", &[("x", "Sure, words here?")]))"#,
            r#"dialog.message(w.say("key", &[("x", &format!("Sure, {name}?"))]))"#,
            r#"let said = "Sure?"; dialog.message(w.say("key", &[("x", said)]))"#,
            r#"let said = w.say("key", &[("x", "Sure?")]); dialog.message(said)"#,
            // Only a message of a run's own is no dialog's.
            r#"ctx.title("Sure?")"#,
            // What can't be followed to the catalog.
            r#"dialog.title(m.name)"#,
            r#"dialog.title(somewhere())"#,
            r#"dialog.title(unknown)"#,
            r#"dialog.title(concat!("Sure", "?"))"#,
        ] {
            assert_eq!(drawn_words_in_a_function(fixture).len(), 1, "{fixture}");
        }
        for fixture in [
            r#"fn warn(title: &str) { dialog.title(title) }"#,
            r#"fn warn() { let shown = |title: &str| dialog.title(title); }"#,
            r#"fn shown() -> Question { ("Sure?".to_owned(), w.say("key", &[]), w.say("key", &[])) }"#,
            r#"fn shown() -> Question { let text = format!("Really? {}", w.say("key", &[])); (w.say("key", &[]), text, w.say("key", &[])) }"#,
        ] {
            assert_eq!(drawn_words(fixture).len(), 1, "{fixture}");
        }
    }

    #[test]
    fn the_guard_leaves_what_the_catalog_says() {
        for fixture in [
            r#"project.title()"#,
            r#"dialog.message(text).title(title)"#,
            r#"confirm(&app, &w.say("dialog.openLink.title", &[]), asked, &w.say("dialog.openLink.yes", &[]))"#,
            r#"SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, label(w, "submenu:window"))"#,
            r#"MenuItemBuilder::with_id(id, label(words, id))"#,
            r#"// dialog.title("Commented out")"#,
            r#"ctx.message(&format!("Saved {file}"))"#,
            r#"let said = w.say("key", &[("name", &m.name), ("n", &n.to_string())]); dialog.title(said)"#,
            r#"let asked = format!("{text}\n\n{title}"); io.confirm(&title, &asked, &yes)"#,
            r#"let shown = format!("\n{}", w.say("key", &[])); dialog.message(&shown)"#,
            r#"let (title, yes) = (w.say("key", &[]), w.say("key", &[])); confirm(&app, &title, text, &yes)"#,
            r#"let title = if plugin { w.say("key", &[]) } else { text.clone() }; dialog.title(title)"#,
            r#"let ask = |(title, text, yes): plugins::Question| confirm(&app, &title, text, &yes)"#,
            r#"self.check(base, &held, now)"#,
            r#"paths::check(&path)"#,
        ] {
            assert_eq!(drawn_words_in_a_function(fixture), Vec::<String>::new(), "{fixture}");
        }
        for fixture in [
            r#"fn confirm(app: &AppHandle, title: &str, message: String) { app.dialog().message(message).title(title) }"#,
            r#"fn save_dialog() { Arc::new(move |title: &str| dialog.set_title(title)) }"#,
            r#"fn shown() -> Question { let text = format!("{}\n", w.say("key", &[])); (w.say("key", &[]), text, w.say("key", &[])) }"#,
            r#"#[cfg(test)] mod tests { fn shown() { dialog.title("Sure?") } }"#,
        ] {
            assert_eq!(drawn_words(fixture), Vec::<String>::new(), "{fixture}");
        }
    }

    /// Every key in `source`: what it says, and the keys it chooses between.
    fn said_keys(source: &str) -> Vec<String> {
        let after = |mark: &str, opening: &str| -> Vec<String> {
            let tails = source.split(mark).skip(1);
            tails.filter_map(|tail| tail.split('"').next()).map(|key| format!("{opening}{key}")).collect()
        };
        [after("say(\"", ""), after("\"dialog.", "dialog.")].concat()
    }

    /// The keys the code says and the keys the catalog holds are the same.
    #[test]
    fn the_code_says_every_key_and_no_other() {
        let mut said: Vec<String> = app_sources().iter().flat_map(|(_, code)| said_keys(code)).collect();
        let own = include_str!("native_text.rs")
            .split(
                "#[cfg(test)]
pub(crate) mod tests",
            )
            .next()
            .unwrap();
        said.extend(said_keys(own));
        said.extend(crate::mac_menu::LABELS.iter().map(|(_, key)| (*key).to_owned()));
        said.sort_unstable();
        said.dedup();
        assert_eq!(said, english().0.keys().cloned().collect::<Vec<_>>());
    }
}
