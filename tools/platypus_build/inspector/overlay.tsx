// The UI inspector's overlay: added to a program by `platypus-build build --add-ui-inspector`
// (appended to the bundle after `values.ts`, `theme.ts` and `shared.tsx`, mounted in its own root by
// `createRoot(9000001)`, the host's inspector target). Names start with `UiInsp` so they never meet a game's.
//   F12  turns the inspector on and off.
//   F9   pins the node under the pointer (and unpins).
//   F7   pick mode: the next mouse click is kept from the game and pins the node under it.
//   F8   edit mode: pins the hovered node if none is pinned and makes the card editable. A value typed into
//        a rule's declaration (or into `element.style`) sets an inline override on that node (Enter or
//        leaving the field commits, Escape puts it back, an empty value removes the override); the last
//        row adds a property.
// What it draws: the node under the pointer with its box model (margin orange, border yellow, padding
// green, content blue) and a label; a pinned node as marching ants (moving dashes, no fill); while
// editing, nothing over the pinned node (it would hide the colours being changed) except the box model
// of a hovered margin / padding line and the marching ants round whatever a hovered rule or row is about.
// The card lists the CSS rules that match the pinned (else hovered) node; its pill switches the theme.
// Design: docs/research/platypus-ui-inspector.md

/** A layout number (`marginTop`, ...) of a node, 0 when it has none. */
function UiInspPx(id: number, name: string): number {
  const n = parseFloat(UiInspector.layout(id, name));
  return Number.isNaN(n) ? 0 : n;
}

/** One thing drawn at a place: absolutely positioned, taking no pointer. */
function UiInspAt(props: { cls: string; x: number; y: number; w: number; h: number }) {
  return <box className={props.cls} style={{ position: "absolute", left: props.x, top: props.y, width: Math.max(0, props.w), height: Math.max(0, props.h), pointerEvents: "none" }} />;
}

/** The label that names a node and its size, above it (below when there is no room above). */
function UiInspLabel(props: { id: number }) {
  const id = props.id;
  const y = UiInspector.y(id);
  const label = UiInspName(id) + "  " + UiInspNum(UiInspector.width(id)) + " × " + UiInspNum(UiInspector.height(id));
  return (
    <box className="uii-tip" style={{ position: "absolute", left: Math.max(0, UiInspector.x(id)), top: y > 26 ? y - 26 : y + UiInspector.height(id) + 3, pointerEvents: "none" }}>
      <text className="uii-text">{label}</text>
    </box>
  );
}

/** A node's box model: margin, border, padding and content, each in its colour. */
function UiInspBoxModel(props: { id: number; label: boolean }) {
  const id = props.id;
  const x = UiInspector.x(id);
  const y = UiInspector.y(id);
  const w = UiInspector.width(id);
  const h = UiInspector.height(id);
  const mt = UiInspPx(id, "marginTop");
  const mr = UiInspPx(id, "marginRight");
  const mb = UiInspPx(id, "marginBottom");
  const ml = UiInspPx(id, "marginLeft");
  const pt = UiInspPx(id, "paddingTop");
  const pr = UiInspPx(id, "paddingRight");
  const pb = UiInspPx(id, "paddingBottom");
  const pl = UiInspPx(id, "paddingLeft");
  const px = UiInspector.paddingBox(id, 0);
  const py = UiInspector.paddingBox(id, 1);
  const pw = UiInspector.paddingBox(id, 2);
  const ph = UiInspector.paddingBox(id, 3);
  return (
    <box style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      <UiInspAt cls="uii-bm-margin" x={x - ml} y={y - mt} w={w + ml + mr} h={h + mt + mb} />
      <UiInspAt cls="uii-bm-border" x={x} y={y} w={w} h={h} />
      <UiInspAt cls="uii-bm-padding" x={px} y={py} w={pw} h={ph} />
      <UiInspAt cls="uii-bm-content" x={px + pl} y={py + pt} w={pw - pl - pr} h={ph - pt - pb} />
      {props.label ? <UiInspLabel id={id} /> : null}
    </box>
  );
}

/** Marching ants round a box: moving dashes along its four edges, no fill. */
function UiInspAnts(props: { x: number; y: number; w: number; h: number }) {
  const { x, y, w, h } = props;
  return (
    <box style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      <UiInspAt cls="uii-ant-h" x={x} y={y} w={w} h={1} />
      <UiInspAt cls="uii-ant-h uii-ant-back" x={x} y={y + h - 1} w={w} h={1} />
      <UiInspAt cls="uii-ant-v uii-ant-back" x={x} y={y} w={1} h={h} />
      <UiInspAt cls="uii-ant-v" x={x + w - 1} y={y} w={1} h={h} />
    </box>
  );
}

/** The boxes the host says a hovered rule or row is about, as marching ants. */
function UiInspHighlights() {
  const n = UiInspector.highlightCount();
  const out: ReactNode[] = [];
  for (let i = 0; i < n; i++) {
    out.push(<UiInspAnts key={i} x={UiInspector.highlightRect(i, 0)} y={UiInspector.highlightRect(i, 1)} w={UiInspector.highlightRect(i, 2)} h={UiInspector.highlightRect(i, 3)} />);
  }
  return <>{out}</>;
}

function UiInspCard(props: { id: number; pinned: boolean; edit: boolean; theme: string; onTheme: (theme: string) => void }) {
  const { id, edit } = props;
  return (
    <box
      className="uii-card"
      style={{
        position: "fixed",
        left: 10,
        bottom: 10,
        maxHeight: "62%",
        minWidth: 280,
        maxWidth: 540,
        padding: 10,
        flexDirection: "column",
        overflow: edit ? "auto" : "hidden",
        pointerEvents: edit ? "auto" : "none",
      }}
    >
      <box className="uii-head">
        <text className="uii-title">{(edit ? "editing  " : props.pinned ? "pinned  " : "") + UiInspName(id)}</text>
        <box style={{ flexGrow: 1 }} />
        {edit ? <UiInspThemeChip theme={props.theme} onTheme={props.onTheme} /> : null}
      </box>
      <UiInspStyles id={id} edit={edit} />
      {edit ? <text className="uii-dim" style={{ marginTop: 6 }}>values you type set inline overrides · Enter commits · Up / Down step · F8 leaves</text> : null}
    </box>
  );
}

function UiInspOverlay() {
  const [on, setOn] = useState(false);
  const [edit, setEdit] = useState(false);
  const [picking, setPicking] = useState(false);
  const [theme, setTheme] = useState("");
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
  const editing = edit && pinned !== 0;
  const focus = UiInspector.boxFocus();
  return (
    <UiInspRoot theme={theme} style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      {picking ? (
        <box className="uii-tip" style={{ position: "fixed", top: 10, left: "50%" }}>
          <text className="uii-text">click an element to pin it · F7 cancels</text>
        </box>
      ) : null}
      {/* Pointing at the page: the box model of what is under the pointer. */}
      {!editing && hovered !== 0 && hovered !== pinned ? <UiInspBoxModel id={hovered} label={true} /> : null}
      {/* Pinned: moving dashes, so the colours under it stay visible. */}
      {!editing && pinned !== 0 ? <UiInspAnts x={UiInspector.x(pinned)} y={UiInspector.y(pinned)} w={UiInspector.width(pinned)} h={UiInspector.height(pinned)} /> : null}
      {/* Editing: nothing over the node, but the box model while a margin / padding line is hovered. */}
      {editing && focus !== 0 ? <UiInspBoxModel id={focus} label={false} /> : null}
      <UiInspHighlights />
      {shown !== 0 ? <UiInspCard id={shown} pinned={pinned !== 0} edit={editing} theme={theme} onTheme={setTheme} /> : null}
    </UiInspRoot>
  );
}

if (UiInspector.enabled()) {
  createRoot(9000001).render(<UiInspOverlay />);
}
