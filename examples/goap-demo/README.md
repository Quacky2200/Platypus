# goap-demo

A villager in a small 3D village, planned by either of two AI planners, one button apart: **GOAP** and **HTN**.
Pick a goal and the planner works out the steps; the villager then walks them out, place by place.

- **Goals:** light a fire, earn coins, get some rest (which needs a fire first).
- **GOAP** (`Goap.plan`) searches for the *cheapest* list of actions from the facts that are true now.
- **HTN** (`Htn.plan`) follows *recipes the author wrote*. A compound task such as `GetWood` has methods tried in
  order (use the wood you have, spend a coin, chop it), each breaking it into subtasks; the first plan the order
  allows wins, and the overlay shows how the recipes broke the goal down ("How the recipes broke it down").
- **Where they differ:** start with a coin (the Purse button) and ask for a fire. GOAP finds chopping a little
  cheaper (abstract cost 12 against 13) and goes to the shed. This HTN's author put "spend a coin" first, so it
  buys the wood at the market. Neither is wrong: the costs are numbers the author chose, and the recipes are an
  order the author chose. (In walking time, buying is shorter here, which the cost numbers don't know.)
- **Walking** is A* (`NavGrid`) over the village: buildings, trees and the stall block cells, grass costs 2.5 to
  cross and the dirt roads cost 1, so the villager goes round buildings and keeps to the roads unless the grass is
  much shorter (the shed to the forest goes back through the crossroads). The white line on the ground is the
  route; the Route button hides it.
- **What you see:** the villager swings arms and legs to walk, carries the axe, a log on the back or a purse while
  those facts are true, chops, buys and sells with an outstretched arm, lights the fire at the camp (flames and a
  light) and lies down to sleep at home.
- **Controls:** the planner, goal, purse and route buttons in the overlay; A / D (or the arrows) orbit, W / S or
  the wheel zoom.

APIs used: `Goap`, `Htn` (`addPrimitive`, `addCompound`, `addMethod`, `plan`, `canPlan`, `methods`), `NavGrid`
(`setBoxBlocked`, `setWeight`, `findPath`), `Gizmos`, `GameObject` / `MeshRenderer` / `Light` / `TextMesh` /
`Camera`, and a React overlay (`docs/platypus-api.md`, "Bot AI").

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/goap-demo
```

```bash
cargo run --release -p platypus_app -- examples/goap-demo
```

## How it works

Facts are strings (`"hasAxe"`, `"at:forest"`); `"!hasAxe"` means false. Both planners get the same steps (`go:<place>`
costing 2, `takeAxe`, `chopWood`, ...). The HTN adds compound tasks: `At:<place>` (nothing if already there, else
walk), `GetAxe`, `GetWood`, and one per goal (`LightFire`, `EarnCoins`, `Rest`). The villager's facts live in module
state; each frame the `Village` component asks the chosen planner for a plan, walks toward the next step's place
along an A* route or spends a beat on the step, and applies the step's effects when it is done, which changes the
facts and so the plan.

Because the plan is recomputed every frame, each HTN recipe starts with an "already done" method (`LightFire:
already lit`, `Rest: already rested`): GOAP knows it is finished when its goal is met, but an HTN task with no such
method would plan the whole job again.
