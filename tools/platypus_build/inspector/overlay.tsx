// The UI inspector's overlay: added to a program by `platypus-build build --add-ui-inspector`
// (appended to the bundle, mounted in its own root by `createRoot(9000001)`, the host's inspector
// target). Names start with `UiInsp` so they never meet a game's. F12 turns it on and off, F9 pins
// the node under the pointer (and unpins). While on, the node under the pointer is outlined
// (padding box green, border box blue) with a label, and a card lists the CSS rules that match
// the pinned (else hovered) node, overridden declarations struck through.
// Design: docs/research/platypus-ui-inspector.md

const UiInspBlue = "rgba(80, 160, 255, 0.9)";
const UiInspFill = "rgba(80, 160, 255, 0.18)";
const UiInspGreen = "rgba(110, 200, 120, 0.28)";

function UiInspName(id: number): string {
  const eid = UiInspector.elementId(id);
  const cls = UiInspector.className(id);
  let s = UiInspector.kind(id);
  if (eid !== "") s += "#" + eid;
  if (cls !== "") s += "." + cls.split(" ").join(".");
  return s;
}

function UiInspNum(n: number): string {
  return String(Math.round(n * 10) / 10);
}

function UiInspOutline(props: { id: number }) {
  const id = props.id;
  const x = UiInspector.x(id);
  const y = UiInspector.y(id);
  const w = UiInspector.width(id);
  const h = UiInspector.height(id);
  const label = UiInspName(id) + "  " + UiInspNum(w) + " × " + UiInspNum(h);
  return (
    <box style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      <box
        style={{
          position: "absolute",
          left: UiInspector.paddingBox(id, 0),
          top: UiInspector.paddingBox(id, 1),
          width: UiInspector.paddingBox(id, 2),
          height: UiInspector.paddingBox(id, 3),
          backgroundColor: UiInspGreen,
        }}
      />
      <box
        style={{
          position: "absolute",
          left: x,
          top: y,
          width: w,
          height: h,
          backgroundColor: UiInspFill,
          border: "1px solid " + UiInspBlue,
        }}
      />
      <box
        style={{
          position: "absolute",
          left: Math.max(0, x),
          top: y > 22 ? y - 22 : y + h + 2,
          padding: "2px 6px",
          backgroundColor: "rgba(20, 24, 32, 0.92)",
          borderRadius: 3,
        }}
      >
        <text style={{ color: "#dfe7f5", fontSize: 12 }}>{label}</text>
      </box>
    </box>
  );
}

function UiInspRule(props: { id: number; rule: number }) {
  const { id, rule } = props;
  const rows: ReactNode[] = [];
  const n = UiInspector.declCount(id, rule);
  for (let i = 0; i < n; i++) {
    const applied = UiInspector.declApplied(id, rule, i);
    const text =
      UiInspector.declProp(id, rule, i) +
      ": " +
      UiInspector.declValue(id, rule, i) +
      (UiInspector.declImportant(id, rule, i) ? " !important" : "");
    rows.push(
      <text
        key={i}
        style={{
          color: applied ? "#dfe7f5" : "#7d8794",
          fontSize: 12,
          textDecoration: applied ? "none" : "line-through",
          paddingLeft: 12,
        }}
      >
        {text}
      </text>,
    );
  }
  return (
    <box style={{ flexDirection: "column", marginBottom: 4 }}>
      <text style={{ color: "#f0b86e", fontSize: 12 }}>{UiInspector.ruleSelector(id, rule)}</text>
      {rows}
    </box>
  );
}

function UiInspCard(props: { id: number; pinned: boolean }) {
  const id = props.id;
  const rules: ReactNode[] = [];
  // Highest priority first, like browser devtools.
  for (let i = UiInspector.ruleCount(id) - 1; i >= 0; i--) {
    rules.push(<UiInspRule key={i} id={id} rule={i} />);
  }
  const inline: ReactNode[] = [];
  for (let i = 0; i < UiInspector.inlineCount(id); i++) {
    const applied = UiInspector.inlineApplied(id, i);
    inline.push(
      <text
        key={i}
        style={{
          color: applied ? "#dfe7f5" : "#7d8794",
          fontSize: 12,
          textDecoration: applied ? "none" : "line-through",
          paddingLeft: 12,
        }}
      >
        {UiInspector.inlineProp(id, i) + ": " + UiInspector.inlineValue(id, i)}
      </text>,
    );
  }
  return (
    <box
      style={{
        position: "fixed",
        left: 8,
        bottom: 8,
        maxHeight: "50%",
        minWidth: 260,
        maxWidth: 420,
        padding: 8,
        flexDirection: "column",
        overflow: "hidden",
        backgroundColor: "rgba(20, 24, 32, 0.92)",
        border: "1px solid rgba(255, 255, 255, 0.15)",
        borderRadius: 4,
        pointerEvents: "none",
      }}
    >
      <text style={{ color: "#8fc7ff", fontSize: 13, marginBottom: 4 }}>
        {(props.pinned ? "pinned  " : "") + UiInspName(id)}
      </text>
      {inline.length > 0 ? <text style={{ color: "#f0b86e", fontSize: 12 }}>element.style</text> : null}
      {inline}
      {rules}
    </box>
  );
}

function UiInspOverlay() {
  const [on, setOn] = useState(false);
  const [, setTick] = useState(0);
  useEffect(() => {
    return input.on("keydown", (e) => {
      if (e.code === "F12") setOn((v) => !v);
      else if (e.code === "F9") {
        UiInspector.select(UiInspector.selected() !== 0 ? 0 : UiInspector.hovered());
      }
    });
  }, []);
  // The host publishes once a frame; redraw at about the same pace while on.
  useEffect(() => {
    if (!on) return undefined;
    const h = setInterval(() => setTick((t) => t + 1), 33);
    return () => clearInterval(h);
  }, [on]);
  if (!on) return null;
  const hovered = UiInspector.hovered();
  const pinned = UiInspector.selected();
  const shown = pinned !== 0 ? pinned : hovered;
  return (
    <box style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      {hovered !== 0 ? <UiInspOutline id={hovered} /> : null}
      {shown !== 0 ? <UiInspCard id={shown} pinned={pinned !== 0} /> : null}
    </box>
  );
}

if (UiInspector.enabled()) {
  createRoot(9000001).render(<UiInspOverlay />);
}
