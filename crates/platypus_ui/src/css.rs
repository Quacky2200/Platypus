// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! CSS stylesheets: parsing with `cssparser` and selectors with Servo's `selectors`
//! crate (DESIGN.md §5.2). Declarations are kept as CSS text and applied with the
//! same `apply_style` as inline styles, after kebab-case → camelCase.

use std::borrow::Borrow;
use std::fmt;

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser as CssParser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, ToCss,
    match_ignore_ascii_case, serialize_identifier,
};
use precomputed_hash::PrecomputedHash;
use selectors::parser::{
    NonTSPseudoClass, ParseRelative, PseudoElement as PseudoElementTrait, SelectorImpl,
    SelectorParseErrorKind,
};
use selectors::{Parser as SelectorParser, SelectorList};

// --- Selector implementation -----------------------------------------------------

/// Our `selectors::SelectorImpl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatypusSelectors;

/// A string used for identifiers, element names and attribute values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CssString(pub String);

impl ToCss for CssString {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        serialize_identifier(&self.0, dest)
    }
}

impl From<&str> for CssString {
    fn from(s: &str) -> Self {
        CssString(s.to_owned())
    }
}

impl From<String> for CssString {
    fn from(s: String) -> Self {
        CssString(s)
    }
}

impl AsRef<str> for CssString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PrecomputedHash for CssString {
    fn precomputed_hash(&self) -> u32 {
        // Must depend only on the contents (used by the ancestor bloom filter).
        self.0.bytes().fold(0x811c_9dc5_u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        })
    }
}

/// Pseudo-classes beyond the tree-structural ones `selectors` handles itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PseudoClass {
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
    Enabled,
    Disabled,
    Checked,
    /// A text field whose text breaks no constraint (`validity.rs`), and one that does.
    Valid,
    Invalid,
    /// A text field with `required`, and one without.
    Required,
    Optional,
    /// Selectors Level 4 `:blank`: no element children and only whitespace text.
    Blank,
    /// s&box's `:outro`: the element was removed and plays its exit transitions
    /// before it goes.
    Outro,
}

impl ToCss for PseudoClass {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(match self {
            PseudoClass::Hover => ":hover",
            PseudoClass::Active => ":active",
            PseudoClass::Focus => ":focus",
            PseudoClass::FocusVisible => ":focus-visible",
            PseudoClass::FocusWithin => ":focus-within",
            PseudoClass::Enabled => ":enabled",
            PseudoClass::Disabled => ":disabled",
            PseudoClass::Checked => ":checked",
            PseudoClass::Valid => ":valid",
            PseudoClass::Invalid => ":invalid",
            PseudoClass::Required => ":required",
            PseudoClass::Optional => ":optional",
            PseudoClass::Blank => ":blank",
            PseudoClass::Outro => ":outro",
        })
    }
}

impl NonTSPseudoClass for PseudoClass {
    fn is_active_or_hover(&self) -> bool {
        matches!(self, PseudoClass::Active | PseudoClass::Hover)
    }

    fn is_user_action_state(&self) -> bool {
        matches!(
            self,
            PseudoClass::Active
                | PseudoClass::Hover
                | PseudoClass::Focus
                | PseudoClass::FocusVisible
                | PseudoClass::FocusWithin
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    Before,
    After,
    /// A list item's marker (bullet or number).
    Marker,
    /// Selected text (colour and background only).
    Selection,
    /// A text field's placeholder.
    Placeholder,
    /// The box behind a modal dialog, covering the window.
    Backdrop,
}

impl ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(match self {
            PseudoElement::Before => "::before",
            PseudoElement::After => "::after",
            PseudoElement::Marker => "::marker",
            PseudoElement::Selection => "::selection",
            PseudoElement::Placeholder => "::placeholder",
            PseudoElement::Backdrop => "::backdrop",
        })
    }
}

impl PseudoElementTrait for PseudoElement {
    fn is_before_or_after(&self) -> bool {
        true
    }
}

impl SelectorImpl for PlatypusSelectors {
    type ExtraMatchingData<'a> = std::marker::PhantomData<&'a ()>;
    type AttrValue = CssString;
    type Identifier = CssString;
    type LocalName = CssString;
    type NamespaceUrl = CssString;
    type NamespacePrefix = CssString;
    type BorrowedLocalName = CssString;
    type BorrowedNamespaceUrl = CssString;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
}

impl Borrow<str> for CssString {
    fn borrow(&self) -> &str {
        &self.0
    }
}

struct PlatypusSelectorParser;

impl<'i> SelectorParser<'i> for PlatypusSelectorParser {
    type Impl = PlatypusSelectors;
    type Error = SelectorParseErrorKind;

    fn parse_nth_child_of(&self) -> bool {
        true
    }

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_non_ts_pseudo_class(
        &self,
        name: CowRcStr<'i>,
    ) -> Result<PseudoClass, ParseError<Self::Error>> {
        let class = match_ignore_ascii_case! { &name,
            "hover" => PseudoClass::Hover,
            "active" => PseudoClass::Active,
            "focus" => PseudoClass::Focus,
            "focus-visible" => PseudoClass::FocusVisible,
            "focus-within" => PseudoClass::FocusWithin,
            "enabled" => PseudoClass::Enabled,
            "disabled" => PseudoClass::Disabled,
            "checked" => PseudoClass::Checked,
            "valid" => PseudoClass::Valid,
            "invalid" => PseudoClass::Invalid,
            "required" => PseudoClass::Required,
            "optional" => PseudoClass::Optional,
            "blank" => PseudoClass::Blank,
            "outro" => PseudoClass::Outro,
            _ => return Err(ParseError::custom(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement,
            )),
        };
        Ok(class)
    }

    fn parse_pseudo_element(
        &self,
        name: CowRcStr<'i>,
    ) -> Result<PseudoElement, ParseError<Self::Error>> {
        match_ignore_ascii_case! { &name,
            "before" => Ok(PseudoElement::Before),
            "after" => Ok(PseudoElement::After),
            "marker" => Ok(PseudoElement::Marker),
            "selection" => Ok(PseudoElement::Selection),
            "placeholder" => Ok(PseudoElement::Placeholder),
            "backdrop" => Ok(PseudoElement::Backdrop),
            _ => Err(ParseError::custom(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement,
            )),
        }
    }
}

/// Parses a selector list such as `.card > text:first-child, #main::before`.
pub fn parse_selectors(text: &str) -> Result<SelectorList<PlatypusSelectors>, String> {
    let mut parser = CssParser::new(text);
    SelectorList::parse(&PlatypusSelectorParser, &mut parser, ParseRelative::No)
        .map_err(|e| format!("invalid selector `{text}`: {:?}", e.kind))
}

// --- Stylesheets -------------------------------------------------------------------

/// One `prop: value` declaration. `prop` is camelCase (custom properties keep
/// their `--name`); `value` is the CSS text, e.g. `"1px solid red"`.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Debug)]
pub struct StyleRule {
    pub selectors: SelectorList<PlatypusSelectors>,
    pub declarations: Vec<Declaration>,
    /// `@media` conditions around the rule: all must match for it to apply.
    pub media: Vec<std::sync::Arc<crate::media::MediaList>>,
}

/// An `@font-face` rule, loaded by the engine like `loadFont()`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFace {
    pub family: String,
    /// The first `url(...)` in `src`, relative to the game folder.
    pub src: String,
    pub weight: Option<f32>,
    pub italic: Option<bool>,
}

/// An `@keyframes` rule: its frames sorted by offset (0..=1); a selector list like
/// `0%, 100%` gives one frame per offset.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframes {
    pub name: String,
    pub frames: Vec<Keyframe>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    pub offset: f32,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug, Default)]
pub struct Stylesheet {
    pub rules: Vec<StyleRule>,
    pub font_faces: Vec<FontFace>,
    pub keyframes: Vec<Keyframes>,
    /// Rules inside `@starting-style`: the "before" style of newly shown elements,
    /// which their transitions start from.
    pub starting: Vec<StyleRule>,
    /// `@import` URLs, in order, with their media condition (the engine loads them;
    /// see `UiTree::add_stylesheet_with_imports`).
    pub imports: Vec<(String, Option<std::sync::Arc<crate::media::MediaList>>)>,
    /// Problems found while parsing (invalid selectors, unsupported at-rules, ...).
    /// The rest of the sheet still applies.
    pub warnings: Vec<String>,
}

enum Item {
    /// A rule and the rules nested in it (flattened), with warnings from inside.
    Rules(Vec<StyleRule>, Vec<String>),
    FontFace(FontFace),
    Keyframes(Keyframes),
    Starting(Stylesheet),
    Import(String, Option<std::sync::Arc<crate::media::MediaList>>),
    /// The contents of an `@media` block, its rules already carrying the condition.
    Media(Stylesheet),
    Skipped(String),
}

enum AtPrelude {
    FontFace,
    Keyframes(String),
    StartingStyle,
    /// The URL, and the conditions after it (a media query list; `supports()` and
    /// `layer` aren't supported).
    Import(Option<String>, String),
    Media(String),
    Unsupported(String),
}

struct SheetParser;

impl<'i> QualifiedRuleParser<'i> for SheetParser {
    /// The selectors, and their text (for rules nested inside).
    type Prelude = (SelectorList<PlatypusSelectors>, String);
    type QualifiedRule = Item;
    type Error = SelectorParseErrorKind;

    fn parse_prelude(
        &mut self,
        input: &mut CssParser<'i>,
    ) -> Result<Self::Prelude, ParseError<Self::Error>> {
        let start = input.position();
        let selectors = SelectorList::parse(&PlatypusSelectorParser, input, ParseRelative::No)?;
        Ok((selectors, input.slice_from(start).trim().to_owned()))
    }

    fn parse_block(
        &mut self,
        (selectors, text): Self::Prelude,
        _start: &ParserState,
        input: &mut CssParser<'i>,
    ) -> Result<Item, ParseError<Self::Error>> {
        let mut rules = Vec::new();
        let mut warnings = Vec::new();
        parse_rule_body(input, selectors, &text, &mut rules, &mut warnings, 0);
        Ok(Item::Rules(rules, warnings))
    }
}

/// How deep rules may nest (deeper ones are skipped with a warning).
const MAX_NESTING: usize = 16;

/// Parses a rule's block: its declarations, then the rules nested in it (CSS
/// Nesting), flattened into `out` after the rule itself.
fn parse_rule_body(
    input: &mut CssParser<'_>,
    selectors: SelectorList<PlatypusSelectors>,
    selector_text: &str,
    out: &mut Vec<StyleRule>,
    warnings: &mut Vec<String>,
    depth: usize,
) {
    let mut parser = BodyParser {
        parent: selector_text,
        depth,
    };
    let mut declarations = Vec::new();
    let mut nested = Vec::new();
    for item in RuleBodyParser::new(input, &mut parser) {
        match item {
            Ok(BodyItem::Declaration(d)) => declarations.push(d),
            Ok(BodyItem::Rules(rules, w)) => {
                nested.extend(rules);
                warnings.extend(w);
            }
            Ok(BodyItem::Skipped(why)) => warnings.push(why),
            Err((_, text, _)) => {
                let text = text.trim();
                // Bad declarations are dropped silently, as before nesting existed;
                // a bad nested rule (it has a block) gets a warning.
                if text.contains('{') {
                    warnings.push(format!("skipped nested rule `{text}`"));
                }
            }
        }
    }
    // A rule that only holds nested rules adds nothing itself.
    if !declarations.is_empty() || nested.is_empty() {
        out.push(StyleRule {
            selectors,
            declarations,
            media: Vec::new(),
        });
    }
    out.extend(nested);
}

/// A nested selector made absolute: `&` is the parent (`:is()` around a list);
/// without `&` it's a descendant of the parent (or `> .x` / `+ .x` / `~ .x` relative
/// to it).
pub fn resolve_nested(nested: &str, parent: &str) -> String {
    let parent = parent.trim();
    let parent = if crate::paint::split_top_level(parent, ',').len() > 1 {
        format!(":is({parent})")
    } else {
        parent.to_owned()
    };
    crate::paint::split_top_level(nested, ',')
        .iter()
        .map(|part| {
            let part = part.trim();
            if part.contains('&') {
                part.replace('&', &parent)
            } else {
                format!("{parent} {part}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

enum BodyItem {
    Declaration(Declaration),
    Rules(Vec<StyleRule>, Vec<String>),
    Skipped(String),
}

struct BodyParser<'a> {
    parent: &'a str,
    depth: usize,
}

impl<'i> DeclarationParser<'i> for BodyParser<'_> {
    type Declaration = BodyItem;
    type Error = ();

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut CssParser<'i>,
        start: &ParserState,
    ) -> Result<BodyItem, ParseError<()>> {
        let d = DeclParser.parse_value(name, input, start)?;
        // `text:hover { ... }` starts like a declaration; a block in the value means
        // it's a nested rule, which the body parser then tries (CSS Nesting).
        if d.value.contains('{') {
            return Err(input.new_error_for_next_token());
        }
        Ok(BodyItem::Declaration(d))
    }
}

impl<'i> QualifiedRuleParser<'i> for BodyParser<'_> {
    /// The nested selector, resolved against the parent.
    type Prelude = String;
    type QualifiedRule = BodyItem;
    type Error = ();

    fn parse_prelude(&mut self, input: &mut CssParser<'i>) -> Result<String, ParseError<()>> {
        let start = input.position();
        while input.next().is_ok() {}
        Ok(resolve_nested(input.slice_from(start), self.parent))
    }

    fn parse_block(
        &mut self,
        text: String,
        _start: &ParserState,
        input: &mut CssParser<'i>,
    ) -> Result<BodyItem, ParseError<()>> {
        if self.depth >= MAX_NESTING {
            return Ok(BodyItem::Skipped(format!(
                "`{text}` is nested too deeply; skipped"
            )));
        }
        let selectors = match parse_selectors(&text) {
            Ok(s) => s,
            Err(e) => return Ok(BodyItem::Skipped(format!("{e}; skipped"))),
        };
        let (mut rules, mut warnings) = (Vec::new(), Vec::new());
        parse_rule_body(
            input,
            selectors,
            &text,
            &mut rules,
            &mut warnings,
            self.depth + 1,
        );
        Ok(BodyItem::Rules(rules, warnings))
    }
}

impl<'i> AtRuleParser<'i> for BodyParser<'_> {
    /// The at-rule's name and prelude text.
    type Prelude = (String, String);
    type AtRule = BodyItem;
    type Error = ();

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut CssParser<'i>,
    ) -> Result<(String, String), ParseError<()>> {
        let start = input.position();
        while input.next().is_ok() {}
        Ok((name.to_string(), input.slice_from(start).trim().to_owned()))
    }

    fn rule_without_block(
        &mut self,
        (name, _): (String, String),
        _start: &ParserState,
    ) -> Result<BodyItem, ()> {
        Ok(BodyItem::Skipped(format!(
            "nested @{name} isn't supported; skipped"
        )))
    }

    fn parse_block(
        &mut self,
        (name, prelude): (String, String),
        _start: &ParserState,
        input: &mut CssParser<'i>,
    ) -> Result<BodyItem, ParseError<()>> {
        // `@media` inside a rule: its declarations and rules belong to the parent
        // selector, under the condition.
        if name.eq_ignore_ascii_case("media")
            && let Some(media) = crate::media::MediaList::parse(&prelude)
            && let Ok(selectors) = parse_selectors(self.parent)
        {
            let (mut rules, mut warnings) = (Vec::new(), Vec::new());
            parse_rule_body(
                input,
                selectors,
                self.parent,
                &mut rules,
                &mut warnings,
                self.depth + 1,
            );
            let media = std::sync::Arc::new(media);
            for rule in &mut rules {
                rule.media.insert(0, media.clone());
            }
            return Ok(BodyItem::Rules(rules, warnings));
        }
        while input.next().is_ok() {}
        Ok(BodyItem::Skipped(format!(
            "nested @{name} {prelude} isn't supported; skipped"
        )))
    }
}

impl<'i> RuleBodyItemParser<'i, BodyItem, ()> for BodyParser<'_> {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        true
    }
}

impl<'i> AtRuleParser<'i> for SheetParser {
    type Prelude = AtPrelude;
    type AtRule = Item;
    type Error = SelectorParseErrorKind;

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut CssParser<'i>,
    ) -> Result<AtPrelude, ParseError<Self::Error>> {
        let start = input.position();
        while input.next().is_ok() {}
        let rest = input.slice_from(start).trim().to_owned();
        Ok(match_ignore_ascii_case! { &name,
            "font-face" => AtPrelude::FontFace,
            "keyframes" | "-webkit-keyframes" => AtPrelude::Keyframes(unquote(&rest)),
            "starting-style" => AtPrelude::StartingStyle,
            "import" => {
                // `url("x.css")`, `url(x.css)` or `"x.css"`, then optional conditions.
                let (url, rest) = if let Some(r) = rest.strip_prefix("url(") {
                    match r.split_once(')') {
                        Some((u, after)) => (Some(unquote(u)), after.trim().to_owned()),
                        None => (None, String::new()),
                    }
                } else if let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    match rest[1..].split_once(q) {
                        Some((u, after)) => (Some(u.to_owned()), after.trim().to_owned()),
                        None => (None, String::new()),
                    }
                } else {
                    (None, String::new())
                };
                AtPrelude::Import(url.filter(|u| !u.is_empty()), rest)
            },
            "media" => AtPrelude::Media(rest),
            _ => AtPrelude::Unsupported(name.to_string()),
        })
    }

    fn rule_without_block(&mut self, prelude: AtPrelude, _start: &ParserState) -> Result<Item, ()> {
        if let AtPrelude::Import(url, conditions) = prelude {
            let Some(url) = url else {
                return Ok(Item::Skipped("@import needs a URL; skipped".into()));
            };
            if conditions.is_empty() {
                return Ok(Item::Import(url, None));
            }
            let media = (!conditions.contains("supports(") && !conditions.contains("layer"))
                .then(|| crate::media::MediaList::parse(&conditions))
                .flatten();
            return Ok(match media {
                Some(m) => Item::Import(url, Some(std::sync::Arc::new(m))),
                None => Item::Skipped(format!(
                    "@import \"{url}\": supports() and layer conditions aren't supported; skipped"
                )),
            });
        }
        Ok(Item::Skipped(match prelude {
            AtPrelude::Unsupported(name) => format!("@{name} isn't supported; skipped"),
            _ => "at-rule without a block; skipped".into(),
        }))
    }

    fn parse_block(
        &mut self,
        prelude: AtPrelude,
        _start: &ParserState,
        input: &mut CssParser<'i>,
    ) -> Result<Item, ParseError<Self::Error>> {
        match prelude {
            AtPrelude::FontFace => {
                let decls = parse_declarations(input);
                let get = |p: &str| decls.iter().find(|d| d.prop == p).map(|d| d.value.as_str());
                let family = get("fontFamily").map(unquote);
                let src = get("src").and_then(first_url);
                match (family, src) {
                    (Some(family), Some(src)) => Ok(Item::FontFace(FontFace {
                        family,
                        src,
                        weight: get("fontWeight").and_then(|w| match w {
                            "normal" => Some(400.0),
                            "bold" => Some(700.0),
                            other => other.parse().ok(),
                        }),
                        italic: get("fontStyle").map(|s| s == "italic" || s == "oblique"),
                    })),
                    _ => Ok(Item::Skipped(
                        "@font-face needs `font-family` and a `src: url(...)`".into(),
                    )),
                }
            }
            AtPrelude::Keyframes(name) => {
                let mut parser = KeyframeParser;
                let mut frames: Vec<Keyframe> = RuleBodyParser::new(input, &mut parser)
                    .filter_map(Result::ok)
                    .flatten()
                    .collect();
                // Stable: a later block at the same offset comes after (and wins).
                frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
                if name.is_empty() || name == "none" {
                    return Ok(Item::Skipped("@keyframes needs a name".into()));
                }
                Ok(Item::Keyframes(Keyframes { name, frames }))
            }
            AtPrelude::StartingStyle => {
                let start = input.position();
                while input.next().is_ok() {}
                Ok(Item::Starting(parse_stylesheet(input.slice_from(start))))
            }
            AtPrelude::Media(query) => {
                let start = input.position();
                while input.next().is_ok() {}
                let Some(media) = crate::media::MediaList::parse(&query) else {
                    return Ok(Item::Skipped(format!(
                        "@media {query}: invalid query; skipped"
                    )));
                };
                let mut inner = parse_stylesheet(input.slice_from(start));
                let media = std::sync::Arc::new(media);
                for rule in inner.rules.iter_mut().chain(inner.starting.iter_mut()) {
                    rule.media.insert(0, media.clone());
                }
                Ok(Item::Media(inner))
            }
            AtPrelude::Import(..) => {
                while input.next().is_ok() {}
                Ok(Item::Skipped("@import can't have a block; skipped".into()))
            }
            AtPrelude::Unsupported(name) => {
                while input.next().is_ok() {}
                Ok(Item::Skipped(format!("@{name} isn't supported; skipped")))
            }
        }
    }
}

/// Parses `prop: value [!important];` declarations in a rule block.
fn parse_declarations(input: &mut CssParser<'_>) -> Vec<Declaration> {
    let mut parser = DeclParser;
    RuleBodyParser::new(input, &mut parser)
        .filter_map(Result::ok)
        .collect()
}

struct DeclParser;

impl<'i> DeclarationParser<'i> for DeclParser {
    type Declaration = Declaration;
    type Error = ();

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut CssParser<'i>,
        _start: &ParserState,
    ) -> Result<Declaration, ParseError<()>> {
        let start = input.position();
        while input.next_including_whitespace_and_comments().is_ok() {}
        let mut value = input.slice_from(start).trim();
        let mut important = false;
        if let Some(bang) = value.rfind('!')
            && value[bang + 1..].trim().eq_ignore_ascii_case("important")
        {
            important = true;
            value = value[..bang].trim();
        }
        // Custom properties keep their text exactly; everything else gets minifier-joined
        // functions (`translateY(0)scale(1)`) spaced, as the property parsers split on spaces.
        let value = if name.starts_with("--") {
            value.to_owned()
        } else {
            space_after_calls(value)
        };
        Ok(Declaration {
            prop: camel_case(&name),
            value,
            important,
        })
    }
}

/// Puts a space between a `)` and a word or `#` straight after it (`blur(2px)brightness(2)`,
/// as minifiers write them). In CSS those are separate values either way. Strings are left
/// alone.
fn space_after_calls(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 4);
    let mut quote = None;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) if c == '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ')'
                && chars
                    .peek()
                    .is_some_and(|n| n.is_ascii_alphabetic() || *n == '#') =>
            {
                out.push(' ')
            }
            None => {}
        }
    }
    out
}

impl<'i> AtRuleParser<'i> for DeclParser {
    type Prelude = ();
    type AtRule = Declaration;
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for DeclParser {
    type Prelude = ();
    type QualifiedRule = Declaration;
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, Declaration, ()> for DeclParser {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// The blocks inside `@keyframes`: `from`, `to` and percentages.
struct KeyframeParser;

impl<'i> QualifiedRuleParser<'i> for KeyframeParser {
    type Prelude = Vec<f32>;
    type QualifiedRule = Vec<Keyframe>;
    type Error = ();

    fn parse_prelude(
        &mut self,
        input: &mut CssParser<'i>,
    ) -> Result<Self::Prelude, ParseError<()>> {
        let start = input.position();
        while input.next().is_ok() {}
        let text = input.slice_from(start);
        text.split(',')
            .map(|s| match s.trim() {
                s if s.eq_ignore_ascii_case("from") => Some(0.0),
                s if s.eq_ignore_ascii_case("to") => Some(1.0),
                s => s
                    .strip_suffix('%')?
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|p| (0.0..=100.0).contains(p))
                    .map(|p| p / 100.0),
            })
            .collect::<Option<Vec<f32>>>()
            .ok_or_else(|| ParseError::custom(()))
    }

    fn parse_block(
        &mut self,
        offsets: Self::Prelude,
        _start: &ParserState,
        input: &mut CssParser<'i>,
    ) -> Result<Vec<Keyframe>, ParseError<()>> {
        // `!important` is ignored in keyframes, as in CSS.
        let declarations: Vec<Declaration> = parse_declarations(input)
            .into_iter()
            .filter(|d| !d.important)
            .collect();
        Ok(offsets
            .into_iter()
            .map(|offset| Keyframe {
                offset,
                declarations: declarations.clone(),
            })
            .collect())
    }
}

impl<'i> AtRuleParser<'i> for KeyframeParser {
    type Prelude = ();
    type AtRule = Vec<Keyframe>;
    type Error = ();
}

impl<'i> DeclarationParser<'i> for KeyframeParser {
    type Declaration = Vec<Keyframe>;
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, Vec<Keyframe>, ()> for KeyframeParser {
    fn parse_declarations(&self) -> bool {
        false
    }

    fn parse_qualified(&self) -> bool {
        true
    }
}

/// `background-color` → `backgroundColor`; custom properties (`--x`) are unchanged.
/// Decodes the CSS escape that follows a backslash in a string (`rest` starts after the
/// backslash): up to six hex digits and one closing space (`\201C `) give that code point
/// (U+FFFD for zero, a surrogate or past U+10FFFF), a newline is a line continuation (no
/// character), anything else stands for itself. Returns the character and the bytes of `rest` used.
pub fn css_escape(rest: &str) -> (Option<char>, usize) {
    let Some(first) = rest.chars().next() else {
        return (None, 0);
    };
    if first.is_ascii_hexdigit() {
        let digits = rest
            .bytes()
            .take(6)
            .take_while(u8::is_ascii_hexdigit)
            .count();
        let code = u32::from_str_radix(&rest[..digits], 16).unwrap_or(0);
        let ch = char::from_u32(code)
            .filter(|c| *c != '\0')
            .unwrap_or(char::REPLACEMENT_CHARACTER);
        // One whitespace ends the escape ("\r\n" counts as one).
        let tail = &rest[digits..];
        let space = if tail.starts_with("\r\n") {
            2
        } else if tail.starts_with([' ', '\t', '\n', '\r', '\u{c}']) {
            1
        } else {
            0
        };
        return (Some(ch), digits + space);
    }
    if rest.starts_with("\r\n") {
        return (None, 2);
    }
    if matches!(first, '\n' | '\r' | '\u{c}') {
        return (None, 1);
    }
    (Some(first), first.len_utf8())
}

pub fn camel_case(prop: &str) -> String {
    if prop.starts_with("--") {
        return prop.to_owned();
    }
    let mut out = String::with_capacity(prop.len());
    let mut upper = false;
    for c in prop.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches(['"', '\'']).to_owned()
}

/// The first `url(...)` in an `@font-face` `src` list that Platypus can load: TrueType,
/// OpenType, WOFF or WOFF2 (by `format()`, else by extension). `local()` entries and
/// EOT / SVG fonts are skipped.
fn first_url(src: &str) -> Option<String> {
    crate::paint::split_top_level(src, ',')
        .into_iter()
        .find_map(|entry| {
            let entry = entry.trim();
            let start = entry.find("url(")? + 4;
            let end = entry[start..].find(')')? + start;
            let url = unquote(&entry[start..end]);
            let format = entry[end..].find("format(").map(|f| {
                unquote(entry[end + f + 7..].split(')').next().unwrap_or("")).to_ascii_lowercase()
            });
            let usable = match format.as_deref() {
                Some(f) => matches!(
                    f,
                    "truetype"
                        | "opentype"
                        | "woff"
                        | "woff2"
                        | "collection"
                        | "truetype-variations"
                        | "opentype-variations"
                        | "woff-variations"
                        | "woff2-variations"
                ),
                None => {
                    let path = url
                        .split(['?', '#'])
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    !(path.ends_with(".eot") || path.ends_with(".svg"))
                }
            };
            (!url.is_empty() && usable).then_some(url)
        })
}

/// Parses a stylesheet. Invalid rules are skipped with a warning; the rest applies.
pub fn parse_stylesheet(css: &str) -> Stylesheet {
    let mut input = CssParser::new(css);
    let mut parser = SheetParser;
    let mut sheet = Stylesheet::default();
    for item in StyleSheetParser::new(&mut input, &mut parser) {
        match item {
            Ok(Item::Rules(rules, warnings)) => {
                sheet.rules.extend(rules);
                sheet.warnings.extend(warnings);
            }
            Ok(Item::FontFace(face)) => sheet.font_faces.push(face),
            Ok(Item::Keyframes(k)) => sheet.keyframes.push(k),
            Ok(Item::Import(url, media)) => sheet.imports.push((url, media)),
            Ok(Item::Media(inner)) => {
                sheet.rules.extend(inner.rules);
                sheet.starting.extend(inner.starting);
                sheet.keyframes.extend(inner.keyframes);
                sheet.font_faces.extend(inner.font_faces);
                sheet.imports.extend(inner.imports);
                sheet.warnings.extend(inner.warnings);
            }
            Ok(Item::Starting(inner)) => {
                sheet.starting.extend(inner.rules);
                sheet.warnings.extend(inner.warnings);
            }
            Ok(Item::Skipped(why)) => sheet.warnings.push(why),
            Err((error, text, location)) => sheet.warnings.push(format!(
                "line {}: skipped `{}` ({:?})",
                location.line + 1,
                text.trim(),
                error.kind
            )),
        }
    }
    sheet
}

// M3-CSS-01
#[cfg(test)]
mod tests {
    use super::*;

    // M3-CSS-13
    #[test]
    fn minified_function_lists_get_spaces_between_calls() {
        let sheet = parse_stylesheet(
            ".a { transform: translateY(0)scale(1.07); filter: blur(2px)brightness(2); \
             content: \"a)b\"; background: url(x.png)#fff; --raw: f(1)g(2); }",
        );
        let values: Vec<&str> = sheet.rules[0]
            .declarations
            .iter()
            .map(|d| d.value.as_str())
            .collect();
        assert_eq!(
            values,
            [
                "translateY(0) scale(1.07)",
                "blur(2px) brightness(2)",
                "\"a)b\"",
                "url(x.png) #fff",
                "f(1)g(2)",
            ]
        );
    }

    #[test]
    fn parses_rules_and_declarations() {
        let sheet = parse_stylesheet(
            r#"
            /* a comment */
            .card, #main > text { background-color: #fff; padding: 4px 8px !important; }
            box:first-child::before { content: "x"; --accent: red; }
            "#,
        );
        assert!(sheet.warnings.is_empty(), "{:?}", sheet.warnings);
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rules[0].selectors.slice().len(), 2);
        assert_eq!(
            sheet.rules[0].declarations,
            [
                Declaration {
                    prop: "backgroundColor".into(),
                    value: "#fff".into(),
                    important: false
                },
                Declaration {
                    prop: "padding".into(),
                    value: "4px 8px".into(),
                    important: true
                },
            ]
        );
        assert_eq!(sheet.rules[1].declarations[0].value, "\"x\"");
        assert_eq!(sheet.rules[1].declarations[1].prop, "--accent");
    }

    #[test]
    fn parses_font_faces() {
        let sheet = parse_stylesheet(
            r#"@font-face { font-family: "Charm"; src: url("fonts/Charm-Bold.ttf") format("truetype"); font-weight: bold; }"#,
        );
        assert_eq!(
            sheet.font_faces,
            [FontFace {
                family: "Charm".into(),
                src: "fonts/Charm-Bold.ttf".into(),
                weight: Some(700.0),
                italic: None
            }]
        );
    }

    #[test]
    fn bad_rules_warn_and_the_rest_still_applies() {
        let sheet = parse_stylesheet(
            r#"
            .ok { width: 10px; }
            .broken:no-such-pseudo { width: 1px; }
            @media (min-width: 600px) { .ok { width: 20px; } }
            @keyframes spin { from { opacity: 0 } bogus { opacity: 1 } }
            @supports (display: grid) { .x { width: 1px } }
            .also-ok { height: 5px }
            "#,
        );
        assert_eq!(sheet.rules.len(), 3, "{:?}", sheet.warnings);
        assert_eq!(sheet.warnings.len(), 2, "{:?}", sheet.warnings);
        assert!(sheet.warnings.iter().any(|w| w.contains("@supports")));
        // The @media rule is kept, with its condition.
        assert_eq!(sheet.rules[1].media.len(), 1);
        // A bad keyframe selector drops just that block.
        assert_eq!(sheet.keyframes.len(), 1);
        assert_eq!(sheet.keyframes[0].frames.len(), 1);
    }

    // M3-FNT-03
    #[test]
    fn font_face_src_lists_pick_a_usable_format() {
        let src = |s: &str| first_url(s);
        assert_eq!(
            src(r#"local("Inter"), url(a.eot) format("embedded-opentype"), url(a.woff2) format("woff2"), url(a.ttf)"#)
                .as_deref(),
            Some("a.woff2")
        );
        assert_eq!(src("url(a.svg#f), url(b.woff)").as_deref(), Some("b.woff"));
        assert_eq!(
            src(r#"url("c.otf") format("opentype")"#).as_deref(),
            Some("c.otf")
        );
        assert_eq!(src(r#"url(d.eot)"#), None);
    }

    // M3-CSS-11
    #[test]
    fn nested_rules_flatten_against_their_parent() {
        let sheet = parse_stylesheet(
            ".card {
                color: red;
                &:hover { color: blue }
                .title { width: 1px; &.big { width: 2px } }
                > .icon { height: 1px }
                text:first-child { opacity: 0.5 }
                margin: 4px;
            }
            .a, .b { & + & { width: 3px } }",
        );
        assert!(sheet.warnings.is_empty(), "{:?}", sheet.warnings);
        let rules: Vec<(String, Vec<&str>)> = sheet
            .rules
            .iter()
            .map(|r| {
                (
                    r.selectors.to_css_string(),
                    r.declarations.iter().map(|d| d.prop.as_str()).collect(),
                )
            })
            .collect();
        let expect = [
            (".card", vec!["color", "margin"]),
            (".card:hover", vec!["color"]),
            (".card .title", vec!["width"]),
            (".card .title.big", vec!["width"]),
            (".card > .icon", vec!["height"]),
            (".card text:first-child", vec!["opacity"]),
            (":is(.a, .b) + :is(.a, .b)", vec!["width"]),
        ];
        assert_eq!(rules.len(), expect.len(), "{rules:?}");
        for ((got, props), (want, want_props)) in rules.iter().zip(expect) {
            assert_eq!(got, want);
            assert_eq!(props, &want_props, "{got}");
        }
    }

    // M3-CSS-11
    #[test]
    fn bad_nested_rules_warn() {
        let sheet = parse_stylesheet(".a { &::nope { width: 1px } @supports (x) { } width: 2px }");
        assert_eq!(sheet.rules.len(), 1);
        assert_eq!(
            sheet.rules[0].declarations.len(),
            1,
            "the rest still applies"
        );
        assert_eq!(sheet.warnings.len(), 2, "{:?}", sheet.warnings);
        assert_eq!(resolve_nested("& > &", ".x .y"), ".x .y > .x .y");
        assert_eq!(resolve_nested(".c, &:hover", ".p"), ".p .c, .p:hover");
    }

    // M3-ANI-03
    #[test]
    fn keyframes_parse_offsets_in_order() {
        let sheet = parse_stylesheet(
            "@keyframes pulse { to { opacity: 1 } 0%, 50% { opacity: 0 !important; width: 1px } 25% { width: 2px } }",
        );
        assert!(sheet.warnings.is_empty(), "{:?}", sheet.warnings);
        let k = &sheet.keyframes[0];
        assert_eq!(k.name, "pulse");
        let offsets: Vec<f32> = k.frames.iter().map(|f| f.offset).collect();
        assert_eq!(offsets, [0.0, 0.25, 0.5, 1.0]);
        let props: Vec<&str> = k.frames[0]
            .declarations
            .iter()
            .map(|d| d.prop.as_str())
            .collect();
        assert_eq!(props, ["width"], "!important is ignored in keyframes");
    }

    #[test]
    fn selectors_cover_the_planned_set() {
        for ok in [
            "*",
            "box",
            ".a.b",
            "#id",
            "[data-kind]",
            r#"[data-kind="enemy"]"#,
            "a > b + c ~ d e",
            ":first-child, :last-child, :only-child, :nth-child(2n+1), :nth-child(odd of .x)",
            ":nth-last-child(2), :first-of-type, :last-of-type, :nth-of-type(3), :root, :empty, :blank",
            ":not(.a), :is(.a, .b), :where(.c), :has(> .child), :has(+ .sib)",
            ":hover, :active, :focus, :focus-visible, :focus-within, :enabled, :disabled, :checked",
            ".x::before, .y::after, li::marker",
        ] {
            assert!(
                parse_selectors(ok).is_ok(),
                "{ok}: {:?}",
                parse_selectors(ok).err()
            );
        }
        assert!(parse_selectors(":visited-later").is_err());
        assert!(parse_selectors("::first-letter").is_err());
    }

    #[test]
    fn camel_cases_property_names() {
        assert_eq!(camel_case("background-color"), "backgroundColor");
        assert_eq!(camel_case("border-top-left-radius"), "borderTopLeftRadius");
        assert_eq!(camel_case("--my-var"), "--my-var");
        assert_eq!(camel_case("width"), "width");
    }
}

/// Replaces `var(--name, fallback)` references in a declaration value using the
/// element's custom properties. Returns `None` when a variable without a fallback
/// is undefined (CSS then treats the declaration as invalid at computed time).
/// Variables may reference other variables; cycles and runaway nesting give `None`.
pub fn substitute_vars(value: &str, vars: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    substitute_depth(value, vars, 0)
}

fn substitute_depth(
    value: &str,
    vars: &dyn Fn(&str) -> Option<String>,
    depth: u32,
) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("var(") {
        out.push_str(&rest[..start]);
        let args_start = start + 4;
        // Find the matching close paren (fallbacks may contain parens).
        let mut depth_parens = 1;
        let mut end = None;
        for (i, c) in rest[args_start..].char_indices() {
            match c {
                '(' => depth_parens += 1,
                ')' => {
                    depth_parens -= 1;
                    if depth_parens == 0 {
                        end = Some(args_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end?;
        let args = &rest[args_start..end];
        let (name, fallback) = match args.split_once(',') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (args.trim(), None),
        };
        if !name.starts_with("--") {
            return None;
        }
        let resolved = match vars(name) {
            Some(v) => substitute_depth(&v, vars, depth + 1),
            None => None,
        };
        let replacement = match (resolved, fallback) {
            (Some(v), _) => v,
            (None, Some(f)) => substitute_depth(f, vars, depth + 1)?,
            (None, None) => return None,
        };
        out.push_str(&replacement);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod var_tests {
    use super::*;

    fn vars(name: &str) -> Option<String> {
        match name {
            "--accent" => Some("red".into()),
            "--gap" => Some("8px".into()),
            "--border" => Some("1px solid var(--accent)".into()),
            "--loop-a" => Some("var(--loop-b)".into()),
            "--loop-b" => Some("var(--loop-a)".into()),
            _ => None,
        }
    }

    // M3-CSS-05
    #[test]
    fn substitutes_variables() {
        assert_eq!(
            substitute_vars("var(--accent)", &vars).as_deref(),
            Some("red")
        );
        assert_eq!(
            substitute_vars("var(--gap) var(--gap)", &vars).as_deref(),
            Some("8px 8px")
        );
        assert_eq!(
            substitute_vars("var(--border)", &vars).as_deref(),
            Some("1px solid red"),
            "nested"
        );
        assert_eq!(
            substitute_vars("var(--missing, blue)", &vars).as_deref(),
            Some("blue"),
            "fallback"
        );
        assert_eq!(
            substitute_vars("var(--missing, rgba(0, 0, 0, 0.5))", &vars).as_deref(),
            Some("rgba(0, 0, 0, 0.5)"),
            "fallback with commas and parens"
        );
        assert_eq!(
            substitute_vars("var(--missing, var(--accent))", &vars).as_deref(),
            Some("red")
        );
        assert_eq!(
            substitute_vars("var(--missing)", &vars),
            None,
            "undefined without fallback"
        );
        assert_eq!(
            substitute_vars("var(--loop-a)", &vars),
            None,
            "cycles are invalid"
        );
        assert_eq!(
            substitute_vars("no vars here", &vars).as_deref(),
            Some("no vars here")
        );
    }
}

/// Rewrites relative `url(...)` references in `value` to be relative to `base`
/// (a folder inside the game folder). Used for stylesheets the bundler wrote into
/// `build/`, whose urls are relative to the stylesheet. `data:` URLs and anything
/// with a scheme are left alone.
pub fn rebase_urls(value: &str, base: &str) -> String {
    let base = base.trim_end_matches(['/', '\\']);
    if base.is_empty() || !value.contains("url(") {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len() + base.len());
    let mut rest = value;
    while let Some(start) = rest.find("url(") {
        out.push_str(&rest[..start + 4]);
        let after = &rest[start + 4..];
        let Some(end) = after.find(')') else {
            out.push_str(after);
            return out;
        };
        let raw = after[..end].trim();
        let url = raw.trim_matches(['"', '\'']);
        if url.contains(':') || url.starts_with(['/', '\\']) || url.is_empty() {
            out.push_str(raw);
        } else {
            let url = url.strip_prefix("./").unwrap_or(url);
            out.push('"');
            out.push_str(base);
            out.push('/');
            out.push_str(url);
            out.push('"');
        }
        out.push(')');
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod rebase_tests {
    use super::*;

    #[test]
    fn rebases_relative_urls_only() {
        assert_eq!(
            rebase_urls(
                r#"url("./_.._/assets/font.ttf") format("truetype")"#,
                "build"
            ),
            r#"url("build/_.._/assets/font.ttf") format("truetype")"#
        );
        assert_eq!(
            rebase_urls("url(a.png), url(b.png)", "build/"),
            r#"url("build/a.png"), url("build/b.png")"#
        );
        assert_eq!(
            rebase_urls("url(data:image/png;base64,AAA=)", "build"),
            "url(data:image/png;base64,AAA=)"
        );
        assert_eq!(rebase_urls("red", "build"), "red");
        assert_eq!(rebase_urls("url(x.png)", ""), "url(x.png)");
    }
}
