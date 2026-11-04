// The UI inspector's overlay: added to a program by `platypus-build build --add-ui-inspector`
// (appended to the bundle after `shared.tsx`, mounted in its own root by `createRoot(9000001)`,
// the host's inspector target). Names start with `UiInsp` so they never meet a game's.
//   F12  turns the inspector on and off.
//   F9   pins the node under the pointer (and unpins).
//   F7   pick mode: the next mouse click is kept from the game and pins the node under it.
//   F8   edit mode: pins the hovered node if none is pinned, outlines the pinned node and makes the
//        card editable. A value typed into a rule's declaration (or into `element.style`) sets an
//        inline override on that node (Enter or leaving the field commits, Escape puts it back, an
//        empty value removes the override); the last row adds a property.
// While on, the node under the pointer is outlined (padding box green, border box blue) with a
// label, a pinned node too, and the card lists the CSS rules that match the pinned (else hovered)
// node, overridden declarations struck through.
// Design: docs/research/platypus-ui-inspector.md

const UiInspBlue = "rgba(80, 160, 255, 0.9)";
const UiInspFill = "rgba(80, 160, 255, 0.18)";
const UiInspGreen = "rgba(110, 200, 120, 0.28)";

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

function UiInspCard(props: { id: number; pinned: boolean; edit: boolean }) {
  const { id, edit } = props;
  return (
    <box
      style={{
        position: "fixed",
        left: 8,
        bottom: 8,
        maxHeight: "60%",
        minWidth: 260,
        maxWidth: 440,
        padding: 8,
        flexDirection: "column",
        overflow: "hidden",
        backgroundColor: "rgba(20, 24, 32, 0.92)",
        border: "1px solid " + (edit ? UiInspBlue : "rgba(255, 255, 255, 0.15)"),
        borderRadius: 4,
        pointerEvents: edit ? "auto" : "none",
      }}
    >
      <text style={{ color: "#8fc7ff", fontSize: 13, marginBottom: 4 }}>
        {(edit ? "editing  " : props.pinned ? "pinned  " : "") + UiInspName(id)}
      </text>
      <UiInspStyles id={id} edit={edit} />
      {edit ? (
        <text style={{ color: "#7d8794", fontSize: 11, marginTop: 4 }}>
          values you type set inline overrides · Enter commits · F8 leaves
        </text>
      ) : null}
    </box>
  );
}

function UiInspOverlay() {
  const [on, setOn] = useState(false);
  const [edit, setEdit] = useState(false);
  const [picking, setPicking] = useState(false);
  const [, setTick] = useState(0);
  useEffect(() => {
    return input.on("keydown", (e) => {
      if (e.code === "F12") {
        setOn((v) => !v);
        setEdit(false);
        setPicking(false);
      } else if (e.code === "F7") {
        setEdit(false);
        setPicking((v) => !v);
      } else if (e.code === "F9") {
        UiInspector.select(UiInspector.selected() !== 0 ? 0 : UiInspector.hovered());
        setEdit(false);
      } else if (e.code === "F8") {
        if (UiInspector.selected() === 0) UiInspector.select(UiInspector.hovered());
        setEdit((v) => !v);
      }
    });
  }, []);
  // In pick mode the host keeps mouse presses from the game and tells us what was under one.
  useEffect(() => {
    UiInspector.pick(on && picking && !edit);
    return () => UiInspector.pick(false);
  }, [on, picking, edit]);
  // The host publishes once a frame; redraw at about the same pace while on.
  useEffect(() => {
    if (!on) return undefined;
    const h = setInterval(() => {
      const clicked = UiInspector.takeClick();
      if (clicked !== 0) {
        UiInspector.select(clicked);
        setPicking(false);
      }
      setTick((t) => t + 1);
    }, 33);
    return () => clearInterval(h);
  }, [on]);
  if (!on) return null;
  const hovered = UiInspector.hovered();
  const pinned = UiInspector.selected();
  const shown = pinned !== 0 ? pinned : hovered;
  // Editing outlines the pinned node only: the pointer is on the card, not on what is being edited.
  const hoverOutline = !(edit && pinned !== 0) && hovered !== 0 && hovered !== pinned;
  return (
    <box style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      {picking ? (
        <box
          style={{
            position: "fixed",
            top: 8,
            left: "50%",
            padding: "3px 10px",
            backgroundColor: "rgba(20, 24, 32, 0.92)",
            border: "1px solid " + UiInspBlue,
            borderRadius: 4,
          }}
        >
          <text style={{ color: "#dfe7f5", fontSize: 12 }}>click an element to pin it · F7 cancels</text>
        </box>
      ) : null}
      {hoverOutline ? <UiInspOutline id={hovered} /> : null}
      {pinned !== 0 ? <UiInspOutline id={pinned} /> : null}
      {shown !== 0 ? <UiInspCard id={shown} pinned={pinned !== 0} edit={edit && pinned !== 0} /> : null}
    </box>
  );
}

if (UiInspector.enabled()) {
  createRoot(9000001).render(<UiInspOverlay />);
}
