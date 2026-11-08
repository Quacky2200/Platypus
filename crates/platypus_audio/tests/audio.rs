// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use kira::AudioManagerSettings;
use platypus_audio::{Audio, OfflineBackend, PlayOptions, gain_to_db};
use platypus_core::Value;

const RATE: u32 = 8000;

fn audio() -> Audio<OfflineBackend> {
    let mut a = Audio::with_settings(AudioManagerSettings {
        backend_settings: RATE,
        ..Default::default()
    })
    .unwrap();
    a.set_sample_rate(RATE);
    a
}

/// The highest frequency there is: +0.5, -0.5, +0.5 ... (a low-pass should kill it).
fn buzz(seconds: f32) -> Vec<[f32; 2]> {
    (0..(seconds * RATE as f32) as usize)
        .map(|i| if i % 2 == 0 { [0.5, 0.5] } else { [-0.5, -0.5] })
        .collect()
}

/// A constant (DC) mono signal: easy to measure gains on.
fn tone(seconds: f32) -> Vec<[f32; 2]> {
    vec![[0.5, 0.5]; (seconds * RATE as f32) as usize]
}

/// Mean absolute level of each channel over the frames.
fn level(frames: &[[f32; 2]]) -> [f32; 2] {
    let n = frames.len().max(1) as f32;
    let sum = frames.iter().fold([0.0, 0.0], |acc, f| {
        [acc[0] + f[0].abs(), acc[1] + f[1].abs()]
    });
    [sum[0] / n, sum[1] / n]
}

fn op(a: &mut Audio<OfflineBackend>, name: &str, args: Vec<Value>) {
    a.apply(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
}

fn num(v: f64) -> Value {
    Value::Number(v)
}

fn arr(items: &[f64]) -> Value {
    Value::Array(items.iter().copied().map(num).collect())
}

/// A 16-bit mono WAV file of a square wave.
fn wav(seconds: f32) -> Vec<u8> {
    let samples = (seconds * RATE as f32) as u32;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + samples * 2).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((samples * 2).to_le_bytes());
    for i in 0..samples {
        let v: i16 = if (i / 20) % 2 == 0 { 12000 } else { -12000 };
        out.extend(v.to_le_bytes());
    }
    out
}

// M4-AUD-01
#[test]
fn playing_a_sound_produces_output_then_reports_it_ended() {
    let mut a = audio();
    assert_eq!(level(&a.backend_mut().render(800)), [0.0, 0.0]);

    a.load(1, wav(0.25), false).unwrap();
    a.load(2, wav(0.25), true).unwrap();
    assert!(a.load(3, b"not audio".to_vec(), false).is_err());
    op(&mut a, "play", vec![num(10.0), num(1.0), Value::Null]);
    op(&mut a, "play", vec![num(11.0), num(2.0), Value::Null]);
    let [l, r] = level(&a.backend_mut().render(800));
    assert!(l > 0.3 && (l - r).abs() < 1e-3, "{l} {r}");
    assert_eq!(a.poll(), Vec::<u32>::new());

    // Both play out after a quarter second. The streamed one decodes on its own thread,
    // which can lag behind rendering this much faster than real time, so give it time.
    let mut ended = Vec::new();
    for _ in 0..500 {
        a.backend_mut().render(800);
        ended.extend(a.poll());
        if ended.len() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    ended.sort_unstable();
    assert_eq!(ended, vec![10, 11]);
    assert_eq!(level(&a.backend_mut().render(800)), [0.0, 0.0]);
    assert_eq!(a.playing(), 0);
}

// M4-AUD-03
#[test]
fn volume_pan_buses_and_stop() {
    let mut a = audio();
    a.load_frames(1, RATE, tone(5.0));
    // Half volume, hard right.
    a.play(
        1,
        1,
        None,
        PlayOptions {
            volume: 0.5,
            pan: 1.0,
            looped: true,
            ..Default::default()
        },
    )
    .unwrap();
    let [l, r] = level(&a.backend_mut().render(400));
    assert!(l < 1e-3 && r > 0.2, "hard right: {l} {r}");

    // A bus at a quarter volume scales what is played on it (0.5 × 0.25; centre
    // panning leaves both channels at full level).
    op(&mut a, "bus", vec![num(5.0), Value::Null, num(0.25)]);
    op(&mut a, "play", vec![num(2.0), num(1.0), num(5.0)]);
    op(&mut a, "stop", vec![num(1.0)]);
    a.backend_mut().render(400);
    let [l, r] = level(&a.backend_mut().render(400));
    assert!(
        (l - 0.125).abs() < 1e-3 && (r - 0.125).abs() < 1e-3,
        "bus: {l} {r}"
    );

    // Muting the master silences everything.
    op(&mut a, "trackVolume", vec![Value::Null, num(0.0)]);
    a.backend_mut().render(400);
    assert!(level(&a.backend_mut().render(400))[0] < 1e-3);
    assert_eq!(a.poll(), vec![1]);
}

// M5-APP-06
#[test]
fn muting_for_the_background_keeps_the_scripts_master_volume() {
    let mut a = audio();
    a.load_frames(1, RATE, tone(5.0));
    op(&mut a, "play", vec![num(1.0), num(1.0), Value::Null]);
    let full = level(&a.backend_mut().render(400))[0];
    assert!(full > 0.4, "{full}");

    a.set_muted(true);
    a.backend_mut().render(2000);
    assert!(level(&a.backend_mut().render(400))[0] < 1e-3);
    // The master set while muted applies once unmuted, not before.
    op(&mut a, "trackVolume", vec![Value::Null, num(0.5)]);
    a.backend_mut().render(400);
    assert!(level(&a.backend_mut().render(400))[0] < 1e-3);

    a.set_muted(false);
    a.backend_mut().render(2000);
    let back = level(&a.backend_mut().render(400))[0];
    assert!((back - full * 0.5).abs() < 0.02, "{back} vs {full}");
}

// M4-AUD-02
#[test]
fn emitters_fall_off_with_distance_and_pan_to_their_side() {
    let mut a = audio();
    a.load_frames(1, RATE, tone(5.0));
    // Listener at the origin facing -z (no rotation); emitters 1 m right and 10 m right,
    // full volume within 1 m and silent past 20 m.
    op(&mut a, "listener", vec![arr(&[0.0, 0.0, 0.0])]);
    op(
        &mut a,
        "emitter",
        vec![num(7.0), arr(&[1.0, 0.0, 0.0]), arr(&[1.0, 20.0])],
    );
    op(&mut a, "play", vec![num(1.0), num(1.0), num(7.0)]);
    a.backend_mut().render(400);
    let [near_l, near_r] = level(&a.backend_mut().render(400));
    assert!(near_r > near_l * 2.0, "panned right: {near_l} {near_r}");

    op(
        &mut a,
        "emitterPosition",
        vec![num(7.0), arr(&[10.0, 0.0, 0.0])],
    );
    a.backend_mut().render(400);
    let [_, far_r] = level(&a.backend_mut().render(400));
    assert!(
        far_r < near_r * 0.6 && far_r > 0.0,
        "fades with distance: {near_r} → {far_r}"
    );

    op(
        &mut a,
        "emitterPosition",
        vec![num(7.0), arr(&[30.0, 0.0, 0.0])],
    );
    a.backend_mut().render(400);
    assert!(
        level(&a.backend_mut().render(400))[1] < 1e-3,
        "silent past max distance"
    );

    // Moving it to the left swaps the channels.
    op(
        &mut a,
        "emitterPosition",
        vec![num(7.0), arr(&[-1.0, 0.0, 0.0])],
    );
    a.backend_mut().render(400);
    let [l, r] = level(&a.backend_mut().render(400));
    assert!(l > r * 2.0, "panned left: {l} {r}");
}

// M4-AUD-04
#[test]
fn gains_convert_to_decibels_and_bad_ops_fail() {
    assert_eq!(gain_to_db(1.0).0, 0.0);
    assert!((gain_to_db(0.5).0 + 6.0206).abs() < 1e-3);
    assert_eq!(gain_to_db(0.0), kira::Decibels::SILENCE);

    let mut a = audio();
    for (name, args) in [
        ("play", vec![num(1.0), num(99.0), Value::Null]),
        ("set", vec![num(1.0), "volume".into(), num(1.0)]),
        ("bus", vec![num(1.0), num(42.0)]),
        ("emitterPosition", vec![num(1.0), arr(&[0.0, 0.0, 0.0])]),
        ("listener", vec![arr(&[0.0, f64::NAN, 0.0])]),
        ("warp", vec![]),
    ] {
        assert!(a.apply(name, &args).is_err(), "{name} {args:?} should fail");
    }
    // Stopping something already finished is fine.
    op(&mut a, "stop", vec![num(123.0)]);
}

// ACO-FX-01
#[test]
fn a_bus_low_pass_removes_highs_and_opens_again() {
    let mut a = audio();
    a.load_frames(1, RATE, buzz(3.0));
    op(
        &mut a,
        "bus",
        vec![num(1.0), Value::Null, num(1.0), num(2.0)],
    );
    op(&mut a, "play", vec![num(10.0), num(1.0), num(1.0)]);
    let open = level(&a.backend_mut().render(400))[0];
    assert!(
        open > 0.4,
        "bus without a filter set passes the highs: {open}"
    );

    op(
        &mut a,
        "fx",
        vec![num(1.0), "lowpass".into(), num(300.0), num(0.0)],
    );
    a.backend_mut().render(800);
    let muffled = level(&a.backend_mut().render(800))[0];
    assert!(muffled < 0.05, "highs removed: {muffled}");

    op(
        &mut a,
        "fx",
        vec![num(1.0), "lowpass".into(), num(22000.0), num(0.0)],
    );
    a.backend_mut().render(800);
    let back = level(&a.backend_mut().render(800))[0];
    assert!(back > 0.4, "open again: {back}");
}

// ACO-FX-02
#[test]
fn a_high_pass_removes_the_lows() {
    let mut a = audio();
    a.load_frames(1, RATE, tone(3.0));
    op(
        &mut a,
        "bus",
        vec![num(1.0), Value::Null, num(1.0), num(2.0)],
    );
    op(&mut a, "play", vec![num(10.0), num(1.0), num(1.0)]);
    op(
        &mut a,
        "fx",
        vec![num(1.0), "highpass".into(), num(1500.0), num(0.0)],
    );
    a.backend_mut().render(1600);
    let thin = level(&a.backend_mut().render(800))[0];
    assert!(thin < 0.05, "a steady level is all low: {thin}");
}

// ACO-FX-03
#[test]
fn reverb_and_echo_leave_a_tail_after_the_sound_ends() {
    let tail = |property: &str| {
        let mut a = audio();
        a.load_frames(1, RATE, tone(0.1));
        op(
            &mut a,
            "bus",
            vec![num(1.0), Value::Null, num(1.0), num(2.0)],
        );
        op(
            &mut a,
            "fx",
            vec![num(1.0), property.into(), num(1.0), num(0.0)],
        );
        op(&mut a, "play", vec![num(10.0), num(1.0), num(1.0)]);
        a.backend_mut().render((RATE as f32 * 0.15) as usize);
        level(&a.backend_mut().render((RATE as f32 * 0.4) as usize))[0]
    };
    // The reverb's size changes nothing while its wet level is still 0.
    let dry = tail("reverbSize");
    assert!(dry < 1e-4, "no tail without them: {dry}");
    assert!(tail("reverb") > 1e-3, "reverb rings on");
    assert!(tail("echo") > 1e-3, "echo repeats");
}

// ACO-FX-04
#[test]
fn emitters_have_a_low_pass_and_effects_report_errors() {
    let mut a = audio();
    op(
        &mut a,
        "emitter",
        vec![
            num(5.0),
            arr(&[0.0, 0.0, -1.0]),
            arr(&[1.0, 50.0]),
            Value::Null,
            num(1.0),
        ],
    );
    op(
        &mut a,
        "fx",
        vec![num(5.0), "lowpass".into(), num(500.0), num(0.0)],
    );
    // Only the full chain has a reverb: asking an emitter that lacks it is ignored, not fatal.
    op(
        &mut a,
        "fx",
        vec![num(5.0), "reverb".into(), num(0.5), num(0.0)],
    );
    assert!(
        a.apply("fx", &[num(5.0), "wobble".into(), num(1.0)])
            .is_err()
    );
    // A plain bus made without effects, and a track that does not exist.
    op(&mut a, "bus", vec![num(2.0), Value::Null, num(1.0)]);
    assert!(
        a.apply("fx", &[num(2.0), "lowpass".into(), num(500.0)])
            .is_err()
    );
    assert!(
        a.apply("fx", &[num(77.0), "lowpass".into(), num(500.0)])
            .is_err()
    );
}

// ACO-FX-05
#[test]
fn the_master_mix_has_effects_as_track_zero() {
    let mut a = audio();
    a.load_frames(1, RATE, buzz(3.0));
    op(&mut a, "play", vec![num(10.0), num(1.0), Value::Null]);
    assert!(level(&a.backend_mut().render(400))[0] > 0.4);
    op(
        &mut a,
        "fx",
        vec![num(0.0), "lowpass".into(), num(300.0), num(0.0)],
    );
    a.backend_mut().render(800);
    assert!(
        level(&a.backend_mut().render(800))[0] < 0.05,
        "everything is dulled"
    );
}
