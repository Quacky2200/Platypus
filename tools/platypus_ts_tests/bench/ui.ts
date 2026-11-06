// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Compiled UI components for the native reconciler (`web_ui_react` through `web_ui_bridge`): JSX,
// useState, useRef, useMemo, useEffect and keyed lists, written as ordinary TSX.

interface CounterProps {
  start: number;
  label: string;
}

function Counter(props: CounterProps): UiElement {
  const [n, setN] = useState(props.start);
  return (
    <box style={{ width: 100 }} onClick={() => setN(n + 1)}>
      <text>{props.label}</text>
      {n}
    </box>
  );
}

export function counterApp(): UiElement {
  return <Counter start={3} label="n=" />;
}

function List(props: { items: string[] }): UiElement {
  return <box>{props.items.map((s) => <text key={s}>{s}</text>)}</box>;
}

export function listApp(n: number, reverse: boolean): UiElement {
  const items: string[] = [];
  for (let i = 0; i < n; i++) items.push("k" + i);
  if (reverse) items.reverse();
  return <List items={items} />;
}

let log = 0;

function Fx(props: { n: number }): UiElement {
  useEffect(() => {
    log = log * 100 + 10 + props.n;
    return () => {
      log = log * 100 + 20 + props.n;
    };
  }, [props.n]);
  return <box />;
}

export function fxApp(n: number): UiElement {
  return <Fx n={n} />;
}

export function getLog(): number {
  return log;
}

let memoRuns = 0;

function Memo(props: { n: number; other: number }): UiElement {
  const sq = useMemo(() => {
    memoRuns += 1;
    return props.n * props.n;
  }, [props.n]);
  return (
    <box>
      {sq}
      {props.other}
    </box>
  );
}

export function memoApp(n: number, other: number): UiElement {
  return <Memo n={n} other={other} />;
}

export function memoCount(): number {
  return memoRuns;
}

function Toggle(): UiElement {
  const [on, setOn] = useState(false);
  const [text, setText] = useState("off");
  const ref = useRef(0);
  ref.current += 1;
  return (
    <box
      onClick={() => {
        setOn(!on);
        setText(on ? "off" : "on");
      }}
    >
      {text}
      {ref.current}
    </box>
  );
}

export function toggleApp(): UiElement {
  return <Toggle />;
}

const Theme = createContext("light");

function Reader(): UiElement {
  const t = useContext(Theme);
  return <text>{t}</text>;
}

export function ctxApp(dark: boolean): UiElement {
  const value = dark ? "dark" : "light";
  return (
    <Theme.Provider value={value}>
      <Reader />
    </Theme.Provider>
  );
}

export function ctxNone(): UiElement {
  return <Reader />;
}

let seenNode = -2;

function WithRef(): UiElement {
  const r = useRef<UiNode | null>(null);
  useEffect(() => {
    seenNode = Ui.nodeId(r.current);
  }, []);
  return <box ref={r} />;
}

export function refApp(): UiElement {
  return <WithRef />;
}

export function getSeenNode(): number {
  return seenNode;
}

interface Api {
  answer: number;
}

let handleSeen = -1;

function Child(props: { handle: Ref<Api | null> }): UiElement {
  useImperativeHandle(props.handle, (): Api => ({ answer: 42 }), []);
  return <box />;
}

function Parent(): UiElement {
  const h = useRef<Api | null>(null);
  useEffect(() => {
    const api = h.current;
    handleSeen = api === null ? -1 : api.answer;
  }, []);
  return <Child handle={h} />;
}

export function handleApp(): UiElement {
  return <Parent />;
}

export function getHandleSeen(): number {
  return handleSeen;
}

function Plus(): UiElement {
  const [n, setN] = useState(0);
  return (
    <box
      onClick={() => {
        setN((c) => c + 1);
        setN((c) => c + 10);
      }}
    >
      {n}
    </box>
  );
}

export function plusApp(): UiElement {
  return <Plus />;
}

interface BoxStyle {
  width: number;
  color?: string;
  height?: number;
}

function Styled(props: { st: BoxStyle }): UiElement {
  return <box style={props.st} />;
}

export function styledApp(w: number, withColor: boolean): UiElement {
  const st: BoxStyle = withColor ? { width: w, color: "red" } : { width: w };
  return <Styled st={st} />;
}

// M9-TS-3525: the declared types of style properties (`PlatypusStyle["position"]`), though the
// compiled style is a list of entries.
type Pos = PlatypusStyle["position"];

function Placed(props: { pos: Pos; left: PlatypusStyle["left"]; width: PlatypusStyle["width"] }): UiElement {
  const dir: PlatypusStyle["flexDirection"] = "column";
  return (
    <box style={{ position: "relative", flexDirection: dir, width: 100, height: 100 }}>
      <box style={{ width: 10, height: 20 }} />
      <box style={{ position: props.pos, left: props.left, top: 0, width: props.width, height: 10 }} />
    </box>
  );
}

export function placedApp(abs: boolean): UiElement {
  const pos: PlatypusStyle["position"] = abs ? "absolute" : "relative";
  const cursor: PlatypusStyle["cursor"] = "pointer";
  const grow: PlatypusStyle["flexGrow"] = 1;
  const left: PlatypusStyle["left"] = cursor === "pointer" && grow === 1 ? 12 : 0;
  return <Placed pos={pos} left={left} width={abs ? "50%" : "30px"} />;
}

interface Common {
  className: string;
  id: string;
  tabIndex?: number;
}

function Spread(props: { c: Common }): UiElement {
  return <box {...props.c} />;
}

export function spreadApp(): UiElement {
  const c: Common = { className: "a b", id: "main", tabIndex: 3 };
  return <Spread c={c} />;
}

function Inner(props: { a: number; b: number }): UiElement {
  return (
    <box>
      {props.a}
      {props.b}
    </box>
  );
}

function Outer(props: { a: number; b: number }): UiElement {
  return <Inner {...props} b={9} />;
}

export function outerApp(): UiElement {
  return <Outer a={1} b={2} />;
}

let leafCount = 0;

function Leaf(props: { n: number }): UiElement {
  leafCount += 1;
  return <box>{props.n}</box>;
}

const FastLeaf = memo(Leaf);

function MemoParent(props: { n: number; tick: number }): UiElement {
  return (
    <box>
      {props.tick}
      <FastLeaf n={props.n} />
    </box>
  );
}

export function memoParentApp(n: number, tick: number): UiElement {
  return <MemoParent n={n} tick={tick} />;
}

export function leafRenders(): number {
  return leafCount;
}

interface BoxApi {
  size: number;
}

const Fwd = forwardRef((props: { label: string }, ref: Ref<BoxApi | null>): UiElement => {
  useImperativeHandle(ref, (): BoxApi => ({ size: props.label.length }), []);
  return <text>{props.label}</text>;
});

let fwdSeen = -1;

function FwdParent(): UiElement {
  const r = useRef<BoxApi | null>(null);
  useEffect(() => {
    const a = r.current;
    fwdSeen = a === null ? -1 : a.size;
  }, []);
  return <Fwd label="hello" ref={r} />;
}

export function fwdApp(): UiElement {
  return <FwdParent />;
}

export function getFwdSeen(): number {
  return fwdSeen;
}

function counterReducer(s: number, a: string): number {
  return a === "inc" ? s + 1 : s - 1;
}

function RedCounter(): UiElement {
  const [count, dispatch] = useReducer(counterReducer, 10);
  return (
    <box
      onClick={() => dispatch("inc")}
      onPointerDown={() => {
        dispatch("dec");
        dispatch("dec");
      }}
    >
      {count}
    </box>
  );
}

export function reducerApp(): UiElement {
  return <RedCounter />;
}

let store = 5;
const listeners: (() => void)[] = [];

function subscribeStore(cb: () => void): () => void {
  listeners.push(cb);
  return () => {
    const i = listeners.indexOf(cb);
    if (i >= 0) listeners.splice(i, 1);
  };
}

function StoreView(): UiElement {
  const v = useSyncExternalStore(subscribeStore, () => store);
  return <box>{v}</box>;
}

export function storeApp(): UiElement {
  return <StoreView />;
}

export function bumpStore(): void {
  store += 1;
  for (const l of listeners.slice()) l();
}

export function listenerCount(): number {
  return listeners.length;
}

export function radiusApp(ball: boolean): UiElement {
  return <box style={{ width: 50, borderRadius: ball ? "50%" : 4 }} />;
}

export function edgeApp(top: boolean): UiElement {
  const side: "left" | "top" = top ? "top" : "left";
  return <box style={{ position: "absolute", width: 72, height: 30, [side]: 8 }} />;
}

function Sized(props: { extra?: object }) {
  return <box style={{ width: 20, ...props.extra }} />;
}

export function objectStyleApp(more: boolean): UiElement {
  return more ? <Sized extra={{ height: 30 }} /> : <Sized />;
}

// ---- functions that return a value where `() => void` is expected ----

const tapLog: string[] = [];

class Marker {
  n = 0;
}

interface Info {
  id: number;
}

function tapNumber(): number {
  tapLog.push("n");
  return 42;
}

function tapString(): string {
  tapLog.push("s");
  return "x" + tapLog.length;
}

function tapMarker(): Marker {
  tapLog.push("m");
  const m = new Marker();
  m.n = tapLog.length;
  return m;
}

function tapInfo(): Info {
  tapLog.push("o");
  return { id: tapLog.length };
}

function tapElement(): UiElement {
  tapLog.push("e");
  return <text>hi</text>;
}

function ValueHandlers(): UiElement {
  return (
    <box>
      <box onClick={tapNumber} />
      <box onClick={tapString} />
      <box onClick={tapMarker} />
      <box onClick={tapInfo} />
      <box onClick={tapElement} />
      <box onClick={() => tapNumber()} />
      <box onClick={() => tapMarker()} />
    </box>
  );
}

export function valueHandlersApp(): UiElement {
  return <ValueHandlers />;
}

export function tapLogText(): string {
  return tapLog.join("");
}

function runTwice(f: () => void): void {
  f();
  f();
}

/** The same functions as callback arguments, with garbage made between the calls. */
export function valueCallbacks(): string {
  tapLog.length = 0;
  runTwice(tapNumber);
  runTwice(tapString);
  runTwice(tapMarker);
  runTwice(tapInfo);
  runTwice(tapElement);
  runTwice(() => tapMarker());
  runTwice(() => tapInfo());
  return tapLog.join("") + ":" + tapLog.length;
}

// ---- events from the interaction layer (M9-TS-3313..) ----

const evLog: string[] = [];

function note(s: string): void {
  evLog.push(s);
}

/** The notes since the last call, space separated. */
export function evLogText(): string {
  const s = evLog.join(" ");
  evLog.length = 0;
  return s;
}

function relatedId(e: PlatypusFocusEvent): number {
  return e.relatedTarget === null ? 0 : e.relatedTarget.id;
}

function FocusForm(): UiElement {
  return (
    <box
      onFocusCapture={(e: PlatypusFocusEvent) => note("cap-focus:" + e.type)}
      onBlurCapture={(e: PlatypusFocusEvent) => note("cap-blur:" + e.type)}
      onFocus={(e: PlatypusFocusEvent) => note("form-focus:" + e.type)}
      onBlur={(e: PlatypusFocusEvent) => note("form-blur:" + e.type)}
    >
      <input
        onFocus={(e: PlatypusFocusEvent) => note("a-focus:" + relatedId(e))}
        onBlur={(e: PlatypusFocusEvent) => note("a-blur:" + relatedId(e))}
      />
      <input
        onFocus={(e: PlatypusFocusEvent) => note("b-focus:" + relatedId(e))}
        onBlur={(e: PlatypusFocusEvent) => note("b-blur:" + relatedId(e))}
      />
    </box>
  );
}

export function focusApp(): UiElement {
  return <FocusForm />;
}

function Fader(): UiElement {
  const [on, setOn] = useState(false);
  return (
    <box
      style={{ width: 50, height: 50, opacity: on ? 0 : 1, transition: "opacity 1s linear" }}
      onClick={() => setOn(!on)}
      onTransitionRun={(e: PlatypusTransitionEvent) => note("run:" + e.propertyName + ":" + e.elapsedTime)}
      onTransitionStart={(e: PlatypusTransitionEvent) => note("start:" + e.propertyName)}
      onTransitionEnd={(e: PlatypusTransitionEvent) =>
        note("end:" + e.propertyName + ":" + e.elapsedTime + ":" + e.pseudoElement.length)
      }
      onTransitionCancel={(e: PlatypusTransitionEvent) => note("cancel:" + e.propertyName)}
    />
  );
}

export function fadeApp(): UiElement {
  return <Fader />;
}

function Pulser(): UiElement {
  return (
    <box
      className="pulse"
      style={{ width: 50, height: 50 }}
      onAnimationStart={(e: PlatypusAnimationEvent) => note("astart:" + e.animationName)}
      onAnimationIteration={(e: PlatypusAnimationEvent) => note("aiter:" + e.animationName + ":" + e.elapsedTime)}
      onAnimationEnd={(e: PlatypusAnimationEvent) => note("aend:" + e.animationName + ":" + e.elapsedTime)}
      onAnimationCancel={(e: PlatypusAnimationEvent) => note("acancel:" + e.animationName)}
    />
  );
}

export function pulseApp(): UiElement {
  return <Pulser />;
}

function Dragger(): UiElement {
  return (
    <box>
      <box
        draggable
        style={{ position: "absolute", left: 0, top: 0, width: 50, height: 50 }}
        onDragStart={(e: PlatypusDragEvent) => {
          e.dataTransfer.setData("Text/Plain", "hello");
          e.dataTransfer.effectAllowed = "move";
          note("dragstart:" + e.dataTransfer.types.join(","));
        }}
        onDragEnd={(e: PlatypusDragEvent) =>
          note("dragend:" + e.dropped + ":" + e.dataTransfer.getData("text/plain") + ":" + e.dataTransfer.dropEffect)
        }
      />
      <box
        style={{ position: "absolute", left: 100, top: 0, width: 50, height: 50 }}
        onDragEnter={(e: PlatypusDragEvent) => note("enter:" + e.dataTransfer.getData("text/plain"))}
        onDragOver={(e: PlatypusDragEvent) => {
          e.dataTransfer.dropEffect = "move";
          e.preventDefault();
        }}
        onDragLeave={(e: PlatypusDragEvent) => note("leave")}
        onDrop={(e: PlatypusDragEvent) => {
          const names: string[] = [];
          for (let i = 0; i < e.dataTransfer.files.length; i++) names.push(e.dataTransfer.files[i].name);
          note("drop:" + e.dataTransfer.getData("text/plain") + ":" + names.join("+") + ":" + e.dataTransfer.types.join(","));
        }}
      />
    </box>
  );
}

export function dragApp(): UiElement {
  return <Dragger />;
}

function Hover(): UiElement {
  return (
    <box
      style={{ width: 50, height: 50 }}
      onMouseEnter={(e: PlatypusPointerEvent) => note("menter")}
      onMouseLeave={(e: PlatypusPointerEvent) => note("mleave")}
      onMouseMove={(e: PlatypusPointerEvent) => note("mmove")}
    >
      <box style={{ width: 20, height: 20 }} />
    </box>
  );
}

export function hoverApp(): UiElement {
  return <Hover />;
}

function Scroller(): UiElement {
  return (
    <box onScroll={(e: PlatypusScrollEvent) => note("outer-scroll")}>
      <box
        style={{ width: 100, height: 100, overflow: "scroll" }}
        onScroll={(e: PlatypusScrollEvent) => note("scroll:" + e.scrollTop)}
      >
        <box style={{ width: 100, height: 400 }} />
      </box>
    </box>
  );
}

export function scrollApp(): UiElement {
  return <Scroller />;
}

// The DOM's scroll calls on a ref, positional and with option objects.
function ScrollCalls(): UiElement {
  const r = useRef<UiNode | null>(null);
  useEffect(() => {
    const n = r.current;
    if (n !== null) {
      n.scrollIntoView({ block: "center", behavior: "smooth" });
      n.scrollIntoView();
      n.scrollTo({ top: 80 });
      n.scrollTo(1, 2);
      n.scrollBy({ left: 5, behavior: "instant" });
      n.scrollBy(0, -10);
    }
  }, []);
  return <box ref={r} />;
}

export function scrollCallsApp(): UiElement {
  return <ScrollCalls />;
}

// ---- What the md-reader and launcher examples use ----

// A callback ref: called with the element, and with null when it goes (or the function changes).
function CallbackRef(): UiElement {
  const [n, setN] = useState(0);
  return (
    <box onClick={() => setN(n + 1)}>
      {n < 2 ? (
        <box
          ref={(el: UiNode | null) => {
            note(el === null ? "ref:null" : "ref:el");
          }}
        />
      ) : null}
    </box>
  );
}

export function callbackRefApp(): UiElement {
  return <CallbackRef />;
}

// `<Fragment key>` around siblings of a list, and components that return any node.
function Word({ w }: { w: string }): ReactNode {
  if (w === "") return null;
  if (w === "list") return ["x", <text>y</text>];
  return w;
}

function Tree({ depth }: { depth: number }) {
  return depth > 0 ? (
    <box>
      <Tree depth={depth - 1} />
    </box>
  ) : (
    <text>leaf</text>
  );
}

const Shown = memo(function Shown({ n }: { n: number }) {
  return <text>{n}</text>;
});

export function nodesApp(): UiElement {
  return (
    <box>
      {["a", "b"].map((s) => (
        <Fragment key={s}>
          <text>{s}</text>
          <text>!</text>
        </Fragment>
      ))}
      <Word w="hi" />
      <Word w="" />
      <Word w="list" />
      <Tree depth={2} />
      <Shown n={7} />
    </box>
  );
}

// `useSyncExternalStore` over a module store, and `useState` of a string-literal union.
type Theme = "dark" | "light";
let theme: Theme = "dark";
const themeListeners = new Set<() => void>();

export function setTheme(light: boolean): void {
  theme = light ? "light" : "dark";
  for (const f of themeListeners) f();
}

function Themed(): UiElement {
  const t = useSyncExternalStore(
    (f) => {
      themeListeners.add(f);
      return () => themeListeners.delete(f);
    },
    () => theme,
  );
  const [mode, setMode] = useState<"browse" | "details">("browse");
  return (
    <box onClick={() => setMode(mode === "browse" ? "details" : "browse")}>
      <text>{`${t}/${mode}`}</text>
    </box>
  );
}

export function themedApp(): UiElement {
  return <Themed />;
}

// What the editor's panels use of an event: `currentTarget` (the element whose handler runs),
// `defaultPrevented` read after a handler prevented it, and pointer capture through it.
function CaptureBox(): UiElement {
  const outer = useRef<PlatypusElement>(null);
  return (
    <box
      ref={outer}
      onPointerDown={(e: PlatypusPointerEvent) => {
        note(`down:${e.currentTarget === outer.current}`);
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerUp={(e: PlatypusPointerEvent) => e.currentTarget.releasePointerCapture(e.pointerId)}
      onKeyDown={(e: PlatypusKeyboardEvent) => note(`outer:${e.defaultPrevented}`)}
    >
      <box
        tabIndex={0}
        onKeyDown={(e: PlatypusKeyboardEvent) => {
          note(`before:${e.defaultPrevented}`);
          e.preventDefault();
          note(`after:${e.defaultPrevented}`);
        }}
      />
    </box>
  );
}

export function captureApp(): UiElement {
  return <CaptureBox />;
}

// React 19's `ref` prop on a function component (`Ref<T>`, left out by one of them) filled by
// `useImperativeHandle`, and a state of a literal union that may be null.
interface PickerHandle {
  size(): number;
}

function Picker({ ref }: { ref?: Ref<PickerHandle> }): UiElement {
  const [pick, setPick] = useState<"a" | "b" | null>(null);
  useImperativeHandle(ref, () => ({ size: () => 3 }), []);
  return (
    <box onClick={() => setPick(pick === null ? "a" : pick === "a" ? "b" : null)}>
      <text>{pick ?? "none"}</text>
    </box>
  );
}

function PickerParent(): UiElement {
  const h = useRef<PickerHandle>(null);
  useEffect(() => {
    const api = h.current;
    note(`size:${api === null ? -1 : api.size()}`);
  }, []);
  return (
    <box>
      <Picker ref={h} />
      <Picker />
    </box>
  );
}

export function pickerApp(): UiElement {
  return <PickerParent />;
}

// Setters kept past their component (a promise that resolves after the panel closed, as in the
// editor's Project browser): as in React, they do nothing once it has unmounted.
let lateSetters: (() => void)[] = [];

function Late(): UiElement {
  const [items, setItems] = useState<string[]>([]);
  const [n, setN] = useState(0);
  const [s, setS] = useState(() => "x");
  const [r, dispatch] = useReducer(counterReducer, 1);
  lateSetters = [
    () => setItems(["a"]),
    () => setN((v) => v + 1),
    () => setS("y"),
    () => dispatch("inc"),
  ];
  return <text>{`${items.length}${n}${s}${r}`}</text>;
}

function LateParent(): UiElement {
  const [on, setOn] = useState(true);
  return <box onClick={() => setOn(!on)}>{on ? <Late /> : null}</box>;
}

export function lateApp(): UiElement {
  return <LateParent />;
}

export function callLateSetters(): void {
  for (const f of lateSetters) f();
  note("called");
}

// M9-TS-3533: `useState(x)` calls `x` for the first value only when `x` here is a function: a local
// value named like a top-level function is the value itself, a local arrow is called.
function initialLabel(): string {
  return "top";
}

function Shadowed(): UiElement {
  const initialLabel = "local";
  const [a] = useState(initialLabel);
  const read = () => "lazy";
  const [b] = useState(read);
  const [c] = useState(topLabel);
  return <text>{`${a} ${b} ${c}`}</text>;
}

function topLabel(): string {
  return initialLabel();
}

export function shadowApp(): UiElement {
  return <Shadowed />;
}

// M9-TS-3532: `{...(cond ? handlers : {})}` in JSX sets the taken arm's props (`{}` sets none),
// with the arms' handlers made by a function, as the editor's Project browser rows do.
function SpreadRow({ name, live }: { name: string; live: boolean }): UiElement {
  const handlers = (n: string) => ({ onClick: () => note(`click:${n}`), onDoubleClick: () => note(`double:${n}`) });
  return (
    <box {...(live ? handlers(name) : {})}>
      <text>{name}</text>
    </box>
  );
}

export function condSpreadApp(): UiElement {
  return (
    <box>
      <SpreadRow name="a" live={true} />
      <SpreadRow name="b" live={false} />
    </box>
  );
}
