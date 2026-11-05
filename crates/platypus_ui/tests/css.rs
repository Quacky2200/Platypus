// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Stylesheets applied to the UI tree: selectors, cascade and restyle
//! (M3-CSS-02, -03, -04, -08, -09, -10, -11).

use platypus_ui::opcodes::*;
use platypus_ui::{ElementState, Rgba, UiTree};
use web_ui_core::{Commit, NodeId, Value};

fn n(id: u32) -> Value {
    Value::Number(id.into())
}

fn op(code: u32, args: &[Value]) -> Value {
    let mut v = vec![Value::Number(code.into())];
    v.extend_from_slice(args);
    Value::Array(v)
}

/// Builds trees from a compact description.
struct Dom {
    ops: Vec<Value>,
    next: u32,
}

impl Dom {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            next: 1,
        }
    }

    /// An element with attributes (`class`, `id`, `data-*`, `disabled`, `checked`).
    fn el(&mut self, parent: u32, kind: &str, attrs: &[(&str, Value)]) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops.push(op(CREATE, &[n(id), kind.into()]));
        if !attrs.is_empty() {
            let mut a = vec![n(id)];
            for (k, v) in attrs {
                a.push((*k).into());
                a.push(v.clone());
            }
            self.ops.push(op(SET_ATTRS, &a));
        }
        self.ops.push(op(APPEND_CHILD, &[n(parent), n(id)]));
        id
    }

    fn class(&mut self, parent: u32, class: &str) -> u32 {
        self.el(parent, "box", &[("className", class.into())])
    }

    fn text(&mut self, parent: u32, text: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops.push(op(CREATE_TEXT, &[n(id), text.into()]));
        self.ops.push(op(APPEND_CHILD, &[n(parent), n(id)]));
        id
    }

    fn build(self, css: &str) -> UiTree {
        let mut tree = UiTree::new(400.0, 400.0);
        let errors = tree.apply_commit(&Commit { ops: self.ops });
        assert!(errors.is_empty(), "{errors:?}");
        let added = tree.add_stylesheet(css);
        assert!(added.warnings.is_empty(), "{:?}", added.warnings);
        tree.layout_if_needed();
        tree
    }
}

fn bg(tree: &UiTree, id: u32) -> Rgba {
    tree.visual(NodeId(id)).unwrap().background
}

const RED: Rgba = Rgba::new(1.0, 0.0, 0.0, 1.0);
const GREEN: Rgba = Rgba::new(0.0, 128.0 / 255.0, 0.0, 1.0);
const BLUE: Rgba = Rgba::new(0.0, 0.0, 1.0, 1.0);
const NONE: Rgba = Rgba::TRANSPARENT;

// M3-CSS-08
#[test]
fn simple_selectors_and_specificity() {
    let mut d = Dom::new();
    let a = d.el(
        0,
        "box",
        &[("className", "card big".into()), ("id", "main".into())],
    );
    let b = d.el(
        0,
        "box",
        &[("className", "card".into()), ("data-kind", "enemy".into())],
    );
    let c = d.el(0, "text", &[]);
    let tree = d.build(
        r#"
        * { opacity: 0.5 }
        box { background-color: red }
        .card { background-color: green }
        #main { background-color: blue }
        .card.big { background-color: red }          /* lower specificity than #main */
        [data-kind="enemy"] { border: 1px solid red }
        text { opacity: 1 }
        "#,
    );
    assert_eq!(bg(&tree, a), BLUE, "#id beats .class.class");
    assert_eq!(bg(&tree, b), GREEN, ".class beats type");
    assert_eq!(
        tree.visual(NodeId(b)).unwrap().border_colors[0],
        RED,
        "attribute selector"
    );
    assert_eq!(tree.visual(NodeId(a)).unwrap().opacity, 0.5, "universal");
    assert_eq!(
        tree.visual(NodeId(c)).unwrap().opacity,
        1.0,
        "type beats universal"
    );
}

// M3-CSS-02
#[test]
fn combinators() {
    let mut d = Dom::new();
    let list = d.class(0, "list");
    let first = d.class(list, "item");
    let second = d.class(list, "item special");
    let third = d.class(list, "item");
    let fourth = d.class(list, "item");
    let deep_parent = d.class(list, "wrap");
    let deep = d.class(deep_parent, "item");
    let tree = d.build(
        r#"
        .list .item { background-color: red }          /* descendant: all items */
        .list > .item { background-color: green }      /* child: not the deep one */
        .special + .item { background-color: blue }    /* next sibling */
        .special ~ .item { border: 2px solid blue }    /* following siblings */
        "#,
    );
    assert_eq!(bg(&tree, first), GREEN);
    assert_eq!(bg(&tree, deep), RED, "descendant but not child");
    assert_eq!(bg(&tree, third), BLUE, "+ picks the next sibling only");
    assert_eq!(bg(&tree, fourth), GREEN);
    for id in [third, fourth] {
        assert_eq!(
            tree.visual(NodeId(id)).unwrap().border_colors[0],
            BLUE,
            "~ picks all following"
        );
    }
    assert_eq!(tree.visual(NodeId(second)).unwrap().border_colors[0], NONE);
}

// M3-CSS-02
#[test]
fn structural_pseudo_classes() {
    let mut d = Dom::new();
    let list = d.class(0, "list");
    // Text nodes between elements must not count as siblings.
    d.text(list, " ");
    let items: Vec<u32> = (0..5)
        .map(|i| d.class(list, if i == 2 { "item x" } else { "item" }))
        .collect();
    let only_parent = d.class(0, "solo");
    let only = d.el(only_parent, "text", &[]);
    let tree = d.build(
        r#"
        .item:first-child { background-color: red }
        .item:last-child { background-color: blue }
        .item:nth-child(2) { opacity: 0.2 }
        .item:nth-child(odd of .item) { border: 1px solid green }
        .item:nth-last-child(2) { opacity: 0.4 }
        text:only-child { opacity: 0.6 }
        box:first-of-type { flex-grow: 3 }
        :root { opacity: 0.9 }
        "#,
    );
    assert_eq!(bg(&tree, items[0]), RED);
    assert_eq!(bg(&tree, items[4]), BLUE);
    assert_eq!(tree.visual(NodeId(items[1])).unwrap().opacity, 0.2);
    assert_eq!(tree.visual(NodeId(items[3])).unwrap().opacity, 0.4);
    for (i, id) in items.iter().enumerate() {
        let expected = if i % 2 == 0 { GREEN } else { NONE };
        assert_eq!(
            tree.visual(NodeId(*id)).unwrap().border_colors[0],
            expected,
            "item {i}"
        );
    }
    assert_eq!(tree.visual(NodeId(only)).unwrap().opacity, 0.6);
    assert_eq!(
        tree.visual(platypus_ui::ROOT).unwrap().opacity,
        0.9,
        ":root is the root container"
    );
}

// M3-CSS-03
#[test]
fn logical_pseudo_classes() {
    let mut d = Dom::new();
    let plain = d.class(0, "a");
    let other = d.class(0, "b");
    let parent_with = d.class(0, "p");
    d.class(parent_with, "child");
    let parent_without = d.class(0, "p");
    d.class(parent_without, "other");
    let before_sib = d.class(0, "q");
    d.class(0, "marker");
    let tree = d.build(
        r#"
        box:not(.a) { opacity: 0.5 }
        :is(.a, .b) { background-color: red }
        :where(.b) { background-color: green }        /* zero specificity: loses to :is(.b) */
        .p:has(> .child) { background-color: blue }
        .q:has(+ .marker) { border: 1px solid red }
        "#,
    );
    assert_eq!(tree.visual(NodeId(plain)).unwrap().opacity, 1.0);
    assert_eq!(tree.visual(NodeId(other)).unwrap().opacity, 0.5);
    assert_eq!(bg(&tree, plain), RED);
    assert_eq!(bg(&tree, other), RED, ":where has zero specificity");
    assert_eq!(bg(&tree, parent_with), BLUE);
    assert_eq!(bg(&tree, parent_without), NONE);
    assert_eq!(
        tree.visual(NodeId(before_sib)).unwrap().border_colors[0],
        RED
    );
}

// M3-CSS-09
#[test]
fn empty_and_blank() {
    let mut d = Dom::new();
    let empty = d.class(0, "t");
    let blank = d.class(0, "t");
    d.text(blank, "  \n ");
    let texty = d.class(0, "t");
    d.text(texty, "hi");
    let parent = d.class(0, "t");
    d.class(parent, "kid");
    let tree = d.build(
        r#"
        .t:empty { background-color: red }
        .t:blank { border: 1px solid blue }
        "#,
    );
    assert_eq!(bg(&tree, empty), RED);
    assert_eq!(bg(&tree, blank), NONE, "whitespace text isn't :empty");
    assert_eq!(tree.visual(NodeId(empty)).unwrap().border_colors[0], BLUE);
    assert_eq!(
        tree.visual(NodeId(blank)).unwrap().border_colors[0],
        BLUE,
        "whitespace text is :blank"
    );
    for id in [texty, parent] {
        assert_eq!(bg(&tree, id), NONE);
        assert_eq!(tree.visual(NodeId(id)).unwrap().border_colors[0], NONE);
    }
}

// M3-CSS-10
#[test]
fn state_and_form_pseudo_classes() {
    let mut d = Dom::new();
    let button = d.class(0, "btn");
    let off = d.el(
        0,
        "box",
        &[("className", "btn".into()), ("disabled", true.into())],
    );
    let check = d.el(
        0,
        "box",
        &[("className", "check".into()), ("checked", true.into())],
    );
    let form = d.class(0, "form");
    let field = d.class(form, "field");
    let mut tree = d.build(
        r#"
        .btn { background-color: green }
        .btn:hover { background-color: blue }
        .btn:active { background-color: red }
        .btn:enabled { opacity: 1 }
        .btn:disabled { opacity: 0.3 }
        .check:checked { background-color: red }
        .field:focus { border: 2px solid blue }
        .field:focus-visible { opacity: 0.8 }
        .form:focus-within { background-color: blue }
        "#,
    );
    assert_eq!(bg(&tree, button), GREEN);
    assert_eq!(tree.visual(NodeId(off)).unwrap().opacity, 0.3);
    assert_eq!(bg(&tree, check), RED);

    tree.set_state(
        NodeId(button),
        ElementState {
            hover: true,
            ..Default::default()
        },
    );
    tree.layout_if_needed();
    assert_eq!(bg(&tree, button), BLUE, ":hover");
    tree.set_state(
        NodeId(button),
        ElementState {
            hover: true,
            active: true,
            ..Default::default()
        },
    );
    tree.layout_if_needed();
    assert_eq!(bg(&tree, button), RED, ":active comes later in the sheet");
    tree.set_state(NodeId(button), ElementState::default());
    tree.layout_if_needed();
    assert_eq!(bg(&tree, button), GREEN, "back to normal");

    tree.set_state(
        NodeId(field),
        ElementState {
            focus: true,
            focus_visible: true,
            ..Default::default()
        },
    );
    tree.layout_if_needed();
    assert_eq!(tree.visual(NodeId(field)).unwrap().border_colors[0], BLUE);
    assert_eq!(tree.visual(NodeId(field)).unwrap().opacity, 0.8);
    assert_eq!(bg(&tree, form), BLUE, ":focus-within on the ancestor");

    // Toggling an attribute restyles.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_ATTRS, &[n(off), "disabled".into(), Value::Null])],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.visual(NodeId(off)).unwrap().opacity, 1.0);
}

// M3-CSS-04
#[test]
fn cascade_order_inline_and_important() {
    let mut d = Dom::new();
    let a = d.class(0, "a");
    let b = d.class(0, "b");
    let mut tree = d.build(
        r#"
        .a { background-color: red; width: 10px }
        .a { background-color: green }               /* later rule wins at equal specificity */
        .b { background-color: green !important; width: 30px }
        "#,
    );
    assert_eq!(bg(&tree, a), GREEN);

    // Inline styles beat normal rules but not !important ones.
    let errors = tree.apply_commit(&Commit {
        ops: vec![
            op(
                SET_STYLE,
                &[
                    n(a),
                    "backgroundColor".into(),
                    "blue".into(),
                    "width".into(),
                    20.0.into(),
                ],
            ),
            op(
                SET_STYLE,
                &[
                    n(b),
                    "backgroundColor".into(),
                    "blue".into(),
                    "width".into(),
                    40.0.into(),
                ],
            ),
        ],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(bg(&tree, a), BLUE);
    assert_eq!(tree.rect(NodeId(a)).unwrap().width, 20.0);
    assert_eq!(bg(&tree, b), GREEN, "!important beats inline");
    assert_eq!(
        tree.rect(NodeId(b)).unwrap().width,
        40.0,
        "inline beats a normal rule"
    );

    // Removing an inline style falls back to the stylesheet.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            SET_STYLE,
            &[n(a), "backgroundColor".into(), Value::Null],
        )],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(bg(&tree, a), GREEN);
}

// M3-CSS-04: inherited properties from stylesheets flow down to text.
#[test]
fn stylesheet_text_properties_inherit() {
    let mut d = Dom::new();
    let panel = d.class(0, "panel");
    let label = d.el(panel, "text", &[]);
    d.text(label, "hello");
    let tree = d.build(".panel { color: blue; font-size: 24px }");
    let computed = tree.computed_text(NodeId(label));
    assert_eq!(computed.color, BLUE);
    assert_eq!(computed.font_size, 24.0);
    assert!(
        tree.rect(NodeId(label)).unwrap().height > 24.0,
        "re-laid out at the new size"
    );
}

#[test]
fn adding_and_removing_sheets_restyles() {
    let mut d = Dom::new();
    let a = d.class(0, "a");
    let mut tree = d.build(".a { background-color: red }");
    let second = tree.add_stylesheet(".a { background-color: blue }");
    tree.layout_if_needed();
    assert_eq!(bg(&tree, a), BLUE, "later sheets win");
    assert!(tree.remove_stylesheet(second.id));
    tree.layout_if_needed();
    assert_eq!(bg(&tree, a), RED);

    let warned = tree.add_stylesheet(".a { colour: red; width: 10px; background-color: nope }");
    assert_eq!(warned.warnings.len(), 2, "{:?}", warned.warnings);
}

// M3-CSS-05
#[test]
fn custom_properties_inherit_and_substitute() {
    let mut d = Dom::new();
    let panel = d.class(0, "panel");
    let card = d.class(panel, "card");
    let inner = d.class(card, "inner");
    let plain = d.class(0, "card");
    let tree = d.build(
        r#"
        :root { --accent: red; --pad: 6px }
        .panel { --accent: blue }                       /* overrides for its subtree */
        .card { background-color: var(--accent); padding: var(--pad) }
        .inner { border: 2px solid var(--missing, green); width: var(--nope) }
        "#,
    );
    assert_eq!(bg(&tree, card), BLUE, "inherits the nearest definition");
    assert_eq!(bg(&tree, plain), RED, "from :root");
    assert_eq!(
        tree.visual(NodeId(inner)).unwrap().border_colors[0],
        GREEN,
        "fallback"
    );
    // width: var(--nope) is invalid at computed time, so width stays auto.
    assert!(tree.rect(NodeId(inner)).unwrap().width < 400.0);
    let card_rect = tree.rect(NodeId(card)).unwrap();
    let inner_rect = tree.rect(NodeId(inner)).unwrap();
    assert_eq!(
        inner_rect.x - card_rect.x,
        6.0,
        "padding from a :root variable"
    );
}

// M3-CSS-05: inline custom properties, without any stylesheet, update live.
#[test]
fn inline_custom_properties_update_descendants() {
    let mut tree = UiTree::new(200.0, 200.0);
    let ops = vec![
        op(CREATE, &[n(1), "box".into()]),
        op(SET_STYLE, &[n(1), "--accent".into(), "red".into()]),
        op(CREATE, &[n(2), "box".into()]),
        op(
            SET_STYLE,
            &[
                n(2),
                "backgroundColor".into(),
                "var(--accent)".into(),
                "width".into(),
                10.0.into(),
            ],
        ),
        op(APPEND_CHILD, &[n(1), n(2)]),
        op(APPEND_CHILD, &[n(0), n(1)]),
    ];
    assert!(tree.apply_commit(&Commit { ops }).is_empty());
    tree.layout_if_needed();
    assert_eq!(bg(&tree, 2), RED);
    assert_eq!(
        tree.rect(NodeId(2)).unwrap().width,
        10.0,
        "non-var inline styles still apply"
    );

    let ops = vec![op(SET_STYLE, &[n(1), "--accent".into(), "#0000ff".into()])];
    assert!(tree.apply_commit(&Commit { ops }).is_empty());
    tree.layout_if_needed();
    assert_eq!(
        bg(&tree, 2),
        BLUE,
        "changing the variable restyles the child"
    );
}

// M3-CSS-06
#[test]
fn before_and_after_generate_content() {
    let mut d = Dom::new();
    let label = d.el(
        0,
        "text",
        &[("className", "tag".into()), ("data-count", 3.0.into())],
    );
    d.text(label, "Hello");
    let list = d.class(0, "list");
    let first = d.class(list, "item");
    let last = d.class(list, "item");
    let mut tree = d.build(
        r#"
        .tag::before { content: "* "; color: red }
        .tag::after { content: " (" attr(data-count) ")" }
        .list::after { content: ""; width: 10px; height: 10px; background-color: blue }
        .list > .item:last-child { opacity: 0.5 }
        "#,
    );
    // Generated text joins the text block, in order.
    assert_eq!(tree.text_content(NodeId(label)), "* Hello (3)");
    let before = tree.children(NodeId(label))[0];
    assert!(tree.is_generated(before));
    assert_eq!(
        tree.computed_text(before).color,
        RED,
        "::before has its own style"
    );

    // A decorative ::after box in a flex container is laid out and drawn.
    let after = *tree.children(NodeId(list)).last().unwrap();
    assert!(tree.is_generated(after));
    let r = tree.rect(after).unwrap();
    assert_eq!((r.width, r.height), (10.0, 10.0));
    assert_eq!(tree.visual(after).unwrap().background, BLUE);
    assert!(
        tree.draw_list()
            .iter()
            .any(|c| matches!(c, platypus_ui::DrawCmd::Quad(q) if q.rect == r))
    );

    // Generated content is invisible to selectors: the real last item still matches.
    assert_eq!(tree.visual(NodeId(last)).unwrap().opacity, 0.5);
    assert_eq!(tree.visual(NodeId(first)).unwrap().opacity, 1.0);

    // React appends land before ::after.
    let errors = tree.apply_commit(&Commit {
        ops: vec![
            op(CREATE, &[n(50), "box".into()]),
            op(SET_ATTRS, &[n(50), "className".into(), "item".into()]),
            op(APPEND_CHILD, &[n(list), n(50)]),
        ],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    let kids = tree.children(NodeId(list)).to_vec();
    assert_eq!(kids[kids.len() - 2], NodeId(50));
    assert!(tree.is_generated(*kids.last().unwrap()));
    assert_eq!(
        tree.visual(NodeId(50)).unwrap().opacity,
        0.5,
        "new last child"
    );
    assert_eq!(
        tree.visual(NodeId(last)).unwrap().opacity,
        1.0,
        "no longer last"
    );

    // Attribute changes update attr() content.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_ATTRS, &[n(label), "data-count".into(), 7.0.into()])],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.text_content(NodeId(label)), "* Hello (7)");
}

// M3-CSS-06: removing the rules removes the generated nodes.
#[test]
fn generated_content_follows_the_stylesheets() {
    let mut d = Dom::new();
    let label = d.el(0, "text", &[("className", "tag".into())]);
    d.text(label, "Hi");
    let mut tree = d.build("");
    assert_eq!(tree.children(NodeId(label)).len(), 1);

    let sheet =
        tree.add_stylesheet(r#".tag::before { content: "> " } .tag::after { content: none }"#);
    tree.layout_if_needed();
    assert_eq!(tree.text_content(NodeId(label)), "> Hi");
    assert_eq!(
        tree.children(NodeId(label)).len(),
        2,
        "content: none generates nothing"
    );

    tree.remove_stylesheet(sheet.id);
    tree.layout_if_needed();
    assert_eq!(tree.text_content(NodeId(label)), "Hi");
    assert_eq!(tree.children(NodeId(label)).len(), 1);
}

// M3-CSS-15: `content` strings decode CSS escapes: hex code points (with the closing space),
// escaped quotes, line continuations.
#[test]
fn generated_content_decodes_css_escapes() {
    let mut d = Dom::new();
    let label = d.el(0, "text", &[("className", "q".into())]);
    d.text(label, "x");
    let mut tree = d.build("");
    tree.add_stylesheet(
        r#".q::before { content: "\201C" } .q::after { content: "\201D \"a\" \1F600 b\
c" }"#,
    );
    tree.layout_if_needed();
    assert_eq!(
        tree.text_content(NodeId(label)),
        "\u{201C}x\u{201D}\"a\" \u{1F600}bc"
    );
}

fn set_class(tree: &mut UiTree, id: u32, class: &str) {
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_ATTRS, &[n(id), "className".into(), class.into()])],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
}

// M3-CSS-07: targeted invalidation still updates everything that depends on a change.
#[test]
fn restyle_reaches_dependent_nodes() {
    let mut d = Dom::new();
    let list = d.class(0, "list");
    let a = d.class(list, "item");
    let b = d.class(list, "item");
    let c = d.class(list, "item");
    let label = d.class(a, "label");
    let holder = d.class(0, "holder");
    let slot = d.class(holder, "slot");
    let mut tree = d.build(
        r#"
        .item.on + .item { background-color: red }        /* next sibling of .on */
        .item.on .label { background-color: blue }          /* descendant of .on */
        .holder:has(.slot.full) { background-color: green } /* ancestor via :has */
        .item:nth-child(2) { opacity: 0.5 }
        "#,
    );
    assert_eq!(bg(&tree, b), NONE);

    set_class(&mut tree, a, "item on");
    assert_eq!(bg(&tree, b), RED, "+ sibling restyled");
    assert_eq!(bg(&tree, c), NONE);
    assert_eq!(bg(&tree, label), BLUE, "descendant restyled");

    set_class(&mut tree, a, "item");
    assert_eq!(bg(&tree, b), NONE, "and back");
    assert_eq!(bg(&tree, label), NONE);

    set_class(&mut tree, slot, "slot full");
    assert_eq!(bg(&tree, holder), GREEN, ":has ancestor restyled");
    set_class(&mut tree, slot, "slot");
    assert_eq!(bg(&tree, holder), NONE);

    // Inserting a new first item shifts :nth-child(2) from b to a.
    assert_eq!(tree.visual(NodeId(b)).unwrap().opacity, 0.5);
    let errors = tree.apply_commit(&Commit {
        ops: vec![
            op(CREATE, &[n(90), "box".into()]),
            op(SET_ATTRS, &[n(90), "className".into(), "item".into()]),
            op(INSERT_BEFORE, &[n(list), n(90), n(a)]),
        ],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.visual(NodeId(a)).unwrap().opacity, 0.5);
    assert_eq!(tree.visual(NodeId(b)).unwrap().opacity, 1.0);
}

// M3-CSS-11
#[test]
fn nested_rules_apply_and_react_to_state() {
    let mut d = Dom::new();
    let card = d.class(0, "card");
    let title = d.el(card, "text", &[("className", "title".into())]);
    let mut tree =
        d.build(".card { opacity: 0.9; .title { opacity: 0.5 } &:hover .title { opacity: 0.25 } }");
    let opacity = |tree: &UiTree, id: u32| tree.visual(NodeId(id)).unwrap().opacity;
    assert_eq!(opacity(&tree, card), 0.9);
    assert_eq!(opacity(&tree, title), 0.5);
    tree.set_state(
        NodeId(card),
        ElementState {
            hover: true,
            ..Default::default()
        },
    );
    tree.layout_if_needed();
    assert_eq!(
        opacity(&tree, title),
        0.25,
        "the nested :hover rule restyles the child"
    );
}

// M3-CSS-12
#[test]
fn imports_load_before_the_importing_sheet() {
    let mut d = Dom::new();
    let a = d.class(0, "a");
    let mut tree = UiTree::new(400.0, 400.0);
    assert!(tree.apply_commit(&Commit { ops: d.ops }).is_empty());
    let mut asked = Vec::new();
    let added = tree.add_stylesheet_with_imports(
        r#"@import url("theme/base.css");
           @import "missing.css";
           @import "print.css" print;
           .a { opacity: 0.5 }"#,
        "build",
        &mut |path| {
            asked.push(path.to_owned());
            match path {
                // Its own import is relative to its folder; it imports the first file
                // back (a cycle), which is skipped.
                "build/theme/base.css" => Some(
                    "@import '../colors.css'; @import 'base.css';
                     .a { opacity: 0.1; width: 10px; background-image: url(img/x.png) }"
                        .into(),
                ),
                "build/colors.css" => Some(".a { height: 20px }".into()),
                _ => None,
            }
        },
    );
    tree.layout_if_needed();
    assert_eq!(
        asked,
        // `print.css` loads too (`@import ... print`), but its rules only apply in print.
        [
            "build/theme/base.css",
            "build/colors.css",
            "build/missing.css",
            "build/print.css"
        ]
    );
    let v = tree.visual(NodeId(a)).unwrap();
    assert_eq!(
        v.opacity, 0.5,
        "the importing sheet's rule wins (it comes later)"
    );
    let r = tree.rect(NodeId(a)).unwrap();
    assert_eq!((r.width, r.height), (10.0, 20.0), "imported rules apply");
    let warnings = added.warnings.join("\n");
    assert!(warnings.contains("missing.css"), "{warnings}");
    assert!(warnings.contains("imports itself"), "{warnings}");
    // URLs in an imported sheet are relative to its own folder.
    let image = format!("{:?}", v.background_images);
    assert!(image.contains("build/theme/img/x.png"), "{image}");
}

// M3-MQ-02
#[test]
fn media_rules_follow_the_window_and_device() {
    let mut d = Dom::new();
    let a = d.class(0, "a");
    let mut tree = d.build(
        "@media (min-width: 500px) { .a { opacity: 0.5 } }
         .a { width: 50px; align-self: flex-start; @media (max-width: 450px) { width: 10px } }
         @media (prefers-color-scheme: dark) { .a { height: 30px } }
         @media print { .a { opacity: 0.1 } }",
    );
    let v = |tree: &UiTree| {
        let r = tree.rect(NodeId(a)).unwrap();
        (tree.visual(NodeId(a)).unwrap().opacity, r.width, r.height)
    };
    // 400 wide, light.
    assert_eq!(v(&tree), (1.0, 10.0, 0.0));
    tree.set_size(600.0, 400.0);
    tree.layout_if_needed();
    assert_eq!(
        v(&tree),
        (0.5, 50.0, 0.0),
        "resizing re-evaluates the queries"
    );
    tree.set_media_env(platypus_ui::media::MediaEnv {
        dark: true,
        ..tree.media_env()
    });
    tree.layout_if_needed();
    assert_eq!(v(&tree).2, 30.0, "prefers-color-scheme");
    tree.set_size(400.0, 400.0);
    tree.layout_if_needed();
    assert_eq!(v(&tree), (1.0, 10.0, 30.0));
}

// M3-MQ-02
#[test]
fn imports_can_carry_a_media_query() {
    let mut d = Dom::new();
    let a = d.class(0, "a");
    let mut tree = UiTree::new(400.0, 400.0);
    assert!(tree.apply_commit(&Commit { ops: d.ops }).is_empty());
    let added = tree.add_stylesheet_with_imports(
        r#"@import "wide.css" (min-width: 800px); @import "b.css" supports(display: grid);"#,
        "",
        &mut |path| (path == "wide.css").then(|| ".a { opacity: 0.25 }".to_owned()),
    );
    assert_eq!(
        added.warnings.len(),
        1,
        "supports() is skipped: {:?}",
        added.warnings
    );
    tree.layout_if_needed();
    assert_eq!(tree.visual(NodeId(a)).unwrap().opacity, 1.0);
    tree.set_size(900.0, 400.0);
    tree.layout_if_needed();
    assert_eq!(tree.visual(NodeId(a)).unwrap().opacity, 0.25);
}

// M3-CSS-14
#[test]
fn text_set_on_an_empty_node_restyles_empty_and_has() {
    let mut d = Dom::new();
    let page = d.class(0, "page");
    let span = d.el(page, "span", &[]);
    let text = d.text(span, "");
    let mut tree = d.build(
        r#"
        .page:has(span:not(:empty)) { background-color: red }
        span:empty { border: 1px solid blue }
        "#,
    );
    assert_eq!(bg(&tree, page), NONE, "no text yet");
    assert_eq!(tree.visual(NodeId(span)).unwrap().border_colors[0], BLUE);
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_TEXT, &[n(text), "12:00".into()])],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    assert_eq!(
        bg(&tree, page),
        RED,
        "`:has(span:not(:empty))` matches once it has text"
    );
    assert_eq!(tree.visual(NodeId(span)).unwrap().border_colors[0], NONE);
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_TEXT, &[n(text), "".into()])],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    assert_eq!(bg(&tree, page), NONE, "and stops matching when emptied");
}
