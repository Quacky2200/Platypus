// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// GOAP (goal-oriented action planning): a villager is given a goal, `Goap.plan` finds the cheapest
// list of actions from the facts that are true now, and the villager walks the plan out. Change
// the goal or the starting facts and the plan changes: with coins in the purse the villager
// buys firewood at the market when that is cheaper than fetching an axe and chopping.

type Place = "shed" | "forest" | "market" | "camp" | "home";

const PLACES: Record<Place, { x: number; y: number; label: string; color: string }> = {
  shed: { x: 90, y: 90, label: "Shed", color: "#a77b4a" },
  forest: { x: 470, y: 70, label: "Forest", color: "#3f8f4f" },
  market: { x: 90, y: 330, label: "Market", color: "#c9a13b" },
  camp: { x: 470, y: 330, label: "Campsite", color: "#d9643a" },
  home: { x: 270, y: 200, label: "Home", color: "#5b78c7" },
};
const ORDER: Place[] = ["shed", "forest", "market", "camp", "home"];

// Action name -> what the villager does once it runs. `go:<place>` actions move.
const brain = new Goap();
for (const p of ORDER) {
  // Walking is possible from anywhere but where you are.
  brain.addAction(
    "go:" + p,
    2,
    ["!at:" + p],
    ["at:" + p, ...ORDER.filter((o) => o !== p).map((o) => "!at:" + o)],
  );
}
brain.addAction("takeAxe", 1, ["at:shed", "!hasAxe"], ["hasAxe"]);
brain.addAction("chopWood", 3, ["at:forest", "hasAxe"], ["hasWood"]);
brain.addAction("buyWood", 5, ["at:market", "hasCoins"], ["hasWood", "!hasCoins"]);
brain.addAction("sellWood", 1, ["at:market", "hasWood"], ["hasCoins", "!hasWood"]);
brain.addAction("buildFire", 2, ["at:camp", "hasWood"], ["fire", "!hasWood"]);
brain.addAction("sleep", 2, ["at:home", "fire"], ["rested"]);

type Goal = { label: string; literals: string[] };
const GOALS: Goal[] = [
  { label: "Light a fire", literals: ["fire"] },
  { label: "Earn coins", literals: ["hasCoins"] },
  { label: "Get some rest", literals: ["rested"] },
];

const START = ["at:home"];

function describe(action: string): string {
  if (action.startsWith("go:")) return "Walk to " + PLACES[action.slice(3) as Place].label;
  const names: Record<string, string> = {
    takeAxe: "Take the axe",
    chopWood: "Chop wood",
    buyWood: "Buy wood",
    sellWood: "Sell wood",
    buildFire: "Build a fire",
    sleep: "Sleep",
  };
  return names[action] ?? action;
}

/** The facts after an action, by running the same literals the planner was given. */
function applyAction(facts: string[], action: string): string[] {
  const set = new Set(facts);
  if (action.startsWith("go:")) {
    for (const p of ORDER) set.delete("at:" + p);
    set.add("at:" + action.slice(3));
    return [...set];
  }
  const effects: Record<string, string[]> = {
    takeAxe: ["hasAxe"],
    chopWood: ["hasWood"],
    buyWood: ["hasWood", "!hasCoins"],
    sellWood: ["hasCoins", "!hasWood"],
    buildFire: ["fire", "!hasWood"],
    sleep: ["rested"],
  };
  for (const e of effects[action] ?? []) {
    if (e.startsWith("!")) set.delete(e.slice(1));
    else set.add(e);
  }
  return [...set];
}

function placeOf(facts: string[]): Place {
  return ORDER.find((p) => facts.includes("at:" + p)) ?? "home";
}

function Button(props: { label: string; active?: boolean; onPress: () => void }) {
  return (
    <box
      tabIndex={0}
      onClick={props.onPress}
      style={{
        padding: "8px 14px",
        borderRadius: 8,
        backgroundColor: props.active ? "#5ec8ff" : "#2a3446",
        cursor: "pointer",
      }}
    >
      <text style={{ color: props.active ? "#0b1220" : "#e8eefc", fontSize: 15 }}>{props.label}</text>
    </box>
  );
}

function App() {
  const [coins, setCoins] = useState(false);
  const [goalIndex, setGoalIndex] = useState(0);
  const [facts, setFacts] = useState<string[]>(START);
  const [pos, setPos] = useState({ x: PLACES.home.x, y: PLACES.home.y });
  const [step, setStep] = useState(0);

  const goal = GOALS[goalIndex] ?? GOALS[0]!;
  const plan = brain.plan(facts, goal.literals);
  const reachable = brain.canPlan(facts, goal.literals);

  const restart = (withCoins: boolean, index: number) => {
    const f = withCoins ? [...START, "hasCoins"] : START;
    setCoins(withCoins);
    setGoalIndex(index);
    setFacts(f);
    setPos({ x: PLACES.home.x, y: PLACES.home.y });
    setStep(0);
  };

  // Walk toward the place the next action is at; run the action on arrival.
  useEffect(() => {
    const id = setInterval(() => {
      const next = plan[0];
      if (next === undefined) return;
      const target = next.startsWith("go:") ? PLACES[next.slice(3) as Place] : PLACES[placeOf(facts)];
      const dx = target.x - pos.x;
      const dy = target.y - pos.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist > 4) {
        setPos({ x: pos.x + (dx / dist) * 4, y: pos.y + (dy / dist) * 4 });
      } else {
        setFacts(applyAction(facts, next));
        setStep(step + 1);
      }
    }, next_delay(plan[0]));
    return () => clearInterval(id);
  }, [facts, pos, step, goalIndex, coins]);

  return (
    <box style={{ flexDirection: "row", gap: 20, padding: 20, backgroundColor: "#111827", width: "100%", height: "100%" }}>
      <box style={{ width: 600, height: 400, position: "relative", backgroundColor: "#1b2433", borderRadius: 14 }}>
        {ORDER.map((p) => (
          <box
            key={p}
            style={{
              position: "absolute",
              left: PLACES[p].x - 40,
              top: PLACES[p].y - 24,
              width: 80,
              height: 48,
              borderRadius: 10,
              backgroundColor: PLACES[p].color,
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <text style={{ color: "#fff", fontSize: 14, fontWeight: 700 }}>{PLACES[p].label}</text>
          </box>
        ))}
        <box
          style={{
            position: "absolute",
            left: pos.x - 10,
            top: pos.y - 10,
            width: 20,
            height: 20,
            borderRadius: "50%",
            backgroundColor: "#fff",
            border: "3px solid #111827",
          }}
        />
      </box>

      <box style={{ flexDirection: "column", gap: 12, width: 290 }}>
        <text style={{ color: "#e8eefc", fontSize: 22, fontWeight: 800 }}>GOAP villager</text>
        <text style={{ color: "#9fb0cc", fontSize: 13 }}>Goal</text>
        <box style={{ flexDirection: "column", gap: 6 }}>
          {GOALS.map((g, i) => (
            <Button key={g.label} label={g.label} active={i === goalIndex} onPress={() => restart(coins, i)} />
          ))}
        </box>
        <Button label={coins ? "Purse: has a coin" : "Purse: empty"} active={coins} onPress={() => restart(!coins, goalIndex)} />
        <text style={{ color: "#9fb0cc", fontSize: 13 }}>
          {reachable ? (plan.length === 0 ? "Done." : "Plan (" + plan.length + " steps left)") : "No plan reaches this goal."}
        </text>
        {plan.map((a, i) => (
          <text key={a + i} style={{ color: i === 0 ? "#5ec8ff" : "#e8eefc", fontSize: 15 }}>
            {i + 1}. {describe(a)}
          </text>
        ))}
        <text style={{ color: "#6b7a99", fontSize: 12 }}>Facts: {facts.filter((f) => !f.startsWith("at:")).join(", ") || "none"}</text>
      </box>
    </box>
  );
}

/** Walking is quick, doing things takes a beat. */
function next_delay(action: string | undefined): number {
  return action !== undefined && !action.startsWith("go:") ? 500 : 16;
}

createRoot().render(<App />);
