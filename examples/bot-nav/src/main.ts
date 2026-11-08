// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A bot walks a 3D grid around a wall (A*), steered along the path. Prints its progress.
const grid = new NavGrid(new Vector3(20, 4, 20), new Vector3(0, 0, 0));
grid.setBoxBlocked(new Vector3(10, 0, 0), new Vector3(10.5, 3.9, 15), true);

const from = new Vector3(2.5, 0.5, 2.5);
const to = new Vector3(17.5, 0.5, 2.5);
const path = grid.findPath(from, to);
console.log("waypoints: " + path.length);

let position = from;
let velocity = new Vector3(0, 0, 0);
let next = 0;
const dt = 1 / 60;
for (let frame = 0; frame < 3000 && next < path.length; frame++) {
  const target = path[next];
  if (position.distance(target) < 0.3) {
    next++;
    continue;
  }
  const accel = Steering.seek(position, velocity, target, 4, 30);
  velocity = velocity.add(accel.scale(dt));
  position = position.add(velocity.scale(dt));
}
console.log("arrived: " + (position.distance(to) < 0.5));

// What to do once there: a GOAP planner picks the cheapest way to get a fire going.
const brain = new Goap();
brain.addAction("pickUpAxe", 1, ["!hasAxe"], ["hasAxe"]);
brain.addAction("chopTree", 2, ["hasAxe"], ["hasWood"]);
brain.addAction("buyWood", 10, ["hasCoins"], ["hasWood"]);
brain.addAction("buildFire", 1, ["hasWood"], ["fire", "!hasWood"]);
console.log("plan: " + brain.plan(["hasCoins"], ["fire"]).join(" > "));
