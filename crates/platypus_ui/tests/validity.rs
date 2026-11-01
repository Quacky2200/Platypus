// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Constraint validation of text fields (M3-INP-05): the rules of `platypus_ui::validity`.

use platypus_ui::validity::{Constraints, Validity, check};

fn typed(input_type: &str) -> Constraints<'_> {
    Constraints {
        input_type: Some(input_type),
        ..Constraints::default()
    }
}

// M3-INP-05
#[test]
fn required_fields_are_missing_only_when_empty() {
    let c = Constraints {
        required: true,
        ..Constraints::default()
    };
    assert!(check(&c, "").value_missing);
    assert!(check(&c, " ").valid(), "a space is a value");
    assert!(check(&Constraints::default(), "").valid());
    // Empty text breaks nothing else, even a pattern or a minimum length.
    let c = Constraints {
        pattern: Some("[0-9]+"),
        min_length: Some(3),
        ..Constraints::default()
    };
    assert!(check(&c, "").valid());
}

// M3-INP-05
#[test]
fn email_and_url_fields_need_their_shape() {
    for ok in ["a@b.co", "first.last+tag@sub.example.org", "x@localhost"] {
        assert!(check(&typed("email"), ok).valid(), "{ok}");
    }
    for bad in ["a", "a@", "@b.c", "a b@c.d", "a@b..c", "a@-b.c"] {
        assert!(check(&typed("email"), bad).type_mismatch, "{bad}");
    }
    for ok in ["https://example.com/a?b=c", "mailto:me@example.com"] {
        assert!(check(&typed("url"), ok).valid(), "{ok}");
    }
    for bad in ["example.com", "http:", "http://a b", "://x"] {
        assert!(check(&typed("url"), bad).type_mismatch, "{bad}");
    }
    assert!(check(&typed("text"), "not an email").valid());
}

// M3-INP-05
#[test]
fn pattern_must_match_the_whole_text_and_min_length_counts_characters() {
    let c = Constraints {
        pattern: Some("[a-z]{3}|[0-9]+"),
        ..Constraints::default()
    };
    assert!(check(&c, "abc").valid());
    assert!(check(&c, "123456").valid());
    assert!(
        check(&c, "abcd").pattern_mismatch,
        "a partial match is not enough"
    );
    assert!(check(&c, "xabc").pattern_mismatch);

    let broken = Constraints {
        pattern: Some("(unclosed"),
        ..Constraints::default()
    };
    assert!(
        check(&broken, "anything").valid(),
        "a bad pattern is ignored"
    );

    let area = Constraints {
        textarea: true,
        pattern: Some("x"),
        ..Constraints::default()
    };
    assert!(check(&area, "y").valid(), "a textarea has no pattern");

    let c = Constraints {
        min_length: Some(3),
        ..Constraints::default()
    };
    assert!(check(&c, "ab").too_short);
    assert!(check(&c, "añ😀").valid(), "three characters, not bytes");
}

// M3-INP-05
#[test]
fn number_fields_check_the_range_and_the_step_grid() {
    let c = Constraints {
        min: Some("1"),
        max: Some("10"),
        step: Some("0.5"),
        ..typed("number")
    };
    assert!(check(&c, "1").valid());
    assert!(check(&c, "9.5").valid());
    assert!(check(&c, "0.5").range_underflow);
    assert!(check(&c, "10.5").range_overflow);
    assert!(check(&c, "2.2").step_mismatch);
    assert!(check(&c, "1.5e").bad_input);
    assert!(check(&c, "-").bad_input);
    assert!(check(&c, "inf").bad_input, "words are not numbers");

    // Steps count from min, and 0.3 is on the 0.1 grid despite float noise.
    let grid = Constraints {
        min: Some("1"),
        step: Some("5"),
        ..typed("number")
    };
    assert!(check(&grid, "11").valid());
    assert!(check(&grid, "10").step_mismatch);
    let tenths = Constraints {
        step: Some("0.1"),
        ..typed("number")
    };
    assert!(check(&tenths, "0.3").valid());

    // The default step is 1; `any` lifts it.
    assert!(check(&typed("number"), "1.5").step_mismatch);
    let any = Constraints {
        step: Some("any"),
        ..typed("number")
    };
    assert!(check(&any, "1.5").valid());

    // Number text is not judged by minLength or pattern.
    let c = Constraints {
        pattern: Some("x"),
        min_length: Some(9),
        ..typed("number")
    };
    assert_eq!(check(&c, "5"), Validity::default());
}
