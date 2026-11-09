# wildlife

A small 3D ecosystem of rabbits and foxes on a meadow with a pond and rocks, each animal planned by GOAP.

- **Look:** a walled meadow of grass with a stone-edged pond, boulders and tufts of grass that are grazed down and
  grow back, trees round the outside. Rabbits hop (a bounce per hop, higher when they run), foxes lope; heads go
  down to graze or drink, animals squat to sleep and lunge when fighting. Young animals are smaller.
- **Needs:** hunger, thirst, tiredness and libido rise over time; health drops when hunger or thirst is full. Only
  well-fed animals breed.
- **Each tick** an animal turns the world around it into facts (`hungry`, `foodKnown`, `threatened`, `rivalNear`,
  `atWater`, ...), picks its most urgent need as the goal, asks its species' `Goap` brain for the cheapest plan
  and does the first step. Rabbits graze, drink, sleep, flee foxes, fight rival males and mate; foxes hunt
  rabbits, drink, sleep, fight rival males and mate. A male with a rival nearby plans `goRival, fight, goMate,
  mate`; without one it just plans `goMate, mate`.
- **Walking** goes through a `NavGrid` (A*, smoothed) around the pond and the rocks, steered with
  `Steering.seek` plus `Steering.separation`.
- A species that dies out is topped up now and then by newcomers from the edge (every 30 s).

## Controls

| Key | |
|---|---|
| Tab (or I) | Stats overlay: the selected animal's needs, goal and plan, births and deaths, the event log |
| N | Select the next animal (a white ring marks it) |
| F | Camera follows the selected animal |
| A / D (or arrows) | Orbit; W / S zoom |
| T | Simulation speed x1, x2, x4 |
| R | Restart |

The game logs births, kills and deaths, and a status line every 10 simulated seconds, to `Player.log`.

APIs used: `Goap`, `NavGrid`, `Steering`, `Rng` (seeded), `GameObject` / `MeshRenderer` / `Light` / `Camera`, and a
React overlay.

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/wildlife
```

```bash
cargo run --release -p platypus_app -- examples/wildlife
```

`platypus-app examples/wildlife --headless --tick-rate 1000 --frames 130000` runs the simulation with no window
(it follows the wall clock, so it takes real time) and prints the log.

## Notes

The brains share one action list built by `makeBrain`; only what "food" means differs (`graze` against `hunt`).
Facts are recomputed every tick, so a plan is dropped the moment the world changes under it (the prey runs off,
the rival backs down). The simulation works in a 600 x 400 plane of pixels; the 3D view maps it to metres
(`M = 0.05`) and the world is right-handed with forward = -z.
