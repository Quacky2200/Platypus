// The UI inspector's overlay: added to a program by `platypus-build build --add-ui-inspector`
// (appended to the bundle after `values.ts` and `shared.tsx`, mounted in its own root by
// `createRoot(9000001)`, the host's inspector target). Names start with `UiInsp` so they never meet a game's.
//   F12  turns the inspector on and off; a small toolbar shows while it is on.
//   Inspect (F7)  the toolbar's button, as in a browser: move over the page to see each element's box
//        model (margin orange, border yellow, padding green, content blue) and name, and click one to
//        edit it. Esc cancels.
//   Editing  a card lists the element's classes, forced states and CSS rules as `selector { ... }`; every
//        value is a field (Enter commits, Escape reverts), and what you type sets an inline override on
//        the element. Nothing is drawn over the element while editing (it would hide the colours being
//        changed), except the box model of a hovered margin / padding / size line and marching ants round
//        the elements a hovered rule matches. The card's × closes it.
//   ?  the toolbar's button lists the shortcuts.
// The inspector's own UI is never picked or outlined (the host skips what is under a `uii` root).
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

// --- Toolbar and help ------------------------------------------------------------------------------

/** What the keys and mouse do, for the `?` panel. */
const UiInspHelp = [
  ["F12", "turn the inspector on or off"],
  ["F7", "inspect: click an element to edit it (Esc cancels)"],
  ["Enter / Esc", "commit / revert the value you are typing"],
  ["Up / Down", "add or take 1 in a number (Shift: 10, Alt: 0.1)"],
  ["Tab", "take the first completion"],
  ["swatch / curve / stops", "colour picker / timing curve / gradient editor"],
  ["flexbox", "a layout editor for a flex container"],
  ["class links", "click to take a class off, again to put it back; + adds one"],
  [":hover :active ...", "force a state on the element"],
  ["hover a rule", "outline the elements it matches"],
  ["hover margin / padding", "show the box model over the element"],
  ["NEW RULE", "type .a:hover { color: red } to add a real rule"],
];

/** The toolbar: Inspect (as in a browser), `?` for the shortcuts, the theme, and off. */
function UiInspToolbar(props: { picking: boolean; help: boolean; theme: string; onInspect: () => void; onHelp: () => void; onTheme: (theme: string) => void; onClose: () => void }) {
  return (
    <box className="uii-card" style={{ position: "fixed", top: 10, right: 10, padding: "6px 8px", flexDirection: "column", gap: 6, pointerEvents: "auto" }}>
      <box style={{ flexDirection: "row", gap: 6, alignItems: "center" }}>
        <UiInspChip label={props.picking ? "Inspecting… (Esc)" : "Inspect  F7"} on={props.picking} onClick={props.onInspect} />
        <UiInspChip label="?" on={props.help} onClick={props.onHelp} />
        <UiInspThemeChip theme={props.theme} onTheme={props.onTheme} />
        <UiInspX onClick={props.onClose} />
      </box>
      {props.help ? (
        <box style={{ flexDirection: "column", gap: 3, maxWidth: 380 }}>
          {UiInspHelp.map((row) => (
            <box key={row[0]} style={{ flexDirection: "row", gap: 8, alignItems: "flex-start" }}>
              <text className="uii-prop" style={{ width: 130 }}>{row[0]}</text>
              <text className="uii-text" style={{ maxWidth: 240 }}>{row[1]}</text>
            </box>
          ))}
        </box>
      ) : null}
    </box>
  );
}

/** The card of the element being edited: its name, a close button, and the style editor. */
function UiInspCard(props: { id: number; onClose: () => void }) {
  const id = props.id;
  return (
    <box
      className="uii-card"
      style={{
        position: "fixed",
        left: 10,
        bottom: 10,
        maxHeight: "74%",
        minWidth: 300,
        maxWidth: 560,
        padding: 10,
        flexDirection: "column",
        overflow: "auto",
        pointerEvents: "auto",
      }}
    >
      <box className="uii-head">
        <text className="uii-title">{UiInspName(id)}</text>
        <box style={{ flexGrow: 1 }} />
        <UiInspX onClick={props.onClose} />
      </box>
      <UiInspStyles id={id} edit={true} />
    </box>
  );
}

function UiInspOverlay() {
  const [on, setOn] = useState(false);
  const [picking, setPicking] = useState(false);
  const [help, setHelp] = useState(false);
  const [theme, setTheme] = useState("");
  const [, setTick] = useState(0);
  useEffect(() => {
    return input.on("keydown", (e) => {
      if (e.code === "F12") {
        setOn((v) => !v);
        setPicking(false);
        setHelp(false);
        UiInspector.select(0);
      } else if (e.code === "F7") {
        setOn(true);
        setPicking((v) => !v);
      } else if (e.code === "Escape") {
        setPicking(false);
      }
    });
  }, []);
  // While inspecting the host keeps mouse presses on the page from the game and says what was under one.
  useEffect(() => {
    UiInspector.pick(on && picking);
    return () => UiInspector.pick(false);
  }, [on, picking]);
  // The host publishes once a frame; redraw at about the same pace while on.
  useEffect(() => {
    if (!on) return undefined;
    const h = setInterval(() => {
      const clicked = UiInspector.takeClick();
      if (clicked !== 0) {
        // The element clicked while inspecting is edited at once.
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
  const editing = pinned !== 0 && !picking;
  const focus = UiInspector.boxFocus();
  return (
    <UiInspRoot theme={theme} style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      {/* Inspecting: the box model of what is under the pointer. */}
      {picking && hovered !== 0 ? <UiInspBoxModel id={hovered} label={true} /> : null}
      {/* Editing: nothing over the element, but the box model while a margin / padding line is hovered. */}
      {editing && focus !== 0 ? <UiInspBoxModel id={focus} label={false} /> : null}
      <UiInspHighlights />
      {editing ? <UiInspCard id={pinned} onClose={() => UiInspector.select(0)} /> : null}
      <UiInspToolbar
        picking={picking}
        help={help}
        theme={theme}
        onInspect={() => setPicking(!picking)}
        onHelp={() => setHelp(!help)}
        onTheme={setTheme}
        onClose={() => {
          setOn(false);
          setPicking(false);
          UiInspector.select(0);
        }}
      />
    </UiInspRoot>
  );
}

if (UiInspector.enabled()) {
  createRoot(9000001).render(<UiInspOverlay />);
}
