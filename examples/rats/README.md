# rats

A swarm of rats in a walled maze, all heading for a piece of cheese: crowd navigation with a flow field.

One search from the cheese outward (`new FlowField(grid, [cheese])`) fills every cell of a `NavGrid` with its
distance to the cheese and the direction that way. Each rat just reads the field where it stands, so hundreds of
rats cost one search, not one A* each. A `Swarm` moves them along the field with a little separation (so they
spread through the gaps instead of piling up) and slides them along walls.

## Controls

| Key | |
|---|---|
| Click the floor | Move the cheese (the field is rebuilt) |
| B | Cycle the gate in the middle wall: open, half height, shut |
| P | With a half-height gate: pile up and climb (on) or just queue (off) |
| G | Show the flow field as arrows (drawn with `Gizmos`) |
| Space | Add 50 rats (up to 800) |
| A / D (or arrows) | Orbit; W / S or the wheel zoom |

Rats that reach the cheese stop and nibble.

## The gate

The gate is the only way through the middle wall, so the flow field always routes through it: it is built on a
second grid where the gate never blocks, while the rats walk on a grid where a half-height or shut gate is solid.
The rats therefore always head for the gate and press against it when it is closed.

- **Open:** they stream through.
- **Half height, pile up on (P):** rats pressed against the gate heap into a ramp (each lifted by how many are in
  front of it). Once five are pressing on it the one on top scrambles over, nose up, about four a second, and
  carries on to the cheese on the far side. The overlay counts how many have climbed over.
- **Half height, pile up off:** they queue at the low wall and stay there.
- **Shut:** a full wall nobody can climb: they pile up against it, flat.

Closing the gate on a rat mid-crossing is safe: `Swarm.step` moves any agent standing in a blocked cell onto the
nearest free one (`AI-48`), so nothing is left stuck inside the gate. A climbing rat is drawn by the script along an
arc and put down on the far side with `crowd.setPosition`.

APIs used: `NavGrid`, `FlowField` (`direction`, `distance`, `rebuild`), `Swarm` (`add`, `step`, `position`,
`velocity`, `count`), `Gizmos`, `GameObject` / `MeshRenderer` / `Light` / `Camera`, and a React overlay.

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/rats
```

```bash
cargo run --release -p platypus_app -- examples/rats
```

## How it works

The maze is a list of wall blocks (inclusive cell ranges) that is both blocked on the grid and drawn as boxes. The
grid is 40 x 24 cells of half a metre, one cell thick, so the flow stays on the floor. The rats are drawn as a few
primitives each, turned to face their velocity, with a bob while they run.
