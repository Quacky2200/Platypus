// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `image` elements: natural size from the app, `objectFit` / `objectPosition`.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::{SET_ATTRS, SET_STYLE};
use platypus_ui::{BackgroundSize, DrawCmd, Len, Rect, UiTree};
use web_ui_core::{Commit, NodeId, Value};

/// A start-aligned column holding one image per entry of `styles`, all showing `src`.
fn images(src: &str, styles: &[&[(&str, Value)]]) -> (UiTree, Vec<u32>) {
    let mut d = Dom::new();
    let col = d.add(
        0,
        "box",
        &[
            ("flexDirection", "column".into()),
            ("alignItems", "flex-start".into()),
        ],
    );
    let ids: Vec<u32> = styles.iter().map(|s| d.add(col, "image", s)).collect();
    for id in &ids {
        d.ops
            .push(op(SET_ATTRS, &[n(*id), "src".into(), src.into()]));
    }
    (d.build(""), ids)
}

fn rect(tree: &mut UiTree, id: u32) -> Rect {
    tree.layout_if_needed();
    tree.rect(NodeId(id)).unwrap()
}

// M3-IMG-01
#[test]
fn images_take_their_natural_size_once_the_app_reports_it() {
    let (mut tree, ids) = images(
        "assets/a.png",
        &[&[], &[("width", px(100.0))], &[("height", px(10.0))]],
    );
    assert_eq!(rect(&mut tree, ids[0]).width, 0.0, "unknown size yet");
    assert_eq!(
        tree.take_image_requests(),
        vec!["assets/a.png".to_owned()],
        "asked once for all users of the src"
    );
    assert!(tree.take_image_requests().is_empty());

    tree.set_image_size("assets/a.png", Some((40, 20)));
    assert_eq!(rect(&mut tree, ids[0]), Rect::new(0.0, 0.0, 40.0, 20.0));
    let wide = rect(&mut tree, ids[1]);
    assert_eq!((wide.width, wide.height), (100.0, 50.0), "keeps the ratio");
    let short = rect(&mut tree, ids[2]);
    assert_eq!((short.width, short.height), (20.0, 10.0));

    // A failed load leaves the element empty but laid out.
    tree.apply_commit(&Commit {
        ops: vec![op(
            SET_ATTRS,
            &[n(ids[0]), "src".into(), "missing.png".into()],
        )],
    });
    assert_eq!(tree.take_image_requests(), vec!["missing.png".to_owned()]);
    tree.set_image_size("missing.png", None);
    assert_eq!(rect(&mut tree, ids[0]).width, 0.0);
}

/// The image fill drawn for the only image in the tree, with the clip around it.
fn image_fill(tree: &mut UiTree) -> (Rect, BackgroundSize, platypus_ui::BackgroundPosition) {
    let cmds = tree.draw_list();
    let i = cmds
        .iter()
        .position(|c| matches!(c, DrawCmd::Quad(q) if !q.images.is_empty()))
        .expect("an image quad");
    assert!(matches!(cmds[i - 1], DrawCmd::PushClip(_)));
    assert!(matches!(cmds[i + 1], DrawCmd::PopClip));
    let DrawCmd::Quad(q) = &cmds[i] else {
        unreachable!()
    };
    let fill = &q.images[0];
    (q.rect, fill.size, fill.position)
}

// M3-IMG-01
#[test]
fn object_fit_and_position_fit_the_image_into_the_content_box() {
    let (mut tree, ids) = images(
        "a.png",
        &[&[
            ("width", px(100.0)),
            ("height", px(50.0)),
            ("padding", px(5.0)),
            ("borderRadius", px(8.0)),
        ]],
    );
    tree.set_image_size("a.png", Some((20, 10)));
    let (area, size, position) = image_fill(&mut tree);
    assert_eq!(area, Rect::new(5.0, 5.0, 90.0, 40.0), "the content box");
    let full = Some(Len::Percent(1.0));
    assert_eq!(size, BackgroundSize::Explicit(full, full), "fill stretches");
    assert_eq!(
        (position.x, position.y),
        (Len::Percent(0.5), Len::Percent(0.5))
    );

    let set = |tree: &mut UiTree, prop: &str, value: &str| {
        let errors = tree.apply_commit(&Commit {
            ops: vec![op(SET_STYLE, &[n(ids[0]), prop.into(), value.into()])],
        });
        assert!(errors.is_empty(), "{errors:?}");
    };
    set(&mut tree, "objectFit", "cover");
    set(&mut tree, "objectPosition", "left top");
    let (_, size, position) = image_fill(&mut tree);
    assert_eq!(size, BackgroundSize::Cover);
    assert_eq!(
        (position.x, position.y),
        (Len::Percent(0.0), Len::Percent(0.0))
    );

    // scale-down: natural size when it fits, contain when it doesn't.
    set(&mut tree, "objectFit", "scale-down");
    assert_eq!(
        image_fill(&mut tree).1,
        BackgroundSize::Explicit(None, None)
    );
    tree.set_image_size("a.png", Some((200, 100)));
    assert_eq!(image_fill(&mut tree).1, BackgroundSize::Contain);

    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            SET_STYLE,
            &[n(ids[0]), "objectFit".into(), "stretch".into()],
        )],
    });
    assert!(!errors.is_empty(), "unknown objectFit is rejected");
}
