// The UI inspector's parts that both the overlay (`overlay.tsx`, appended to a program by
// `platypus-build build --add-ui-inspector`) and the editor's "UI" panel
// (`editor/src/UiInspectorPanel.tsx`, which imports this file) draw: names, the style editor and its
// widgets. They use only the `UiInspector` API, which is always declared and answers nothing unless
// the host runs the inspector, so a program built without the flag still compiles with them. Names
// start with `UiInsp` so they never meet a game's. When the build appends this file it drops the
// `export`s and the `import`s. The look is `inspector.css` (classes `uii-*`, a dark and a light theme),
// which the build adds to `game.css` with the flag: an entry point wraps what it draws in `UiInspRoot`.
// What the style editor offers: Up / Down on a value add one unit (Shift ten, Alt a tenth); names and
// keywords complete from the properties the engine supports (with their docs); a swatch opens a colour
// picker (hex / rgb / hsl), a timing function opens a cubic-bezier editor, a gradient opens a stops
// editor, a `display: flex` node gets a flexbox editor; `calc()`, `var()` and relative units show what
// they come to; states (`:hover` ...), classes and pseudo-elements can be toggled and added.
// Design: docs/research/platypus-ui-inspector.md

import {
  uiValCamel,
  uiValColorFormat,
  uiValEasingPoint,
  uiValFindEasing,
  uiValFormatColor,
  uiValFormatEasing,
  uiValFormatGradient,
  uiValFormatNumber,
  uiValFromHsv,
  uiValGradientAngle,
  uiValKebab,
  uiValNumbers,
  uiValParseColor,
  uiValParseGradient,
  uiValStep,
  uiValStepSize,
  uiValSuggest,
  uiValToHsv,
  UI_VAL_EASINGS,
  type UiValColorFormat,
  type UiValGradient,
} from "./values.ts";

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

function UiInspClamp01(n: number): number {
  return n < 0 ? 0 : n > 1 ? 1 : n;
}

// --- Theme -----------------------------------------------------------------------------------------

/** What an entry point draws inside: the colour variables of the theme (`""`: follow the system; `dark`; `light`). */
export function UiInspRoot(props: { theme: string; style?: PlatypusStyle; children?: ReactNode }) {
  return (
    <box className={"uii" + (props.theme !== "" ? " uii-" + props.theme : "")} style={props.style}>
      {props.children}
    </box>
  );
}

/** The next theme in the cycle system, light, dark. */
export function UiInspNextTheme(theme: string): string {
  return theme === "" ? "light" : theme === "light" ? "dark" : "";
}

/** A pill that shows the theme and cycles it. */
export function UiInspThemeChip(props: { theme: string; onTheme: (theme: string) => void }) {
  return (
    <UiInspChip
      label={props.theme === "" ? "theme: system" : props.theme === "light" ? "theme: light" : "theme: dark"}
      onClick={() => props.onTheme(UiInspNextTheme(props.theme))}
    />
  );
}

// --- Pieces ----------------------------------------------------------------------------------------

/** A small labelled pill; `on` highlights it. */
function UiInspChip(props: { label: string; on?: boolean; onClick: () => void; dim?: boolean }) {
  return (
    <box className={props.on === true ? "uii-chip on" : "uii-chip"} onClick={props.onClick}>
      <text className={props.dim === true ? "uii-chip-text uii-chip-dim" : "uii-chip-text"}>{props.label}</text>
    </box>
  );
}

/** The red × that removes something. */
function UiInspX(props: { onClick: () => void }) {
  return (
    <box className="uii-x" onClick={props.onClick}>
      <text className="uii-x-text">×</text>
    </box>
  );
}

/**
 * A box that reports where the pointer is inside it (0..1 on each axis) while pressed. It asks the host for
 * its own place on screen (`probe`) when the pointer first enters it, so its children must not take
 * pointer events (`pointerEvents: "none"`).
 */
function UiInspPad(props: { width: number; height: number; onPick: (fx: number, fy: number) => void; children?: ReactNode }) {
  const node = useRef(0);
  const down = useRef(false);
  const pick = (e: PlatypusPointerEvent): void => {
    const id = node.current;
    const w = UiInspector.width(id);
    const h = UiInspector.height(id);
    if (id === 0 || w <= 0 || h <= 0) return;
    props.onPick(UiInspClamp01((e.clientX - UiInspector.x(id)) / w), UiInspClamp01((e.clientY - UiInspector.y(id)) / h));
  };
  return (
    <box
      style={{ position: "relative", width: props.width, height: props.height, cursor: "crosshair" }}
      onMouseEnter={(e: PlatypusPointerEvent) => {
        node.current = e.target.id;
        UiInspector.probe(e.target.id);
      }}
      onPointerDown={(e: PlatypusPointerEvent) => {
        down.current = true;
        e.currentTarget.setPointerCapture(e.pointerId);
        pick(e);
      }}
      onPointerMove={(e: PlatypusPointerEvent) => {
        if (down.current) pick(e);
      }}
      onPointerUp={(e: PlatypusPointerEvent) => {
        down.current = false;
        e.currentTarget.releasePointerCapture(e.pointerId);
      }}
    >
      {props.children}
    </box>
  );
}

/** A list of completions under a field; a press picks without taking the focus from the field. */
function UiInspSuggest(props: { items: string[]; onPick: (word: string) => void }) {
  if (props.items.length === 0) return null;
  return (
    <box className="uii-sugg" style={{ flexDirection: "column" }}>
      {props.items.map((w, i) => (
        <box
          key={i}
          className={i === 0 ? "uii-sugg-item first" : "uii-sugg-item"}
          onPointerDown={(e: PlatypusPointerEvent) => {
            e.preventDefault();
            props.onPick(w);
          }}
        >
          <text className="uii-text">{w}</text>
        </box>
      ))}
    </box>
  );
}

// --- Text fields -----------------------------------------------------------------------------------

/**
 * A text field that commits on Enter or when left; Escape puts the value back. `words` are what it
 * completes (Tab or a press takes the first). `boxed` draws it as an input even when idle.
 */
function UiInspField(props: { value: string; placeholder?: string; width: number; words?: string[]; boxed?: boolean; onCommit: (s: string) => void }) {
  const [text, setText] = useState(props.value);
  const [focused, setFocused] = useState(false);
  const editing = useRef(false);
  useEffect(() => {
    if (!editing.current) setText(props.value);
  }, [props.value]);
  const commit = (s: string): void => {
    editing.current = false;
    setFocused(false);
    if (s !== props.value) props.onCommit(s);
  };
  const suggestions = focused && props.words !== undefined ? uiValSuggest(text, props.words, 6) : [];
  return (
    <box style={{ flexDirection: "column" }}>
      <Input
        className={props.boxed === true ? "uii-field uii-boxed" : "uii-field"}
        value={text}
        placeholder={props.placeholder}
        style={{ width: props.width }}
        onFocus={() => setFocused(true)}
        onChange={(e) => {
          editing.current = true;
          setText(e.value);
        }}
        onKeyDown={(e: PlatypusKeyboardEvent) => {
          if (e.key === "Enter") commit(text);
          else if (e.key === "Escape") {
            editing.current = false;
            setText(props.value);
            setFocused(false);
          } else if (e.key === "Tab" && suggestions.length > 0) {
            e.preventDefault();
            editing.current = true;
            setText(suggestions[0]!);
          }
        }}
        onBlur={() => commit(text)}
      />
      <UiInspSuggest
        items={suggestions}
        onPick={(w) => {
          editing.current = false;
          setText(w);
          setFocused(false);
          props.onCommit(w);
        }}
      />
    </box>
  );
}

/**
 * The field of a style value. Up and Down add one unit to the number (Shift: ten, Alt: a tenth) and
 * commit at once; the keywords the property takes complete as you type.
 */
function UiInspValueField(props: {
  prop: string;
  value: string;
  width: number;
  boxed?: boolean;
  /** Where the caret can be put from outside (to come back to the value from the name). */
  inputRef?: RefObject<PlatypusElement | null>;
  /** Backspace in an empty field: the line's owner moves on to the name (the empty text is not committed). */
  onBackspaceEmpty?: () => void;
  onCommit: (s: string) => void;
}) {
  const [text, setText] = useState(props.value);
  const [focused, setFocused] = useState(false);
  const editing = useRef(false);
  // Set when Backspace left the field: leaving it must not commit its (empty) text.
  const left = useRef(false);
  useEffect(() => {
    if (!editing.current) setText(props.value);
  }, [props.value]);
  const commit = (s: string): void => {
    editing.current = false;
    setFocused(false);
    if (left.current) {
      left.current = false;
      return;
    }
    if (s !== props.value) props.onCommit(s);
  };
  const words = focused ? UiInspector.styleKeywords(props.prop) : [];
  const suggestions = focused && text.indexOf(" ") < 0 ? uiValSuggest(text, words, 6) : [];
  // A value wider than its field is shown wrapped, in a few lines, so all of it can be read; a one-line
  // field would only show its end. (Chosen by the committed value, so typing never swaps the element
  // under the caret.)
  // The width a value needs is the estimate the line sizes its field by (6.4px a character and 16 of padding), so
  // only a field held at its largest width, or one set narrower than its value, wraps.
  const natural = props.value.length * 6.4 + 16;
  const wrapped = natural > props.width + 2;
  const rows = Math.max(2, Math.ceil((props.value.length * 6.9) / (props.width - 14)));
  const onChangeText = (e: PlatypusInputEvent): void => {
    editing.current = true;
    setText(e.value);
  };
  const onKey = (e: PlatypusKeyboardEvent): void => {
    if (e.key === "Backspace" && text === "" && props.onBackspaceEmpty !== undefined) {
      e.preventDefault();
      left.current = true;
      props.onBackspaceEmpty();
    } else if (e.key === "Enter") {
      // Enter commits (a wrapped value is a text area, where it would add a line).
      e.preventDefault();
      commit(text);
    } else if (e.key === "Escape") {
      editing.current = false;
      setText(props.value);
      setFocused(false);
    } else if (e.key === "Tab" && suggestions.length > 0) {
      e.preventDefault();
      editing.current = true;
      setText(suggestions[0]!);
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      const nums = uiValNumbers(text);
      if (nums.length === 0) return;
      e.preventDefault();
      const big = e.shiftKey ? 10 : e.altKey ? 0.1 : 1;
      const next = uiValStep(text, e.key === "ArrowUp" ? 1 : -1, uiValStepSize(props.prop, nums[0]!.unit) * big, -1);
      if (next !== null) {
        editing.current = false;
        setText(next);
        props.onCommit(next);
      }
    }
  };
  return (
    <box style={{ flexDirection: "column" }}>
      {wrapped ? (
        <TextArea
          ref={props.inputRef}
          rows={rows}
          className={props.boxed === true ? "uii-field uii-boxed" : "uii-field"}
          value={text}
          style={{ width: props.width }}
          onFocus={() => setFocused(true)}
          onChange={onChangeText}
          onKeyDown={onKey}
          onBlur={() => commit(text)}
        />
      ) : (
        <Input
          ref={props.inputRef}
          className={props.boxed === true ? "uii-field uii-boxed" : "uii-field"}
          value={text}
          style={{ width: props.width }}
          onFocus={() => setFocused(true)}
          onChange={onChangeText}
          onKeyDown={onKey}
          onBlur={() => commit(text)}
        />
      )}
      <UiInspSuggest
        items={suggestions}
        onPick={(w) => {
          editing.current = false;
          setText(w);
          setFocused(false);
          props.onCommit(w);
        }}
      />
    </box>
  );
}

// --- Colour picker ---------------------------------------------------------------------------------

const UiInspPickW = 176;
const UiInspPickH = 112;

/** A saturation / value square, hue and alpha sliders, and the format the colour is written in. */
function UiInspColorPicker(props: { value: string; onChange: (text: string) => void }) {
  const start = uiValParseColor(props.value) ?? { r: 255, g: 255, b: 255, a: 1 };
  const [hsv, setHsv] = useState(() => {
    const v = uiValToHsv(start);
    return [v[0]!, v[1]!, v[2]!, start.a];
  });
  const [format, setFormat] = useState<UiValColorFormat>(uiValColorFormat(props.value));
  const last = useRef(props.value);
  // Another edit of the value (typing in the field) moves the picker; its own output does not.
  useEffect(() => {
    if (props.value === last.current) return;
    last.current = props.value;
    const c = uiValParseColor(props.value);
    if (c !== null) {
      const v = uiValToHsv(c);
      setHsv([v[0]!, v[1]!, v[2]!, c.a]);
    }
  }, [props.value]);
  const emit = (h: number, s: number, v: number, a: number, f: UiValColorFormat): void => {
    setHsv([h, s, v, a]);
    const text = uiValFormatColor(uiValFromHsv(h, s, v, a), f);
    last.current = text;
    props.onChange(text);
  };
  const h = hsv[0]!;
  const s = hsv[1]!;
  const v = hsv[2]!;
  const a = hsv[3]!;
  const hueColor = uiValFormatColor(uiValFromHsv(h, 1, 1, 1), "hex");
  const solid = uiValFormatColor(uiValFromHsv(h, s, v, 1), "hex");
  const fill = { position: "absolute", left: 0, top: 0, width: "100%", height: "100%", pointerEvents: "none" } as const;
  return (
    <box className="uii-pop" style={{ flexDirection: "column", width: UiInspPickW + 16 }}>
      <UiInspPad width={UiInspPickW} height={UiInspPickH} onPick={(fx, fy) => emit(h, fx, 1 - fy, a, format)}>
        <box style={{ ...fill, borderRadius: 4, backgroundColor: hueColor }} />
        <box style={{ ...fill, borderRadius: 4, backgroundImage: "linear-gradient(to right, #ffffff, rgba(255,255,255,0))" }} />
        <box style={{ ...fill, borderRadius: 4, backgroundImage: "linear-gradient(to top, #000000, rgba(0,0,0,0))" }} />
        <box className="uii-knob" style={{ position: "absolute", left: s * UiInspPickW - 6, top: (1 - v) * UiInspPickH - 6, width: 12, height: 12, borderRadius: 6, pointerEvents: "none" }} />
      </UiInspPad>
      <UiInspPad width={UiInspPickW} height={12} onPick={(fx) => emit(fx * 360, s, v, a, format)}>
        <box style={{ ...fill, borderRadius: 6, backgroundImage: "linear-gradient(to right, #ff0000, #ffff00, #00ff00, #00ffff, #0000ff, #ff00ff, #ff0000)" }} />
        <box className="uii-knob" style={{ position: "absolute", left: (h / 360) * UiInspPickW - 7, top: -1, width: 14, height: 14, borderRadius: 7, pointerEvents: "none" }} />
      </UiInspPad>
      <UiInspPad width={UiInspPickW} height={12} onPick={(fx) => emit(h, s, v, Math.round(fx * 100) / 100, format)}>
        <box style={{ ...fill, borderRadius: 6, backgroundColor: "#8a8f99" }} />
        <box style={{ ...fill, borderRadius: 6, backgroundImage: "linear-gradient(to right, rgba(0,0,0,0), " + solid + ")" }} />
        <box className="uii-knob" style={{ position: "absolute", left: a * UiInspPickW - 7, top: -1, width: 14, height: 14, borderRadius: 7, pointerEvents: "none" }} />
      </UiInspPad>
      <box style={{ flexDirection: "row", gap: 4, alignItems: "center" }}>
        <text className="uii-dim">as</text>
        {["hex", "rgb", "hsl"].map((f) => (
          <UiInspChip
            key={f}
            label={f}
            on={format === f}
            onClick={() => {
              const next: UiValColorFormat = f === "rgb" ? "rgb" : f === "hsl" ? "hsl" : "hex";
              setFormat(next);
              emit(h, s, v, a, next);
            }}
          />
        ))}
      </box>
    </box>
  );
}

// --- Cubic-bezier editor ---------------------------------------------------------------------------

const UiInspCurveSize = 140;
/** The graph shows y from -0.5 to 1.5, so overshooting curves have room. */
const UiInspCurveLo = -0.5;
const UiInspCurveSpan = 2;

/** The easing graph with two draggable handles, and the keyword presets. */
function UiInspEasingEditor(props: { curve: number[]; onChange: (curve: number[]) => void }) {
  const c = props.curve;
  const [grab, setGrab] = useState(0);
  const px = (x: number): number => x * UiInspCurveSize;
  const py = (y: number): number => (1 - (y - UiInspCurveLo) / UiInspCurveSpan) * UiInspCurveSize;
  const dots: ReactNode[] = [];
  for (let i = 0; i <= 32; i++) {
    const p = uiValEasingPoint(c, i / 32);
    dots.push(<box key={i} style={{ position: "absolute", left: px(p[0]!) - 1.5, top: py(p[1]!) - 1.5, width: 3, height: 3, borderRadius: 2, backgroundColor: "#6cb6ff", pointerEvents: "none" }} />);
  }
  const fill = { position: "absolute", pointerEvents: "none" } as const;
  return (
    <box className="uii-pop" style={{ flexDirection: "column", width: UiInspCurveSize + 16 }}>
      <UiInspPad
        width={UiInspCurveSize}
        height={UiInspCurveSize}
        onPick={(fx, fy) => {
          const y = Math.round((UiInspCurveLo + (1 - fy) * UiInspCurveSpan) * 100) / 100;
          const x = Math.round(fx * 100) / 100;
          // The press moves the handle nearest the pointer and keeps moving it while held.
          let which = grab;
          if (which === 0) {
            const d1 = (x - c[0]!) * (x - c[0]!) + (y - c[1]!) * (y - c[1]!);
            const d2 = (x - c[2]!) * (x - c[2]!) + (y - c[3]!) * (y - c[3]!);
            which = d1 <= d2 ? 1 : 2;
            setGrab(which);
          }
          props.onChange(which === 1 ? [x, y, c[2]!, c[3]!] : [c[0]!, c[1]!, x, y]);
        }}
      >
        <box className="uii-graph" style={{ ...fill, left: 0, top: 0, width: "100%", height: "100%" }} />
        <box className="uii-line" style={{ ...fill, left: 0, top: py(1), width: "100%", height: 1 }} />
        <box className="uii-line" style={{ ...fill, left: 0, top: py(0), width: "100%", height: 1 }} />
        {dots}
        <box style={{ ...fill, left: px(0), top: py(0) - 1, width: px(c[0]!) - px(0), height: 2, backgroundColor: "rgba(240, 184, 110, 0.5)" }} />
        <box className="uii-knob" style={{ ...fill, left: px(c[0]!) - 6, top: py(c[1]!) - 6, width: 12, height: 12, borderRadius: 6, backgroundColor: "#f0b86e" }} />
        <box className="uii-knob" style={{ ...fill, left: px(c[2]!) - 6, top: py(c[3]!) - 6, width: 12, height: 12, borderRadius: 6, backgroundColor: "#7ee0a0" }} />
      </UiInspPad>
      <text className="uii-dim">{uiValFormatEasing(c)}</text>
      <box style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
        {UI_VAL_EASINGS.map((e) => (
          <UiInspChip
            key={e.name}
            label={e.name}
            on={uiValFormatEasing(c) === e.name}
            onClick={() => {
              setGrab(0);
              props.onChange(e.curve);
            }}
          />
        ))}
      </box>
    </box>
  );
}

// --- Gradient helper -------------------------------------------------------------------------------

/** A typed angle: a bare number is degrees, anything else stays as written (`to right`). */
function UiInspAngle(text: string): string {
  const t = text.trim();
  if (t === "") return "";
  const n = parseFloat(t);
  return !Number.isNaN(n) && String(n) === t ? t + "deg" : t;
}

/** A preview bar, the direction, and the stops (colour and position, add and remove). */
function UiInspGradientEditor(props: { value: string; onChange: (text: string) => void }) {
  const g = uiValParseGradient(props.value);
  const [openStop, setOpenStop] = useState(-1);
  if (g === null) return null;
  const angle = uiValGradientAngle(g);
  const set = (next: UiValGradient): void => props.onChange(uiValFormatGradient(next));
  return (
    <box className="uii-pop" style={{ flexDirection: "column", width: 250 }}>
      <box className="uii-graph" style={{ height: 22, backgroundImage: props.value }} />
      {g.kind === "linear" ? (
        <box style={{ flexDirection: "row", gap: 4, alignItems: "center", flexWrap: "wrap" }}>
          <text className="uii-dim">angle</text>
          <UiInspField boxed={true} value={angle === null ? g.head : String(angle)} width={52} onCommit={(s) => set({ ...g, head: UiInspAngle(s) })} />
          {[
            ["↑", "0deg"],
            ["→", "90deg"],
            ["↓", "180deg"],
            ["←", "270deg"],
          ].map((d) => (
            <UiInspChip key={d[1]} label={d[0]!} on={g.head === d[1]} onClick={() => set({ ...g, head: d[1]! })} />
          ))}
        </box>
      ) : (
        <box style={{ flexDirection: "row", gap: 4, alignItems: "center" }}>
          <text className="uii-dim">shape</text>
          <UiInspField boxed={true} value={g.head} width={150} onCommit={(s) => set({ ...g, head: s.trim() })} />
        </box>
      )}
      {g.stops.map((st, i) => (
        <box key={i} style={{ flexDirection: "column" }}>
          <box style={{ flexDirection: "row", gap: 5, alignItems: "center" }}>
            <box
              className="uii-swatch"
              style={{ width: 18, height: 18, backgroundColor: st.color }}
              onClick={() => setOpenStop(openStop === i ? -1 : i)}
            />
            <UiInspField boxed={true} value={st.color} width={104} onCommit={(s) => set({ ...g, stops: g.stops.map((x, j) => (j === i ? { color: s, pos: x.pos } : x)) })} />
            <UiInspField boxed={true} value={st.pos} placeholder="pos" width={54} onCommit={(s) => set({ ...g, stops: g.stops.map((x, j) => (j === i ? { color: x.color, pos: s.trim() } : x)) })} />
            {g.stops.length > 2 ? <UiInspX onClick={() => set({ ...g, stops: g.stops.filter((_, j) => j !== i) })} /> : null}
          </box>
          {openStop === i ? (
            <UiInspColorPicker value={st.color} onChange={(s) => set({ ...g, stops: g.stops.map((x, j) => (j === i ? { color: s, pos: x.pos } : x)) })} />
          ) : null}
        </box>
      ))}
      <box style={{ flexDirection: "row" }}>
        <UiInspChip label="+ stop" onClick={() => set({ ...g, stops: g.stops.concat([{ color: g.stops[g.stops.length - 1]!.color, pos: "" }]) })} />
      </box>
    </box>
  );
}

// --- Flexbox editor --------------------------------------------------------------------------------

function UiInspFlexRow(props: { id: number; label: string; prop: string; options: string[] }) {
  const current = UiInspector.layout(props.id, props.prop);
  return (
    <box style={{ flexDirection: "row", gap: 3, alignItems: "center", flexWrap: "wrap" }}>
      <text className="uii-dim" style={{ width: 54 }}>{props.label}</text>
      {props.options.map((o) => (
        <UiInspChip key={o} label={o} on={current === o} onClick={() => UiInspector.setStyle(props.id, props.prop, o)} />
      ))}
    </box>
  );
}

/** A flex container: a live preview of three boxes laid out as set, and a button per choice. */
function UiInspFlexEditor(props: { id: number }) {
  const id = props.id;
  const dir = UiInspector.layout(id, "flexDirection");
  const wrap = UiInspector.layout(id, "flexWrap");
  const justify = UiInspector.layout(id, "justifyContent");
  const align = UiInspector.layout(id, "alignItems");
  const gap = UiInspector.layout(id, "rowGap");
  const kid = (n: number, w: number, h: number): ReactNode => (
    <box key={n} style={{ width: w, height: h, borderRadius: 3, backgroundColor: n === 1 ? "#f0b86e" : n === 2 ? "#6cb6ff" : "#7ee0a0" }} />
  );
  return (
    <box className="uii-pop" style={{ flexDirection: "column", width: 290 }}>
      <box
        className="uii-flex-box"
        style={{
          width: 270,
          height: 88,
          padding: 4,
          display: "flex",
          flexDirection: dir === "column" ? "column" : dir === "row-reverse" ? "row-reverse" : dir === "column-reverse" ? "column-reverse" : "row",
          flexWrap: wrap === "wrap" ? "wrap" : wrap === "wrap-reverse" ? "wrap-reverse" : "nowrap",
          justifyContent: justify === "normal" ? "flex-start" : justify,
          alignItems: align === "normal" ? "stretch" : align,
          gap: gap === "" ? 4 : gap,
        }}
      >
        {kid(1, 36, 22)}
        {kid(2, 36, 40)}
        {kid(3, 36, 30)}
      </box>
      <UiInspFlexRow id={id} label="direction" prop="flexDirection" options={["row", "column", "row-reverse", "column-reverse"]} />
      <UiInspFlexRow id={id} label="justify" prop="justifyContent" options={["flex-start", "center", "flex-end", "space-between", "space-around", "space-evenly"]} />
      <UiInspFlexRow id={id} label="align" prop="alignItems" options={["flex-start", "center", "flex-end", "stretch", "baseline"]} />
      <UiInspFlexRow id={id} label="wrap" prop="flexWrap" options={["nowrap", "wrap", "wrap-reverse"]} />
      <box style={{ flexDirection: "row", gap: 4, alignItems: "center" }}>
        <text className="uii-dim" style={{ width: 54 }}>gap</text>
        <UiInspValueField boxed={true} prop="gap" value={gap} width={72} onCommit={(s) => UiInspector.setStyle(id, "gap", s)} />
      </box>
    </box>
  );
}

// --- Style lines -----------------------------------------------------------------------------------

/** What a value can open under its line: `color`, `easing`, `gradient` or nothing. */
function UiInspHelperKind(prop: string, value: string): string {
  if (uiValParseColor(value) !== null && value.trim() !== "") return "color";
  const easingProp = prop.startsWith("transition") || prop.startsWith("animation");
  if (easingProp && uiValFindEasing(value) !== null) return "easing";
  if ((prop === "backgroundImage" || prop === "background") && uiValParseGradient(value) !== null) return "gradient";
  return "";
}

/** Whether a property is about the box (margin, padding, border, size): hovering it shows the box model. */
function UiInspBoxProp(prop: string): boolean {
  return prop.startsWith("margin") || prop.startsWith("padding") || prop.startsWith("border") || prop === "width" || prop === "height" || prop === "gap";
}

// The doc of the property under the pointer, for the strip at the foot of a card (`UiInspDocLine`). A line
// sets it when the pointer enters and clears it when it leaves; the strip reads it on every redraw.
let uiInspDoc = "";
let uiInspDocOwner = "";

/**
 * The strip at the foot of a card: the name and doc of the property the pointer is on. It always takes the
 * same room, so it neither covers a row nor moves one.
 */
export function UiInspDocLine() {
  return (
    <box className="uii-foot">
      <text className="uii-dim">{uiInspDoc === "" ? "Hover a property to read what it does." : uiInspDoc}</text>
    </box>
  );
}

/** A property name as typed (`background-color` or `backgroundColor`): the engine's name for it, `""` when it is not one. */
function UiInspProp(text: string): string {
  const t = text.trim();
  if (t.startsWith("--") && t.length > 2) return t;
  const camel = uiValCamel(t);
  return UiInspector.styleProps().indexOf(camel) >= 0 ? camel : "";
}

/** Why a typed property name was refused, with the nearest supported one when there is one. */
function UiInspPropError(text: string): string {
  const t = text.trim();
  if (t === "") return "Type a property name first.";
  const guess = uiValSuggest(uiValCamel(t), UiInspector.styleProps(), 1);
  return "`" + t + "` is not a style property here" + (guess.length > 0 ? ". Did you mean " + uiValKebab(guess[0]!) + "?" : ".");
}

/** A small error balloon under a field; it floats, so nothing below it moves. */
function UiInspError(props: { text: string }) {
  return (
    <box className="uii-error" style={{ position: "absolute", left: 0, top: 24, maxWidth: 340, pointerEvents: "none" }}>
      <text className="uii-error-text">{props.text}</text>
    </box>
  );
}

/**
 * The property name of an `element.style` line, editable. `:`, Enter and Tab check the name (a wrong one
 * gets an error balloon) and move on to the value; Backspace in an emptied name removes the line.
 */
function UiInspKeyField(props: { value: string; inputRef: RefObject<PlatypusElement | null>; onRename: (name: string) => void; onNext: () => void; onRemove: () => void }) {
  const [text, setText] = useState(props.value);
  const [err, setErr] = useState("");
  const editing = useRef(false);
  useEffect(() => {
    if (!editing.current) setText(props.value);
  }, [props.value]);
  // The name is good: the line is renamed when it changed.
  const accept = (): boolean => {
    const name = UiInspProp(text);
    if (name === "") {
      setErr(UiInspPropError(text));
      return false;
    }
    setErr("");
    editing.current = false;
    if (name !== UiInspProp(props.value)) props.onRename(name);
    return true;
  };
  return (
    <box style={{ flexDirection: "row", alignItems: "flex-start", position: "relative", zIndex: err !== "" ? 100 : 0 }}>
      <Input
        ref={props.inputRef}
        className="uii-field uii-key"
        value={text}
        style={{ width: Math.max(30, text.length * 6.6 + 12) }}
        onChange={(e) => {
          editing.current = true;
          setErr("");
          setText(e.value);
        }}
        onKeyDown={(e: PlatypusKeyboardEvent) => {
          if (e.key === "Backspace" && text === "") {
            e.preventDefault();
            props.onRemove();
          } else if (e.key === ":" || e.key === "Enter" || e.key === "Tab") {
            e.preventDefault();
            if (accept()) props.onNext();
          } else if (e.key === "Escape") {
            editing.current = false;
            setErr("");
            setText(props.value);
          }
        }}
        onBlur={() => {
          // Leaving with a changed name renames the line if the name is good, else puts the old one back.
          if (editing.current && !accept()) {
            editing.current = false;
            setErr("");
            setText(props.value);
          }
        }}
      />
      <text className="uii-prop" style={{ marginLeft: -3 }}>:</text>
      {err !== "" ? <UiInspError text={err} /> : null}
    </box>
  );
}

/** The inline override that decides `prop` of a node (`""` when none). */
function UiInspOverride(id: number, prop: string): string {
  let out = "";
  for (let i = 0; i < UiInspector.inlineCount(id); i++) {
    if (UiInspector.inlineProp(id, i) === prop && UiInspector.inlineApplied(id, i)) out = UiInspector.inlineValue(id, i);
  }
  return out;
}

/**
 * One `prop: value` line: plain text, or in edit mode the editable value with the helper it earns. A
 * rule's line whose property has an inline override shows that value (the one in effect), marked
 * edited, with a button that gives the rule's own back (`override` is that value, `""` for none).
 */
function UiInspLine(props: { id: number; prop: string; value: string; override: string; computed: string; suffix: string; applied: boolean; edit: boolean; removable: boolean }) {
  const { id, prop, applied, edit } = props;
  const edited = props.override !== "" && edit;
  // What the line stands for: the override once there is one.
  const value = edited ? props.override : props.value;
  const [open, setOpen] = useState(false);
  const pop = useRef<PlatypusElement | null>(null);
  // The name and the value, to move the caret between them.
  const keyBox = useRef<PlatypusElement | null>(null);
  const valBox = useRef<PlatypusElement | null>(null);
  // A helper opened below the visible part of the scrolling card scrolls into view.
  useEffect(() => {
    if (open) pop.current?.scrollIntoView({ block: "nearest" });
  }, [open]);
  const kind = edit ? UiInspHelperKind(prop, value) : "";
  const shownComputed = props.computed !== "" ? "= " + props.computed : "";
  if (!edit) {
    return (
      <text className={applied ? "uii-text" : "uii-text uii-off"} style={{ paddingLeft: 12 }}>
        {uiValKebab(prop) + ": " + value + props.suffix + (shownComputed !== "" ? "   " + shownComputed : "")}
      </text>
    );
  }
  const easing = kind === "easing" ? uiValFindEasing(value) : null;
  return (
    <box
      style={{ flexDirection: "column", marginBottom: 1, position: "relative" }}
      onMouseEnter={() => {
        // The property's doc goes to the strip at the foot of the card.
        uiInspDoc = UiInspector.styleDoc(prop) === "" ? "" : uiValKebab(prop) + " — " + UiInspector.styleDoc(prop);
        uiInspDocOwner = id + ":" + prop;
        // A line about the box shows the box model over the node, while the pointer is on it.
        if (UiInspBoxProp(prop)) UiInspector.showBox(id);
      }}
      onMouseLeave={() => {
        if (uiInspDocOwner === id + ":" + prop) {
          uiInspDoc = "";
          uiInspDocOwner = "";
        }
        if (UiInspBoxProp(prop)) UiInspector.showBox(0);
      }}
    >
      <box className="uii-row">
        {props.removable ? (
          // A line of element.style: the name is editable, and Backspace through the value, then the name, removes it.
          <UiInspKeyField
            value={uiValKebab(prop)}
            inputRef={keyBox}
            onRename={(name) => {
              UiInspector.setStyle(id, prop, "");
              UiInspector.setStyle(id, name, value);
            }}
            onNext={() => valBox.current?.focus()}
            onRemove={() => UiInspector.setStyle(id, prop, "")}
          />
        ) : (
          <text className={applied || edited ? "uii-prop" : "uii-prop uii-off"}>{uiValKebab(prop) + ":"}</text>
        )}
        {kind === "color" ? (
          <box className="uii-swatch" style={{ width: 18, height: 18, marginTop: 1, marginLeft: 6, backgroundColor: value }} onClick={() => setOpen(!open)} />
        ) : null}
        <UiInspValueField
          prop={prop}
          value={value}
          inputRef={valBox}
          onBackspaceEmpty={props.removable ? () => keyBox.current?.focus() : undefined}
          width={Math.min(280, Math.max(kind === "color" ? 60 : 44, value.length * 6.4 + 16))}
          onCommit={(s) => UiInspector.setStyle(id, prop, s)}
        />
        {/* What the value comes to, right beside it; it is read again every frame, so it follows the layout. */}
        {shownComputed !== "" ? <text className="uii-computed">{shownComputed}</text> : null}
        {kind === "easing" || kind === "gradient" ? <UiInspChip label={kind === "easing" ? "curve" : "stops"} on={open} onClick={() => setOpen(!open)} /> : null}
        {edited ? <UiInspChip label="edited  ↺" on={true} onClick={() => UiInspector.setStyle(id, prop, "")} /> : null}
      </box>
      {open ? (
        <box ref={pop} style={{ flexDirection: "column" }}>
          {kind === "color" ? <UiInspColorPicker value={value} onChange={(s) => UiInspector.setStyle(id, prop, s)} /> : null}
          {easing !== null ? (
            <UiInspEasingEditor
              curve={easing.curve}
              onChange={(c) => UiInspector.setStyle(id, prop, value.substring(0, easing.start) + uiValFormatEasing(c) + value.substring(easing.end))}
            />
          ) : null}
          {kind === "gradient" ? <UiInspGradientEditor value={value} onChange={(s) => UiInspector.setStyle(id, prop, s)} /> : null}
        </box>
      ) : null}
    </box>
  );
}

/** A matching rule; `claims` are the properties whose inline override this rule's line stands for. */
function UiInspRule(props: { id: number; rule: number; edit: boolean; claims: string[] }) {
  const { id, rule, edit } = props;
  const rows: ReactNode[] = [];
  const n = UiInspector.declCount(id, rule);
  for (let i = 0; i < n; i++) {
    const prop = UiInspector.declProp(id, rule, i);
    rows.push(
      <UiInspLine
        key={i}
        id={id}
        prop={prop}
        override={props.claims.indexOf(prop) >= 0 ? UiInspOverride(id, prop) : ""}
        value={UiInspector.declValue(id, rule, i)}
        computed={UiInspector.declComputed(id, rule, i)}
        suffix={UiInspector.declImportant(id, rule, i) ? " !important" : ""}
        applied={UiInspector.declApplied(id, rule, i)}
        edit={edit}
        removable={false}
      />,
    );
  }
  return (
    <UiInspBlock
      head={UiInspector.ruleSelector(id, rule)}
      onEnter={() => UiInspector.highlightRule(UiInspector.ruleSheet(id, rule), UiInspector.ruleIndex(id, rule))}
      onLeave={() => UiInspector.clearHighlight()}
    >
      {rows}
    </UiInspBlock>
  );
}

/** A rule as devtools draws it: `selector {`, its lines, `}`; hovering it can outline what it is about. */
function UiInspBlock(props: { head: string; onEnter?: () => void; onLeave?: () => void; children?: ReactNode }) {
  return (
    <box className="uii-block" onMouseEnter={props.onEnter} onMouseLeave={props.onLeave}>
      <box style={{ flexDirection: "row" }}>
        <text className="uii-sel">{props.head}</text>
        <text className="uii-brace" style={{ paddingLeft: 4 }}>{"{"}</text>
      </box>
      {props.children}
      <text className="uii-brace">{"}"}</text>
    </box>
  );
}

/** The row that adds a property: a name that completes from the supported ones, then a value. */
function UiInspAdd(props: { id: number }) {
  // 0: nothing showing (the block looks empty), 1: typing a property name, 2: typing its value.
  const [stage, setStage] = useState(0);
  const [prop, setProp] = useState("");
  const [val, setVal] = useState("");
  const [err, setErr] = useState("");
  const stageNow = useRef(0);
  const propBox = useRef<PlatypusElement | null>(null);
  const valBox = useRef<PlatypusElement | null>(null);
  const go = (s: number): void => {
    stageNow.current = s;
    setStage(s);
  };
  useEffect(() => {
    if (stage === 1) propBox.current?.focus();
    if (stage === 2) valBox.current?.focus();
  }, [stage]);
  // The line is done: a property with a value is set; the entry is cleared either way.
  const finish = (): void => {
    if (prop.trim() !== "" && val.trim() !== "") UiInspector.setStyle(props.id, prop.trim(), val.trim());
    setProp("");
    setVal("");
  };
  if (stage === 0) {
    // Clicking in the empty block starts a line, as in a browser's devtools.
    return <box className="uii-row uii-add" onClick={() => go(1)} />;
  }
  const propWords = stage === 1 ? uiValSuggest(prop, UiInspector.styleProps(), 6) : [];
  const valWords = stage === 2 && val.indexOf(" ") < 0 ? uiValSuggest(val, UiInspector.styleKeywords(prop.trim()), 6) : [];
  return (
    <box className="uii-row" style={{ alignItems: "flex-start" }}>
      {stage === 1 ? (
        <box style={{ flexDirection: "column", position: "relative", zIndex: err !== "" ? 100 : 0 }}>
          <Input
            ref={propBox}
            className="uii-field"
            value={prop}
            style={{ width: Math.max(90, prop.length * 7 + 24) }}
            onChange={(e) => {
              setErr("");
              // A whole declaration pasted (`color: red`): the name is checked, the value goes in its place.
              const at = e.value.indexOf(":");
              if (at > 0) {
                const name = UiInspProp(e.value.substring(0, at));
                if (name !== "") {
                  setProp(name);
                  setVal(e.value.substring(at + 1).trim());
                  go(2);
                } else {
                  setProp(e.value.substring(0, at));
                  setErr(UiInspPropError(e.value.substring(0, at)));
                }
              } else {
                setProp(e.value);
              }
            }}
            onKeyDown={(e: PlatypusKeyboardEvent) => {
              if (e.key === "Enter" || e.key === ":" || (e.key === "Tab" && propWords.length === 0)) {
                // `:` (like Enter) moves on to the value when the name is a property, else says why not.
                e.preventDefault();
                if (prop.trim() === "") {
                  if (e.key !== ":") go(0);
                } else if (UiInspProp(prop) !== "") {
                  setProp(UiInspProp(prop));
                  setErr("");
                  go(2);
                } else {
                  setErr(UiInspPropError(prop));
                }
              } else if (e.key === "Tab" && propWords.length > 0) {
                e.preventDefault();
                setProp(propWords[0]!);
              } else if (e.key === "Escape") {
                setProp("");
                go(0);
              }
            }}
            onBlur={() => {
              if (stageNow.current === 1) {
                setProp("");
                setErr("");
                go(0);
              }
            }}
          />
          <UiInspSuggest
            items={propWords}
            onPick={(w) => {
              setProp(w);
              go(2);
            }}
          />
          {/* After the input: a child inserted before it would move it and remount it, which loses the focus. */}
          {err !== "" ? <UiInspError text={err} /> : null}
        </box>
      ) : (
        <text className="uii-prop">{prop.trim() + ":"}</text>
      )}
      {stage === 2 ? (
        <box style={{ flexDirection: "column" }}>
          <Input
            ref={valBox}
            className="uii-field"
            value={val}
            style={{ width: Math.max(110, val.length * 7 + 24) }}
            onChange={(e) => setVal(e.value)}
            onKeyDown={(e: PlatypusKeyboardEvent) => {
              if (e.key === "Enter") {
                e.preventDefault();
                // Done: the property is set and a new line starts.
                finish();
                go(1);
              } else if (e.key === "Tab" && valWords.length > 0) {
                e.preventDefault();
                setVal(valWords[0]!);
              } else if (e.key === "Escape") {
                setProp("");
                setVal("");
                go(0);
              }
            }}
            onBlur={() => {
              if (stageNow.current === 2) {
                finish();
                go(0);
              }
            }}
          />
          <UiInspSuggest
            items={valWords}
            onPick={(w) => {
              setVal(w);
            }}
          />
        </box>
      ) : null}
    </box>
  );
}

// --- Node header: states, classes, pseudo-elements -------------------------------------------------

const UiInspStates = ["hover", "active", "focus", "focus-visible"];

/** Text that acts: a hand cursor, underlined when hovered or on. */
function UiInspLink(props: { label: string; on?: boolean; gone?: boolean; add?: boolean; onClick: () => void }) {
  const cls = "uii-link" + (props.on === true ? " on" : "") + (props.gone === true ? " gone" : "") + (props.add === true ? " add" : "");
  return (
    <box onClick={props.onClick}>
      <text className={cls}>{props.label}</text>
    </box>
  );
}

/**
 * The `+` that adds a class: it opens a pill-shaped box (at least ten characters wide, with the caret
 * in it) that completes from the classes the stylesheets name; Enter adds, Escape or leaving closes it.
 */
function UiInspAddClass(props: { id: number }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const box = useRef<PlatypusElement | null>(null);
  useEffect(() => {
    if (open) box.current?.focus();
  }, [open]);
  if (!open) return <UiInspLink label="+" add={true} onClick={() => setOpen(true)} />;
  const words = UiInspector.classNames();
  const suggestions = uiValSuggest(text.startsWith(".") ? text.substring(1) : text, words, 6);
  const add = (s: string): void => {
    const name = s.trim().startsWith(".") ? s.trim().substring(1) : s.trim();
    if (name !== "") UiInspector.setClass(props.id, name, true);
    setText("");
    setOpen(false);
  };
  return (
    <box style={{ flexDirection: "column" }}>
      <Input
        ref={box}
        className="uii-field uii-pill"
        value={text}
        placeholder="class name"
        style={{ width: Math.max(92, text.length * 7 + 26) }}
        onChange={(e) => setText(e.value)}
        onKeyDown={(e: PlatypusKeyboardEvent) => {
          if (e.key === "Enter") add(text);
          else if (e.key === "Escape") {
            setText("");
            setOpen(false);
          } else if (e.key === "Tab" && suggestions.length > 0) {
            e.preventDefault();
            setText(suggestions[0]!);
          }
        }}
        onBlur={() => {
          setText("");
          setOpen(false);
        }}
      />
      <UiInspSuggest items={suggestions} onPick={(w) => add(w)} />
    </box>
  );
}

/**
 * The node's classes as links (a click takes one off, a click on a struck-through one puts it back), the
 * `+` that adds one, and the interaction states to force (`:hover` ...) as links too.
 */
function UiInspNodeBar(props: { id: number }) {
  const id = props.id;
  const forced = " " + UiInspector.forcedStates(id) + " ";
  const classes = UiInspector.className(id) === "" ? [] : UiInspector.className(id).split(" ");
  // The classes taken off in this panel, so a click can bring one back.
  const [gone, setGone] = useState<string[]>([]);
  useEffect(() => {
    setGone([]);
  }, [id]);
  return (
    <box style={{ flexDirection: "column", gap: 3 }}>
      <box style={{ flexDirection: "row", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
        <text className="uii-dim" style={{ width: 54 }}>classes</text>
        {classes.map((c) => (
          <UiInspLink
            key={c}
            label={"." + c}
            onClick={() => {
              setGone(gone.concat([c]));
              UiInspector.setClass(id, c, false);
            }}
          />
        ))}
        {gone
          .filter((c) => classes.indexOf(c) < 0)
          .map((c) => (
            <UiInspLink
              key={"gone-" + c}
              label={"." + c}
              gone={true}
              onClick={() => {
                setGone(gone.filter((x) => x !== c));
                UiInspector.setClass(id, c, true);
              }}
            />
          ))}
        <UiInspAddClass id={id} />
      </box>
      <box style={{ flexDirection: "row", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
        <text className="uii-dim" style={{ width: 54 }}>force</text>
        {UiInspStates.map((s) => (
          <UiInspLink key={s} label={":" + s} on={forced.indexOf(" " + s + " ") >= 0} onClick={() => UiInspector.forceState(id, s, forced.indexOf(" " + s + " ") < 0)} />
        ))}
      </box>
    </box>
  );
}

// --- Rules you write -------------------------------------------------------------------------------

let uiInspRuleSheet = 0;
const uiInspUserRules: string[] = [];

/** Applies the rules typed in the inspector as one stylesheet of the program. */
function uiInspApplyRules(): void {
  if (uiInspRuleSheet !== 0) removeStyleSheet(uiInspRuleSheet);
  uiInspRuleSheet = uiInspUserRules.length > 0 ? addStyleSheet(uiInspUserRules.join("\n")) : 0;
}

/** Whether `text` looks like a rule: a selector, then declarations in braces. */
function uiInspIsRule(text: string): boolean {
  const open = text.indexOf("{");
  const close = text.lastIndexOf("}");
  return open > 0 && close > open && text.substring(0, open).trim() !== "";
}

/**
 * A field for a CSS rule typed out (`.card:hover { color: red }`, `.card::before { content: "" }`): it is
 * added to the program's styles at once, pseudo-classes and pseudo-elements included. The rules written so
 * far are listed under it to edit or remove.
 */
function UiInspUserRules() {
  const [text, setText] = useState("");
  const [bad, setBad] = useState(false);
  return (
    <box style={{ flexDirection: "column", marginTop: 4 }}>
      <text className="uii-section">NEW RULE</text>
      <box className="uii-row">
        <UiInspField
          boxed={true}
          value={text}
          placeholder=".selector:hover { color: red }"
          width={300}
          onCommit={(s) => {
            if (s.trim() === "") return;
            if (!uiInspIsRule(s)) {
              setBad(true);
              setText(s);
              return;
            }
            setBad(false);
            uiInspUserRules.push(s.trim());
            uiInspApplyRules();
            setText("");
          }}
        />
      </box>
      {bad ? <text className="uii-x-text" style={{ paddingLeft: 18 }}>write it as  selector &#123; property: value &#125;</text> : null}
      {uiInspUserRules.map((r, i) => (
        <box key={i} className="uii-row" style={{ marginTop: 2 }}>
          <UiInspField
            boxed={true}
            value={r}
            width={300}
            onCommit={(s) => {
              if (s.trim() === "") uiInspUserRules.splice(i, 1);
              else if (uiInspIsRule(s)) uiInspUserRules[i] = s.trim();
              else return;
              uiInspApplyRules();
              setText("");
            }}
          />
          <UiInspX
            onClick={() => {
              uiInspUserRules.splice(i, 1);
              uiInspApplyRules();
              setText("");
            }}
          />
        </box>
      ))}
    </box>
  );
}

/**
 * A node's styles as CSS to paste elsewhere: `element.style`, then each matching rule (highest priority
 * first, pseudo-element rules too), with property names as CSS writes them (`background-color`).
 * Declarations something else overrides are kept as comments.
 */
export function UiInspCssText(id: number): string {
  let out = "";
  const block = (head: string, rows: string[]): void => {
    out += head + " {\n" + rows.join("") + "}\n\n";
  };
  const inline: string[] = [];
  for (let i = 0; i < UiInspector.inlineCount(id); i++) {
    const row = uiValKebab(UiInspector.inlineProp(id, i)) + ": " + UiInspector.inlineValue(id, i) + ";";
    inline.push(UiInspector.inlineApplied(id, i) ? "  " + row + "\n" : "  /* " + row + " */\n");
  }
  if (inline.length > 0) block("element.style", inline);
  // The element's own rules, then the pseudo-elements', each highest priority first (as the card lists them).
  for (const pseudoRules of [false, true]) {
    for (let r = UiInspector.ruleCount(id) - 1; r >= 0; r--) {
      if ((UiInspector.rulePseudo(id, r) !== "") !== pseudoRules) continue;
      const rows: string[] = [];
      for (let d = 0; d < UiInspector.declCount(id, r); d++) {
        const row = uiValKebab(UiInspector.declProp(id, r, d)) + ": " + UiInspector.declValue(id, r, d) + (UiInspector.declImportant(id, r, d) ? " !important" : "") + ";";
        rows.push(UiInspector.declApplied(id, r, d) ? "  " + row + "\n" : "  /* " + row + " */\n");
      }
      block(UiInspector.ruleSelector(id, r), rows);
    }
  }
  return out.trim() + "\n";
}

/** A value as a JSX literal: a bare number stays one, anything else is a quoted string. */
function uiInspJsxValue(v: string): string {
  const n = uiValNumbers(v);
  if (n.length === 1 && n[0]!.unit === "" && uiValFormatNumber(n[0]!.num) === v) return v;
  return "\"" + v.split("\\").join("\\\\").split("\"").join("\\\"") + "\"";
}

/**
 * A node's styles as a JSX `style` prop: what is in effect on the element itself (rules lowest priority
 * first, then `element.style`; pseudo-element rules and overridden declarations left out).
 */
export function UiInspJsxText(id: number): string {
  const names: string[] = [];
  const values: string[] = [];
  const put = (prop: string, value: string): void => {
    const at = names.indexOf(prop);
    if (at >= 0) values[at] = value;
    else {
      names.push(prop);
      values.push(value);
    }
  };
  for (let r = 0; r < UiInspector.ruleCount(id); r++) {
    if (UiInspector.rulePseudo(id, r) !== "") continue;
    for (let d = 0; d < UiInspector.declCount(id, r); d++) {
      if (UiInspector.declApplied(id, r, d)) put(UiInspector.declProp(id, r, d), UiInspector.declValue(id, r, d));
    }
  }
  for (let i = 0; i < UiInspector.inlineCount(id); i++) {
    if (UiInspector.inlineApplied(id, i)) put(UiInspector.inlineProp(id, i), UiInspector.inlineValue(id, i));
  }
  let out = "style={{\n";
  for (let i = 0; i < names.length; i++) {
    const key = names[i]!.startsWith("--") ? "\"" + names[i] + "\"" : names[i]!;
    out += "  " + key + ": " + uiInspJsxValue(values[i]!) + ",\n";
  }
  return out + "}}\n";
}

/**
 * A node's styles: with `edit`, its states, classes and pseudo-element tabs, a flexbox editor for a flex
 * container, then `element.style` (inline) and the matching rules, highest priority first, overridden
 * declarations struck through; values are fields and a row adds a property (the edits are inline
 * overrides of the node).
 */
export function UiInspStyles(props: { id: number; edit: boolean }) {
  const { id, edit } = props;
  const [flexOpen, setFlexOpen] = useState(false);
  // Matching rules, highest priority first: the element's own, then each pseudo-element's under a
  // heading. The highest-priority rule that sets a property stands for its inline override.
  const rules: ReactNode[] = [];
  const claimed: string[] = [];
  const pseudos: string[] = [];
  for (let i = UiInspector.ruleCount(id) - 1; i >= 0; i--) {
    const p = UiInspector.rulePseudo(id, i);
    if (p !== "" && pseudos.indexOf(p) < 0) pseudos.push(p);
  }
  for (const group of [""].concat(pseudos)) {
    if (group !== "") rules.push(<text key={"g-" + group} className="uii-group">{"Pseudo ::" + group + " element"}</text>);
    for (let i = UiInspector.ruleCount(id) - 1; i >= 0; i--) {
      if (UiInspector.rulePseudo(id, i) !== group) continue;
      const mine: string[] = [];
      for (let j = 0; j < UiInspector.declCount(id, i); j++) {
        const p = UiInspector.declProp(id, i, j);
        // A pseudo-element's declarations never stand for the element's own override.
        if (group === "" && claimed.indexOf(p) < 0) {
          claimed.push(p);
          mine.push(p);
        }
      }
      rules.push(<UiInspRule key={group + i} id={id} rule={i} edit={edit} claims={mine} />);
    }
  }
  const inline: ReactNode[] = [];
  for (let i = 0; i < UiInspector.inlineCount(id); i++) {
    inline.push(
      <UiInspLine
        key={i}
        id={id}
        prop={UiInspector.inlineProp(id, i)}
        override=""
        value={UiInspector.inlineValue(id, i)}
        computed={UiInspector.inlineComputed(id, i)}
        suffix=""
        applied={UiInspector.inlineApplied(id, i)}
        edit={edit}
        removable={true}
      />,
    );
  }
  const isFlex = UiInspector.layout(id, "display") === "flex";
  return (
    <box style={{ flexDirection: "column" }}>
      {edit ? <UiInspNodeBar id={id} /> : null}
      {edit && isFlex ? (
        <box style={{ flexDirection: "column", marginTop: 3 }}>
          <box style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <text className="uii-dim" style={{ width: 54 }}>layout</text>
            <UiInspLink label="flexbox" on={flexOpen} onClick={() => setFlexOpen(!flexOpen)} />
          </box>
          {flexOpen ? <UiInspFlexEditor id={id} /> : null}
        </box>
      ) : null}
      <box style={{ height: 8 }} />
      {inline.length > 0 || edit ? (
        <UiInspBlock head="element.style" onEnter={() => UiInspector.highlightNode(id)} onLeave={() => UiInspector.clearHighlight()}>
          {inline}
          {edit ? <UiInspAdd id={id} /> : null}
        </UiInspBlock>
      ) : null}
      {rules}
      {edit ? <UiInspUserRules /> : null}
    </box>
  );
}
