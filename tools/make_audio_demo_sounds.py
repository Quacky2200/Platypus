# Copyright 2026 The Platypus Authors
# SPDX-License-Identifier: Apache-2.0
"""Synthesises the sounds of the `audio-*` examples (no downloads, no licences): a radio tune,
speech-like babble, a shout, waves, wind and birds, cave drips, footsteps, a gunshot, beeps, an
explosion, a horn, an engine, a knock, static. Mono 16-bit WAV at 22.05 kHz, written into each
example's `assets/sounds/`.

    python tools/make_audio_demo_sounds.py            (from the repository root)
"""

import math
import os
import random
import struct
import wave

RATE = 22050
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples")


def write(path, samples):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    peak = max(1e-9, max(abs(s) for s in samples))
    scale = min(1.0, 0.9 / peak) * 32767
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(s * scale)) for s in samples))


def noise(n, rng):
    return [rng.uniform(-1, 1) for _ in range(n)]


def lowpass(xs, cutoff):
    a = 1 - math.exp(-2 * math.pi * cutoff / RATE)
    y, out = 0.0, []
    for x in xs:
        y += a * (x - y)
        out.append(y)
    return out


def highpass(xs, cutoff):
    lo = lowpass(xs, cutoff)
    return [x - l for x, l in zip(xs, lo)]


def resonator(xs, freq, bw):
    """A two-pole band-pass: one formant."""
    r = math.exp(-math.pi * bw / RATE)
    c1 = 2 * r * math.cos(2 * math.pi * freq / RATE)
    c2 = -r * r
    gain = 1 - r
    y1 = y2 = 0.0
    out = []
    for x in xs:
        y = gain * x + c1 * y1 + c2 * y2
        out.append(y)
        y2, y1 = y1, y
    return out


def envelope(n, attack, release):
    a, r = int(attack * RATE), int(release * RATE)
    return [min(1.0, i / max(1, a), (n - i) / max(1, r)) for i in range(n)]


def mix(*tracks):
    n = max(len(t) for t in tracks)
    return [sum(t[i] if i < len(t) else 0.0 for t in tracks) for i in range(n)]


def sine(freq, seconds, phase=0.0):
    return [math.sin(2 * math.pi * freq * i / RATE + phase) for i in range(int(seconds * RATE))]


# ---- the sounds ----


def radio_music():
    """A twelve-second loop: a bright little tune over a bass line, a touch of hiss."""
    rng = random.Random(1)
    beat = 0.25
    melody = [72, 76, 79, 76, 74, 77, 81, 77, 72, 76, 79, 83, 81, 79, 76, 74] * 3
    bass = [48, 48, 55, 55, 53, 53, 55, 50] * 6
    out = [0.0] * int(12 * RATE)
    for i, note in enumerate(melody):
        f = 440 * 2 ** ((note - 69) / 12)
        n = int(beat * RATE)
        seg = [
            (0.6 if math.sin(2 * math.pi * f * k / RATE) > 0 else -0.6) * 0.5
            + 0.5 * math.sin(2 * math.pi * f * k / RATE)
            for k in range(n)
        ]
        env = envelope(n, 0.005, 0.12)
        start = int(i * beat * RATE)
        for k in range(n):
            if start + k < len(out):
                out[start + k] += seg[k] * env[k] * 0.5
    for i, note in enumerate(bass):
        f = 440 * 2 ** ((note - 69) / 12)
        n = int(2 * beat * RATE)
        env = envelope(n, 0.01, 0.2)
        start = int(i * 2 * beat * RATE)
        for k in range(n):
            if start + k < len(out):
                out[start + k] += math.sin(2 * math.pi * f * k / RATE) * env[k] * 0.45
    hiss = [x * 0.03 for x in highpass(noise(len(out), rng), 3000)]
    return mix(out, hiss)


def voice(seconds=9.0, seed=7):
    """Speech-like babble: voiced syllables through vowel formants, with pauses."""
    rng = random.Random(seed)
    vowels = [(730, 1090), (270, 2290), (300, 870), (660, 1720), (520, 1190)]
    out = [0.0] * int(seconds * RATE)
    t = 0.1
    while t < seconds - 0.4:
        words = rng.randint(2, 5)
        pitch = rng.uniform(105, 150)
        for _ in range(words):
            n = int(rng.uniform(0.12, 0.26) * RATE)
            f0 = pitch * rng.uniform(0.9, 1.15)
            f1, f2 = rng.choice(vowels)
            src = [((f0 * k / RATE) % 1.0) * 2 - 1 for k in range(n)]
            src = [s - 0.0 for s in src]
            seg = mix(resonator(src, f1, 90), [x * 0.6 for x in resonator(src, f2, 140)])
            env = envelope(n, 0.03, 0.05)
            start = int(t * RATE)
            for k in range(n):
                if start + k < len(out):
                    out[start + k] += seg[k] * env[k] * 6
            t += n / RATE + rng.uniform(0.01, 0.05)
        t += rng.uniform(0.35, 0.8)
    return out


def shout():
    """"Hey!": a loud voiced vowel gliding up then down."""
    n = int(1.1 * RATE)
    phase, src = 0.0, []
    for k in range(n):
        x = k / n
        f0 = 170 + 150 * math.sin(math.pi * min(1.0, x * 1.4)) - 40 * x
        phase += f0 / RATE
        src.append((phase % 1.0) * 2 - 1)
    seg = mix(resonator(src, 700, 110), [x * 0.7 for x in resonator(src, 1200, 150)])
    env = envelope(n, 0.04, 0.35)
    return [s * e * 6 for s, e in zip(seg, env)]


def waves(seconds=10.0):
    """Surf: noise swelling and washing out, seamless over the loop."""
    rng = random.Random(3)
    n = int(seconds * RATE)
    base = lowpass(noise(n, rng), 900)
    hiss = highpass(lowpass(noise(n, rng), 5000), 1500)
    out = []
    for i in range(n):
        s = i / n
        swell = (0.55 + 0.45 * math.sin(2 * math.pi * 2 * s)) ** 2
        out.append(base[i] * 1.4 * swell + hiss[i] * (swell**3) * 0.9)
    return out


def wind(seconds=10.0):
    rng = random.Random(4)
    n = int(seconds * RATE)
    base = resonator(noise(n, rng), 450, 250)
    return [b * (0.5 + 0.5 * math.sin(2 * math.pi * 3 * i / n)) ** 1.5 * 5 for i, b in enumerate(base)]


def nature(seconds=10.0):
    """Breeze and birdsong."""
    rng = random.Random(5)
    out = wind(seconds)
    out = [x * 0.5 for x in out]
    for _ in range(26):
        start = rng.uniform(0, seconds - 0.6)
        f = rng.uniform(2200, 4200)
        for chirp in range(rng.randint(1, 4)):
            n = int(0.07 * RATE)
            s0 = int((start + chirp * 0.1) * RATE)
            sweep = rng.uniform(-900, 1200)
            phase = 0.0
            for k in range(n):
                phase += (f + sweep * k / n) / RATE
                env = math.sin(math.pi * k / n)
                if s0 + k < len(out):
                    out[s0 + k] += math.sin(2 * math.pi * phase) * env * 0.35
    return out


def drips(seconds=8.0):
    rng = random.Random(6)
    out = [0.0] * int(seconds * RATE)
    t = 0.3
    while t < seconds - 0.5:
        f = rng.uniform(900, 2100)
        n = int(0.35 * RATE)
        s0 = int(t * RATE)
        for k in range(n):
            if s0 + k < len(out):
                out[s0 + k] += math.sin(2 * math.pi * f * (1 + 0.4 * (1 - k / n)) * k / RATE) * math.exp(-k / (0.04 * RATE)) * 0.8
        t += rng.uniform(0.5, 1.8)
    return out


def footstep():
    rng = random.Random(8)
    n = int(0.22 * RATE)
    body = lowpass(noise(n, rng), 600)
    thud = sine(70, 0.22)
    return [(b * 2 + t) * math.exp(-k / (0.03 * RATE)) for k, (b, t) in enumerate(zip(body, thud))]


def gunshot():
    rng = random.Random(9)
    n = int(0.9 * RATE)
    crack = highpass(noise(n, rng), 800)
    boom = lowpass(noise(n, rng), 250)
    return [crack[k] * math.exp(-k / (0.02 * RATE)) + boom[k] * 3 * math.exp(-k / (0.18 * RATE)) for k in range(n)]


def explosion():
    rng = random.Random(10)
    n = int(2.4 * RATE)
    boom = lowpass(noise(n, rng), 300)
    crack = lowpass(noise(n, rng), 2500)
    return [boom[k] * 4 * math.exp(-k / (0.5 * RATE)) + crack[k] * math.exp(-k / (0.08 * RATE)) for k in range(n)]


def beep():
    n = int(0.16 * RATE)
    env = envelope(n, 0.005, 0.03)
    return [math.sin(2 * math.pi * 1800 * k / RATE) * env[k] for k in range(n)]


def horn():
    """A two-tone car horn, 1.2 s, loopable-ish."""
    n = int(1.2 * RATE)
    env = envelope(n, 0.03, 0.1)
    out = []
    for k in range(n):
        a = (1 if math.sin(2 * math.pi * 440 * k / RATE) > 0 else -1) * 0.5 + math.sin(2 * math.pi * 440 * k / RATE)
        b = (1 if math.sin(2 * math.pi * 554 * k / RATE) > 0 else -1) * 0.5 + math.sin(2 * math.pi * 554 * k / RATE)
        out.append((a + b) * env[k])
    return lowpass(out, 3500)


def engine(seconds=2.0):
    rng = random.Random(11)
    n = int(seconds * RATE)
    rumble = lowpass(noise(n, rng), 200)
    out = []
    for k in range(n):
        saw = ((50 * k / RATE) % 1.0) * 2 - 1
        pulse = 0.6 + 0.4 * math.sin(2 * math.pi * 25 * k / RATE)
        out.append(saw * pulse * 0.6 + rumble[k] * 2)
    return lowpass(out, 900)


def knock():
    rng = random.Random(12)
    out = [0.0] * int(0.6 * RATE)
    for t in (0.0, 0.18, 0.36):
        n = int(0.12 * RATE)
        body = lowpass(noise(n, rng), 500)
        s0 = int(t * RATE)
        for k in range(n):
            out[s0 + k] += (body[k] * 3 + math.sin(2 * math.pi * 110 * k / RATE)) * math.exp(-k / (0.025 * RATE))
    return out


def static(seconds=3.0):
    rng = random.Random(13)
    n = int(seconds * RATE)
    xs = highpass(noise(n, rng), 1200)
    return [x * (0.5 + 0.5 * math.sin(2 * math.pi * 7 * i / n) ** 2) for i, x in enumerate(xs)]


def door():
    rng = random.Random(14)
    n = int(0.5 * RATE)
    body = lowpass(noise(n, rng), 400)
    return [(b * 3 + math.sin(2 * math.pi * 60 * k / RATE)) * math.exp(-k / (0.07 * RATE)) for k, b in enumerate(body)]


SOUNDS = {
    "radio_music": radio_music,
    "voice": voice,
    "shout": shout,
    "waves": waves,
    "wind": wind,
    "nature": nature,
    "drips": drips,
    "footstep": footstep,
    "gunshot": gunshot,
    "explosion": explosion,
    "beep": beep,
    "horn": horn,
    "engine": engine,
    "knock": knock,
    "static": static,
    "door": door,
}

# Which example gets which sounds.
EXAMPLES = {
    "audio-house": ["radio_music", "voice", "knock", "door", "static", "beep", "wind"],
    "audio-radar": ["footstep", "gunshot", "beep", "explosion", "engine", "voice"],
    "audio-cave": ["voice", "drips", "nature", "waves", "wind", "footstep", "radio_music"],
    "audio-road": ["engine", "horn", "shout", "wind", "radio_music"],
}

if __name__ == "__main__":
    cache = {}
    for example, names in EXAMPLES.items():
        for name in names:
            if name not in cache:
                cache[name] = SOUNDS[name]()
            write(os.path.join(ROOT, example, "assets", "sounds", name + ".wav"), cache[name])
            print(example, name, round(len(cache[name]) / RATE, 2), "s")
