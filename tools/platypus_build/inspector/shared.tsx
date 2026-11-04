// The UI inspector's parts that both the overlay (`overlay.tsx`, appended to a program by
// `platypus-build build --add-ui-inspector`) and the editor's "UI" panel
// (`editor/src/UiInspectorPanel.tsx`, which imports this file) draw: names, an editable field,
// and the lists of a node's inline styles and matching rules. They use only the `UiInspector` API,
// which is always declared and answers nothing unless the host runs the inspector, so a program
// built without the flag still compiles with them. Names start with `UiInsp` so they never meet
// a game's. When the build appends this file it drops the `export`s.
// Design: docs/research/platypus-ui-inspector.md

/** `box#id.class.other`: what the inspector calls a node. */
export function UiInspName(id: number): string {
  const eid = UiInspector.elementId(id);
  const cls = UiInspector.className(id);
  let s = UiInspector.kind(id);
  if (eid !== "") s += "#" + eid;
  if (cls !== "") s += "." + cls.split(" ").join(".");
  return s;
}

export function UiInspNum(n: number): string {
  return String(Math.round(n * 10) / 10);
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
        <box style={{ cursor: "pointer", padding: "0 4px" }} onClick={() => UiInspector.setStyle(id, prop, "")}>
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

/**
 * A node's styles: `element.style` (inline), then the matching rules, highest priority first,
 * overridden declarations struck through. With `edit`, values are fields and a row adds a property
 * (the edits are inline overrides of the node).
 */
export function UiInspStyles(props: { id: number; edit: boolean }) {
  const { id, edit } = props;
  const rules: ReactNode[] = [];
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
    <box style={{ flexDirection: "column" }}>
      {inline.length > 0 || edit ? <text style={{ color: "#f0b86e", fontSize: 12 }}>element.style</text> : null}
      {inline}
      {edit ? <UiInspAdd id={id} /> : null}
      {rules}
    </box>
  );
}
