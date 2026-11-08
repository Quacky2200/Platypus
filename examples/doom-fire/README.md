# doom-fire

A port of the [Doom fire algorithm](https://github.com/filipedeschamps/doom-fire-algorithm) by Filipe Deschamps
(MIT, notice at the top of `src/main.tsx`), in two versions you switch between with **G**:

- **Particles** (default): a row of 56 `ParticleSystem`s. A column's source heat (0..36, kept and decayed like the
  original's source row) sets its emission rate and its particles' lifetime, so hot columns burn taller. A
  particle's colour over life (`addColorKey`) walks the original's 37-colour palette from white to black, and its
  size shrinks as it cools. They blend additively (each particle dim, so heat shows where they pile up) and leave a
  cone, so a flame fans out from its column.
- **Grid**: the algorithm cell for cell on a 256x144 grid. Each tick the heat grid becomes one `Texture.create`
  (point filtered, the palette converted to linear colours, the old texture destroyed) shown on a sprite that covers
  the view. An earlier version drew the grid as UI boxes with gradients (2,304 of them at 128x72); it dropped
  to 30 fps, while the texture at four times the cells runs at several hundred.

APIs used: `ParticleSystem` (`addColorKey`, `addSizeKey`, `setStartLifetimeRange`, `randomSeed`), `Camera`
(orthographic), `Texture.create`, `Sprite.create`, `SpriteRenderer`, `input.getKeyDown`, React UI for the text.

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/doom-fire
```

```bash
cargo run --release -p platypus_app -- examples/doom-fire
```

A screenshot run needs a fixed clock, or the fire has barely started: add `--frames 240 --fixed-dt 0.0166`.

## Controls

| Key | What |
|---|---|
| Space | Put the fire out, or light it again (the source row decays or recovers column by column) |
| G | Switch between particles and the grid |
| P | Grid pixel size: original (256x144 cells), double (128x72) or triple (85x48), cycling; the grid restarts cold and regrows |
| Up / Down | Grid flame height, as a fraction of the screen (starts at 0.35). Cooling is set from it, so it stays the same however many cells the grid has |

## Differences from the original

The grid is 144 rows, not hundreds, so a cell cools with a probability per row set from the flame height (instead of `decay & 1`), and moves one
cell either way rather than only left (moving only left leaves the right edge stale).
