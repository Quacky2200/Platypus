# goap-demo

A villager planned by GOAP (goal-oriented action planning), in a small 3D village. Pick a goal and the `Goap`
planner finds the cheapest list of actions from the facts that are true now; the villager then walks the plan
out, place by place.

- **Goals:** light a fire, earn coins, get some rest (which needs a fire first).
- **Purse:** with a coin the villager buys firewood at the market when that is cheaper than fetching the axe,
  chopping and carrying the wood; with an empty purse it takes the axe from the shed and chops.
- **What you see:** the villager swings arms and legs to walk, carries the axe, a log on the back or a purse while
  those facts are true, chops, buys and sells with an outstretched arm, lights the fire at the camp (flames and a
  light) and lies down to sleep at home. The plan in the overlay updates as the facts change; its first step is
  highlighted.
- **Controls:** the goal and purse buttons in the overlay; A / D (or the arrows) orbit, W / S or the wheel zoom.

APIs used: `new Goap()`, `addAction(name, cost, preconditions, effects)`, `plan(facts, goal)`, `canPlan(...)`
(`docs/platypus-api.md`, "Bot AI"), `GameObject` / `MeshRenderer` / `Light` / `TextMesh` / `Camera`, and a React
overlay.

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/goap-demo
```

```bash
cargo run --release -p platypus_app -- examples/goap-demo
```

## How it works

Facts are strings (`"hasAxe"`, `"at:forest"`); `"!hasAxe"` means false. Walking is one `go:<place>` action per
place, costing 2 from anywhere. The villager's facts live in module state; the `Village` component asks
`brain.plan(facts, goal)` every frame, walks toward the next action's place or spends a beat on the action, and
applies the action's effects when it is done, which changes the facts and so the plan. The overlay reads the same
state ten times a second.
