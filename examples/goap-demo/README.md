# goap-demo

A villager planned by GOAP (goal-oriented action planning). Pick a goal and the `Goap` planner finds the
cheapest list of actions from the facts that are true now; the villager then walks the plan out, place by place.

- **Goals:** light a fire, earn coins, get some rest (which needs a fire first).
- **Purse:** with a coin the villager buys firewood at the market when that is cheaper than fetching the axe,
  chopping and carrying the wood; with an empty purse it takes the axe from the shed and chops.
- The plan on the right updates as facts change; the first step is highlighted.

APIs used: `new Goap()`, `addAction(name, cost, preconditions, effects)`, `plan(facts, goal)`, `canPlan(...)`
(`docs/platypus-api.md`, "Bot AI"), and the React UI (`box`, `text`, `useState`, `useEffect`).

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/goap-demo
```

```bash
cargo run --release -p platypus_app -- examples/goap-demo
```

## How it works

Facts are strings (`"hasAxe"`, `"at:forest"`); `"!hasAxe"` means false. Walking is one `go:<place>` action per
place, costing 2 from anywhere. The component keeps the villager's facts in state, asks `brain.plan(facts, goal)`
every render, and a timer moves the villager toward the next action's place and applies the action's effects when
it arrives, which changes the facts and so the plan.
