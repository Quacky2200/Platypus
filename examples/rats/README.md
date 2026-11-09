# rats

A swarm of rats in a walled maze, all heading for a piece of cheese: crowd navigation with a flow field.

One search from the cheese outward (`new FlowField(grid, [cheese])`) fills every cell of a `NavGrid` with its
distance to the cheese and the direction that way. Each rat just reads the field where it stands, so hundreds of
rats cost one search, not one A* each. A `Crowd` moves them along the field with a little separation (so they
spread through the gaps instead of piling up) and slides them along walls.

## Controls

| Key | |
|---|---|
| Click the floor | Move the cheese (the field is rebuilt) |
| B | Shut or open the gate in the middle wall: the field is rebuilt, and the rats take the long way round |
| G | Show the flow field as arrows (drawn with `Gizmos`) |
| Space | Add 50 rats (up to 800) |
| A / D (or arrows) | Orbit; W / S or the wheel zoom |

Rats that reach the cheese stop and nibble.

APIs used: `NavGrid`, `FlowField` (`direction`, `distance`, `rebuild`), `Crowd` (`add`, `step`, `position`,
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
