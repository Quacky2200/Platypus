// The UI inspector's overlay: added to a program by `platypus-build build --add-ui-inspector`
// (appended to the bundle, mounted in its own root by `createRoot(9000001)`, the host's inspector
// target). Names start with `UiInsp` so they never meet a game's.
//   F12  turns the inspector on and off.
//   F9   pins the node under the pointer (and unpins).
//   F8   edit mode: pins the hovered node if none is pinned, outlines the pinned node and makes the
//        card editable. A value typed into a rule's declaration (or into `element.style`) sets an
//        inline override on that node (Enter or leaving the field commits, Escape puts it back, an
//        empty value removes the override); the last row adds a property.
// While on, the node under the pointer is outlined (padding box green, border box blue) with a
// label, and the card lists the CSS rules that match the pinned (else hovered) node, overridden
// declarations struck through.
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

/** A text field that commits on Enter or when left; Escape puts the value back. */
function UiInspField(props: { value: string; placeholder?: string; width: number; onCommit: (s: string) => void }) {
  const [text, setText] = useState(props.value);
  const editing = useRef(false);
  useEffect(() => {
    if (!editing.current) setText(props.value);
  }, [props.value]);
  const commit = (): void => {
    editing.current = false;
    if (text !== props.value) props.onCommit(text);
  };
  return (
    <Input
      value={text}
      placeholder={props.placeholder}
      style={{
        width: props.width,
        fontSize: 12,
        color: "#dfe7f5",
        backgroundColor: "rgba(255, 255, 255, 0.08)",
        border: "1px solid rgba(255, 255, 255, 0.18)",
        borderRadius: 2,
        padding: "1px 4px",
      }}
      onChange={(e) => {
        editing.current = true;
        setText(e.value);
      }}
      onKeyDown={(e: PlatypusKeyboardEvent) => {
        if (e.key === "Enter") commit();
        else if (e.key === "Escape") {
          editing.current = false;
          setText(props.value);
        }
      }}
      onBlur={commit}
    />
  );
}

/** One `prop: value` line: plain text, or with a field for the value in edit mode. */
function UiInspLine(props: { id: number; prop: string; value: string; suffix: string; applied: boolean; edit: boolean; removable: boolean }) {
  const { id, prop, value, applied, edit } = props;
  if (!edit) {
    return (
      <text
        style={{
          color: applied ? "#dfe7f5" : "#7d8794",
          fontSize: 12,
          textDecoration: applied ? "none" : "line-through",
          paddingLeft: 12,
        }}
      >
        {prop + ": " + value + props.suffix}
      </text>
    );
  }
  return (
    <box style={{ flexDirection: "row", alignItems: "center", gap: 4, paddingLeft: 12, marginBottom: 2 }}>
      <text style={{ color: applied ? "#9cc4ff" : "#7d8794", fontSize: 12, textDecoration: applied ? "none" : "line-through" }}>
        {prop + ":"}
      </text>
      <UiInspField value={value} width={150} onCommit={(s) => UiInspector.setStyle(id, prop, s)} />
      {props.removable ? (
        <box
          style={{ cursor: "pointer", padding: "0 4px" }}
          onClick={() => UiInspector.setStyle(id, prop, "")}
        >
          <text style={{ color: "#e07a7a", fontSize: 12 }}>×</text>
        </box>
      ) : null}
    </box>
  );
}

function UiInspRule(props: { id: number; rule: number; edit: boolean }) {
  const { id, rule, edit } = props;
  const rows: ReactNode[] = [];
  const n = UiInspector.declCount(id, rule);
  for (let i = 0; i < n; i++) {
    rows.push(
      <UiInspLine
        key={i}
        id={id}
        prop={UiInspector.declProp(id, rule, i)}
        value={UiInspector.declValue(id, rule, i)}
        suffix={UiInspector.declImportant(id, rule, i) ? " !important" : ""}
        applied={UiInspector.declApplied(id, rule, i)}
        edit={edit}
        removable={false}
      />,
    );
  }
  return (
    <box style={{ flexDirection: "column", marginBottom: 4 }}>
      <text style={{ color: "#f0b86e", fontSize: 12 }}>{UiInspector.ruleSelector(id, rule)}</text>
      {rows}
    </box>
  );
}

/** The row that adds a property: a name, then a value committed with Enter. */
function UiInspAdd(props: { id: number }) {
  const [prop, setProp] = useState("");
  return (
    <box style={{ flexDirection: "row", alignItems: "center", gap: 4, paddingLeft: 12, marginTop: 4 }}>
      <UiInspField value={prop} placeholder="property" width={110} onCommit={(s) => setProp(s)} />
      <UiInspField
        value=""
        placeholder="value"
        width={110}
        onCommit={(s) => {
          if (prop !== "" && s !== "") {
            UiInspector.setStyle(props.id, prop, s);
            setProp("");
          }
        }}
      />
    </box>
  );
}

function UiInspCard(props: { id: number; pinned: boolean; edit: boolean }) {
  const { id, edit } = props;
  const rules: ReactNode[] = [];
  // Highest priority first, like browser devtools.
  for (let i = UiInspector.ruleCount(id) - 1; i >= 0; i--) {
    rules.push(<UiInspRule key={i} id={id} rule={i} edit={edit} />);
  }
  const inline: ReactNode[] = [];
  for (let i = 0; i < UiInspector.inlineCount(id); i++) {
    inline.push(
      <UiInspLine
        key={i}
        id={id}
        prop={UiInspector.inlineProp(id, i)}
        value={UiInspector.inlineValue(id, i)}
        suffix=""
        applied={UiInspector.inlineApplied(id, i)}
        edit={edit}
        removable={true}
      />,
    );
  }
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
      {inline.length > 0 || edit ? <text style={{ color: "#f0b86e", fontSize: 12 }}>element.style</text> : null}
      {inline}
      {edit ? <UiInspAdd id={id} /> : null}
      {rules}
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
  const [, setTick] = useState(0);
  useEffect(() => {
    return input.on("keydown", (e) => {
      if (e.code === "F12") {
        setOn((v) => !v);
        setEdit(false);
      } else if (e.code === "F9") {
        UiInspector.select(UiInspector.selected() !== 0 ? 0 : UiInspector.hovered());
        setEdit(false);
      } else if (e.code === "F8") {
        if (UiInspector.selected() === 0) UiInspector.select(UiInspector.hovered());
        setEdit((v) => !v);
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
  // Editing outlines the pinned node: the pointer is on the card, not on what is being edited.
  const outlined = edit && pinned !== 0 ? pinned : hovered;
  return (
    <box style={{ position: "fixed", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" }}>
      {outlined !== 0 ? <UiInspOutline id={outlined} /> : null}
      {shown !== 0 ? <UiInspCard id={shown} pinned={pinned !== 0} edit={edit && pinned !== 0} /> : null}
    </box>
  );
}

if (UiInspector.enabled()) {
  createRoot(9000001).render(<UiInspOverlay />);
}
