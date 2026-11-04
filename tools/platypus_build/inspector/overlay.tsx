// The UI inspector's overlay: added to a program by `platypus-build build --add-ui-inspector`
// (appended to the bundle after `values.ts` and `shared.tsx`, mounted in its own root by
// `createRoot(9000001)`, the host's inspector target). Names start with `UiInsp` so they never meet a game's.
//   F12  turns the inspector on and off; a small dark toolbar (top centre, with icons that name themselves
//        on hover, and a grip to drag it anywhere) shows while it is on.
//   Inspect (Ctrl+Shift+C, F7)  the toolbar's first icon, as in Chrome: move over the page to see each element's box
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

/** Whether Ctrl and Shift are both held. */
function UiInspCtrlShift(): boolean {
  const ctrl = input.isKeyDown("ControlLeft") || input.isKeyDown("ControlRight");
  const shift = input.isKeyDown("ShiftLeft") || input.isKeyDown("ShiftRight");
  return ctrl && shift;
}

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

/** The keys, for the `?` panel: what to press, what it does. */
const UiInspKeys = [
  ["F12", "Turn the inspector on or off"],
  ["Ctrl+Shift+C  or  F7", "Inspect: click an element to edit it, as in Chrome"],
  ["Esc", "Stop inspecting"],
  ["Enter", "Commit what you typed; in a new property, the value after the name"],
  ["Esc  (in a field)", "Put the old value back"],
  [":  (in a new property)", "Check the name and go to the value"],
  ["Tab", "Take the first completion"],
  ["Up / Down", "Add or take 1 in a number (Shift: 10, Alt: 0.1)"],
  ["Backspace  (empty field)", "Go back to the name, then remove the property"],
];

/** What the pieces of the card do, for the `?` panel: the thing, what it does. */
const UiInspThings = [
  ["Inspect (toolbar)", "Pick an element on the page and edit its styles"],
  ["Title bar, grip", "Drag to move the toolbar or the card"],
  ["Colour swatch", "Open a colour picker (hex, rgb or hsl)"],
  ["curve, stops", "Edit a timing curve or a gradient"],
  ["flexbox", "A layout editor for a flex container"],
  ["Classes", "Click one to take it off (again to put it back); + adds one"],
  [":hover  :active …", "Force a state on the element"],
  ["A rule { … }", "Hover it to outline the elements it matches"],
  ["margin, padding …", "Hover the line to see the box model"],
  ["NEW RULE", "Type  .a:hover { color: red }  to add a real rule"],
  ["Menu (three bars)", "Copy styles, choose the card's colour scheme"],
];

/** A table for the help: a heading, then rows of two columns that line up. */
function UiInspHelpTable(props: { title: string; left: string; right: string; rows: string[][] }) {
  return (
    <box style={{ flexDirection: "column", gap: 5 }}>
      <text className="uii-section">{props.title}</text>
      <box className="uii-help-grid">
        <text className="uii-dim">{props.left}</text>
        <text className="uii-dim">{props.right}</text>
        {props.rows.map((row) => (
          <Fragment key={row[0]}>
            <text className="uii-help-key">{row[0]}</text>
            <text className="uii-text">{row[1]}</text>
          </Fragment>
        ))}
      </box>
    </box>
  );
}

// --- Icons ---------------------------------------------------------------------------------------------

// The icons take their colours from the theme through classes (`.uii-ico-*` in inspector.css), so the
// toolbar follows the system theme, the toolbar's pill and a card's own scheme alike.

/** A box with a cursor over its corner: pick an element. */
function UiInspIconInspect() {
  return (
    <svg viewBox="0 0 20 20" style={{ width: 17, height: 17 }}>
      <path className="uii-ico-s" d="M3.5 9 V3.5 H14.5 V9 M3.5 9 V14.5 H9" strokeWidth={1.3} strokeLinecap="round" strokeLinejoin="round" />
      <path className="uii-ico-cursor" d="M9.2 8.6 V18.2 L11.6 15.9 L13.4 19.2 L15.1 18.4 L13.3 15.1 L16.6 14.9 Z" strokeWidth={0.9} strokeLinejoin="round" />
    </svg>
  );
}

/** A question mark in a circle: the shortcuts. */
function UiInspIconHelp() {
  return (
    <svg viewBox="0 0 20 20" style={{ width: 17, height: 17 }}>
      <circle className="uii-ico-s" cx={10} cy={10} r={7.3} strokeWidth={1.3} />
      <path className="uii-ico-s" d="M7.7 8.2 C7.7 6.1 12.3 6.1 12.3 8.3 C12.3 9.9 10 9.9 10 11.8" strokeWidth={1.3} strokeLinecap="round" />
      <circle className="uii-ico-f" cx={10} cy={14.3} r={0.95} />
    </svg>
  );
}

/** A circle half filled: the theme. */
function UiInspIconTheme() {
  return (
    <svg viewBox="0 0 20 20" style={{ width: 17, height: 17 }}>
      <circle className="uii-ico-s" cx={10} cy={10} r={6.8} strokeWidth={1.3} />
      <path className="uii-ico-f" d="M10 3.2 A6.8 6.8 0 0 0 10 16.8 Z" />
    </svg>
  );
}

/** A thin cross: close. */
function UiInspIconClose() {
  return (
    <svg viewBox="0 0 20 20" style={{ width: 17, height: 17 }}>
      <path className="uii-ico-s" d="M5.2 5.2 L14.8 14.8 M14.8 5.2 L5.2 14.8" strokeWidth={1.1} strokeLinecap="round" />
    </svg>
  );
}

/** Six dots: something to drag. */
function UiInspIconGrip() {
  return (
    <svg viewBox="0 0 10 18" style={{ width: 9, height: 16 }}>
      <circle className="uii-ico-d" cx={3} cy={4} r={1.15} />
      <circle className="uii-ico-d" cx={7} cy={4} r={1.15} />
      <circle className="uii-ico-d" cx={3} cy={9} r={1.15} />
      <circle className="uii-ico-d" cx={7} cy={9} r={1.15} />
      <circle className="uii-ico-d" cx={3} cy={14} r={1.15} />
      <circle className="uii-ico-d" cx={7} cy={14} r={1.15} />
    </svg>
  );
}

// --- Toolbar and cards -----------------------------------------------------------------------------

/** A tool: an icon button whose name (with its shortcut) shows when the pointer rests on it. */
function UiInspTool(props: { tip: string; on?: boolean; close?: boolean; onClick: () => void; children?: ReactNode }) {
  const [hot, setHot] = useState(false);
  const cls = "uii-tool" + (props.on === true ? " on" : "") + (props.close === true ? " close" : "");
  return (
    <box className={cls} onClick={props.onClick} onMouseEnter={() => setHot(true)} onMouseLeave={() => setHot(false)}>
      {props.children}
      {hot ? (
        <box className="uii-tool-tip" style={{ position: "absolute", top: 32, left: 0, pointerEvents: "none" }}>
          <text className="uii-tool-tip-text">{props.tip}</text>
        </box>
      ) : null}
    </box>
  );
}

/**
 * Something to drag by: the pointer is captured while pressed and each move reports how far it went, so a
 * parent that moves by those amounts follows the pointer. The cursor is a move symbol (a fist while held).
 */
function UiInspGrip(props: { className: string; onMove: (dx: number, dy: number) => void; children?: ReactNode }) {
  const [held, setHeld] = useState(false);
  const down = useRef(false);
  const last = useRef([0, 0]);
  return (
    <box
      className={held ? props.className + " drag" : props.className}
      onPointerDown={(e: PlatypusPointerEvent) => {
        down.current = true;
        setHeld(true);
        last.current = [e.clientX, e.clientY];
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e: PlatypusPointerEvent) => {
        if (!down.current) return;
        props.onMove(e.clientX - last.current[0]!, e.clientY - last.current[1]!);
        last.current = [e.clientX, e.clientY];
      }}
      onPointerUp={(e: PlatypusPointerEvent) => {
        down.current = false;
        setHeld(false);
        e.currentTarget.releasePointerCapture(e.pointerId);
      }}
    >
      {props.children}
    </box>
  );
}

/** The toolbar, top centre: a grip to drag it, Inspect, the shortcuts, the theme and close; each tool names itself on hover. */
function UiInspToolbar(props: { picking: boolean; help: boolean; theme: string; onInspect: () => void; onHelp: () => void; onTheme: (theme: string) => void; onClose: () => void }) {
  // Where it has been dragged to, from the top-centre it starts at.
  const at = useRef([0, 0]);
  const [pos, setPos] = useState([0, 0]);
  const themeName = props.theme === "" ? "system" : props.theme;
  return (
    <box style={{ position: "fixed", left: 0, top: 10, width: "100%", flexDirection: "row", justifyContent: "center", pointerEvents: "none" }}>
      <box style={{ position: "relative", left: pos[0], top: pos[1], flexDirection: "column", alignItems: "center", gap: 6, pointerEvents: "none" }}>
        <box className="uii-bar" style={{ pointerEvents: "auto" }}>
          <UiInspGrip
            className="uii-grip"
            onMove={(dx, dy) => {
              at.current = [at.current[0]! + dx, at.current[1]! + dy];
              setPos(at.current);
            }}
          >
            <UiInspIconGrip />
          </UiInspGrip>
          <UiInspTool tip={props.picking ? "Inspecting… click an element (Esc cancels)" : "Inspect (Ctrl+Shift+C)"} on={props.picking} onClick={props.onInspect}>
            <UiInspIconInspect />
          </UiInspTool>
          <UiInspTool tip="Shortcuts" on={props.help} onClick={props.onHelp}>
            <UiInspIconHelp />
          </UiInspTool>
          <UiInspTool tip={"Theme: " + themeName + " (click to change)"} onClick={() => props.onTheme(UiInspNextTheme(props.theme))}>
            <UiInspIconTheme />
          </UiInspTool>
          <UiInspTool tip="Close (F12)" close={true} onClick={props.onClose}>
            <UiInspIconClose />
          </UiInspTool>
        </box>
        {props.help ? (
          <box className="uii-card" style={{ padding: 12, flexDirection: "column", gap: 10, width: 500, pointerEvents: "auto" }}>
            <UiInspHelpTable title="KEYBOARD" left="Press" right="What it does" rows={UiInspKeys} />
            <UiInspHelpTable title="THE INSPECTOR" left="Part" right="What it does" rows={UiInspThings} />
          </box>
        ) : null}
      </box>
    </box>
  );
}

/** Two sheets: copy. (A mid grey, which reads on a light card and on a dark one.) */
function UiInspIconCopy() {
  return (
    <svg viewBox="0 0 20 20" style={{ width: 15, height: 15 }}>
      <rect className="uii-ico-sd" x={7.2} y={7.2} width={9} height={9.6} rx={1.6} strokeWidth={1.3} />
      <path className="uii-ico-sd" d="M5 12.8 V5.2 A1.6 1.6 0 0 1 6.6 3.6 H12.8" strokeWidth={1.3} strokeLinecap="round" />
    </svg>
  );
}

/** Three bars: a menu. */
function UiInspIconMenu() {
  return (
    <svg viewBox="0 0 20 20" style={{ width: 15, height: 15 }}>
      <path className="uii-ico-sd" d="M4 6 H16 M4 10 H16 M4 14 H16" strokeWidth={1.4} strokeLinecap="round" />
    </svg>
  );
}

/** A small icon button for a themed card; its name shows under it, at the right edge, when the pointer rests on it. */
function UiInspMini(props: { tip: string; on?: boolean; onClick: () => void; children?: ReactNode }) {
  const [hot, setHot] = useState(false);
  return (
    <box className={props.on === true ? "uii-mini on" : "uii-mini"} onClick={props.onClick} onMouseEnter={() => setHot(true)} onMouseLeave={() => setHot(false)}>
      {props.children}
      {hot && props.on !== true ? (
        <box className="uii-tool-tip" style={{ position: "absolute", top: 27, right: 0, pointerEvents: "none" }}>
          <text className="uii-tool-tip-text">{props.tip}</text>
        </box>
      ) : null}
    </box>
  );
}

/** A tick (drawn, the font has no check mark) or an empty place the same size. */
function UiInspTick(props: { on: boolean }) {
  return (
    <svg viewBox="0 0 14 14" style={{ width: 12, height: 12 }}>
      {props.on ? <path d="M2.5 7.4 L5.8 10.6 L11.5 3.8" fill="none" stroke="#6cb6ff" strokeWidth={1.7} strokeLinecap="round" strokeLinejoin="round" /> : null}
    </svg>
  );
}

/** A chevron: right when closed, down when open. */
function UiInspChevron(props: { open: boolean }) {
  return (
    <svg viewBox="0 0 14 14" style={{ width: 10, height: 10 }}>
      <path d={props.open ? "M3 5 L7 9.5 L11 5" : "M5 3 L9.5 7 L5 11"} fill="none" stroke="#8a94a3" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/**
 * One line of the card's menu; `off` greys it out (a coming feature), `tick` marks the chosen one of a
 * submenu, `chevron` says there is a submenu (`true`: open).
 */
function UiInspMenuItem(props: { label: string; off?: boolean; tick?: boolean; sub?: boolean; hint?: string; chevron?: boolean; onClick: () => void }) {
  const cls = "uii-menu-item" + (props.off === true ? " off" : "") + (props.sub === true ? " uii-menu-sub" : "");
  return (
    <box className={cls} onClick={props.off === true ? undefined : props.onClick}>
      {props.sub === true ? <UiInspTick on={props.tick === true} /> : null}
      <text className="uii-text">{props.label}</text>
      <box style={{ flexGrow: 1 }} />
      {props.hint !== undefined ? <text className="uii-dim">{props.hint}</text> : null}
      {props.chevron !== undefined ? <UiInspChevron open={props.chevron} /> : null}
    </box>
  );
}

/**
 * The card of the element being edited. The header has a grip and the title to drag it by, Copy styles, a
 * menu and close; the styles scroll under it. The menu has a colour scheme of its own (match the toolbar,
 * light or dark) and Detach, which is not built yet.
 */
function UiInspCard(props: { id: number; onClose: () => void }) {
  const id = props.id;
  const at = useRef([0, 0]);
  const [pos, setPos] = useState([0, 0]);
  const [menu, setMenu] = useState(false);
  const [schemeOpen, setSchemeOpen] = useState(false);
  // "" matches the toolbar's theme; "light" and "dark" are the card's own.
  const [scheme, setScheme] = useState("");
  const [copied, setCopied] = useState(false);
  const [copyMenu, setCopyMenu] = useState(false);
  const move = (dx: number, dy: number): void => {
    at.current = [at.current[0]! + dx, at.current[1]! + dy];
    setPos(at.current);
  };
  const copy = (jsx: boolean): void => {
    UiInspector.copy(jsx ? UiInspJsxText(id) : UiInspCssText(id));
    setCopied(true);
    setCopyMenu(false);
    setTimeout(() => setCopied(false), 1400);
  };
  const cls = "uii-card" + (scheme === "" ? "" : " uii-scheme-" + scheme);
  return (
    <box
      className={cls}
      style={{
        position: "fixed",
        left: 10 + pos[0]!,
        bottom: 10 - pos[1]!,
        maxHeight: "74%",
        // A fixed width, so a long doc in the foot strip wraps instead of widening the card.
        width: 440,
        flexDirection: "column",
        pointerEvents: "auto",
      }}
    >
      {/* The header is raised over the body, so the menu and the tooltips that hang from it are on top of (and
          take the clicks before) the rows below. */}
      <box className="uii-head" style={{ margin: "10px 10px 6px 10px", position: "relative", zIndex: 100 }}>
        <UiInspGrip className="uii-card-grip" onMove={move}>
          <UiInspIconGrip />
        </UiInspGrip>
        <UiInspGrip className="uii-title-bar" onMove={move}>
          <text className="uii-title">{UiInspName(id)}</text>
        </UiInspGrip>
        <UiInspMini tip={copied ? "Copied" : "Copy styles"} on={copied || copyMenu} onClick={() => setCopyMenu(!copyMenu)}>
          <UiInspIconCopy />
        </UiInspMini>
        <UiInspMini tip="Menu" on={menu} onClick={() => setMenu(!menu)}>
          <UiInspIconMenu />
        </UiInspMini>
        <UiInspMini tip="Close" onClick={props.onClose}>
          <UiInspIconClose />
        </UiInspMini>
        {copyMenu ? (
          <box className="uii-menu" style={{ position: "absolute", top: 30, right: 56, flexDirection: "column", pointerEvents: "auto" }}>
            <UiInspMenuItem label="Copy CSS" hint="element.style, rules" onClick={() => copy(false)} />
            <UiInspMenuItem label="Copy JSX" hint="style={{ … }}" onClick={() => copy(true)} />
          </box>
        ) : null}
        {menu ? (
          <box className="uii-menu" style={{ position: "absolute", top: 30, right: 0, flexDirection: "column", pointerEvents: "auto" }}>
            <UiInspMenuItem label="Copy CSS" onClick={() => { copy(false); setMenu(false); }} />
            <UiInspMenuItem label="Copy JSX" onClick={() => { copy(true); setMenu(false); }} />
            <UiInspMenuItem label="Colour scheme" chevron={schemeOpen} onClick={() => setSchemeOpen(!schemeOpen)} />
            {schemeOpen ? (
              <box style={{ flexDirection: "column" }}>
                <UiInspMenuItem sub={true} tick={scheme === ""} label="Match toolbar" onClick={() => setScheme("")} />
                <UiInspMenuItem sub={true} tick={scheme === "light"} label="Light" onClick={() => setScheme("light")} />
                <UiInspMenuItem sub={true} tick={scheme === "dark"} label="Dark" onClick={() => setScheme("dark")} />
              </box>
            ) : null}
            <box className="uii-menu-sep" />
            <UiInspMenuItem off={true} label="Detach to window" hint="soon" onClick={() => {}} />
          </box>
        ) : null}
      </box>
      <box style={{ padding: "0 10px 6px 10px", flexDirection: "column", overflow: "auto", flexShrink: 1, minHeight: 0 }}>
        <UiInspStyles id={id} edit={true} />
      </box>
      <UiInspDocLine />
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
      } else if (e.code === "F7" || (e.code === "KeyC" && UiInspCtrlShift())) {
        // Ctrl+Shift+C, as in Chrome (F7 too): turns the inspector on and starts inspecting.
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
