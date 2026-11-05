# UI without React: a native reconciler with the hooks we use

Status: **design for review, no code** (TODO.md M9.b2 "UI without React", a STOP item). Written 2026-10-19,
revised the same day after the user's decision.

**Decision (user, 2026-10-19).** Keep the hooks API the code uses today (`useState`, `useEffect` with cleanup, `useRef`,
`useMemo`, `useCallback`, `useSyncExternalStore`, `useImperativeHandle`, context, keys) and add portals. Implement it
natively with **re-render semantics**: components return plain element descriptions and a small native reconciler diffs
them against the previous tree and emits the `platypus_ui` operations. This replaces `react-reconciler`'s host config and
leaves component code nearly unchanged. Not Preact (dynamic JS: `platypus_ts` cannot compile it) and not run-once signals
(every component would be rewritten); signals can come later as an optional layer. **React stays as it is until the
native version exists.** (The first draft of this note recommended signals; it is replaced by this one.)

## 1. How the UI works today

```
 React 19 + react-reconciler (JS, in QuickJS)
   createInstance / appendChild / commitUpdate ...
        │  queueFor(id).push([OPCODE, ...])           js/react/src/host.ts, style.ts, attrs.ts
        ▼
   resetAfterCommit → __platypus_commit(ops[, target])    ONE call per React commit
        │  Vec<Value> arrays, decoded in Rust
        ▼
 platypus_ui::UiTree::apply_commit(&Commit)               crates/platypus_ui/src/tree.rs
   8 ops: CREATE, SET_STYLE, APPEND_CHILD, INSERT_BEFORE, REMOVE_CHILD, CREATE_TEXT, SET_TEXT, SET_ATTRS,
   then one layout sync; CSS cascade, Taffy, Parley, draw list, hit testing ...
        ▲
        │  Pointer / Interaction build the event and its path (target, then ancestors)
 __platypus_ui_event(type, path, data, strings)           js/react/src/events.ts
   JS does capture/bubble over `on*` props, flushSyncWork, returns `defaultPrevented`
```

Rust already owns the tree. React supplies three things: component state and hooks, the diff (which ops to send),
and the handler tables with capture/bubble. Those three are what this design moves to Rust. Everything below the
op list stays.

## 2. What is actually used (counted in `examples/`, `editor/`, `js/editor-ui`)

| Feature | Uses | Notes |
|---|---|---|
| `useState` | 76 | |
| `useEffect` | 45 | subscriptions, timers, focus, listeners; cleanups matter |
| `useRef` | 29 | node refs (`focus()`, scroll) and mutable boxes |
| `useMemo` | 10 | |
| `useSyncExternalStore` | 5 | log, screen size (`Screen.onChange`) |
| `useCallback` | 2 | |
| `useImperativeHandle` | 2 | `VirtualList` exposes a scroll handle |
| `createContext` / `useContext` | 0 in app code | only the reconciler's internal transition context; still in the decision: keep |
| `Suspense`, `lazy`, `useReducer`, `useLayoutEffect`, `useId`, transitions, `memo`, `forwardRef` | 0 (one `Suspense` hide/unhide path inside `host.ts`) | not built; add `memo` and `forwardRef`/ref-as-prop when a port needs them |
| Elements | `<box>` 168, `<Text>` 76, `<Input>` 6, `<svg>` 3, `<Span>` 3, `<Image>` 3, `<TextArea>` 2 | plus `List`, `Dialog` |
| Lists | 38 `.map(` and 33 `key=` | keyed children are common |
| Generic components | `TreeView<T>`, `VirtualList` props | see section 8 |
| Roots | `createRoot()` (screen) and `createRoot(panel)` (world panels, `examples/world`) | one root per target |
| Fast Refresh | dev builds | see section 7 |

Portals are new (none today); dialogs already use the top layer in `platypus_ui`.

## 3. Avoiding duplication

| Piece | Decision |
|---|---|
| `UiTree` and everything under it | Reused unchanged. |
| The op set | Stays the one mutation interface. Add a typed `Op` enum beside `Commit`, decode the `Value` arrays into it once, and make the applier take `&[Op]`. React keeps sending `Value`s; the native reconciler builds `Op`s directly. One applier, no second tree API. |
| Style/attribute parsing | Reused: the reconciler passes the same raw numbers and CSS strings; Rust parses and reports `UiError`. |
| Event path and `defaultPrevented` | Reused: the path comes from `Pointer`/`Interaction`. Capture/bubble over handlers moves to Rust for native roots (the handler table is part of the reconciler's node record). `events.ts` stays for React roots. |
| Props and event type declarations | One declaration set in `@platypus/ui` used by both paths (the native one is the source; `@platypus/react` re-exports). |
| `js/react` | Untouched until the native version passes the same UI tests and goldens; removed when nothing imports it. |

## 4. The element description

Components return **descriptions**, built by JSX rewritten to builder calls (a source rewrite like `generators.rs`;
oxc already parses TSX). The type has to live inside the compiled subset (no `any`, no open unions, no generics yet):

```ts
class Element {
  kind: number;          // HOST, TEXT, FRAGMENT, COMPONENT, PORTAL, PROVIDER, NONE
  type: string;          // host element name ("box", "text", "input", ...), else ""
  key: string | null;
  props: BoxProps | null;       // host: style, attributes, handlers (below)
  children: Element[];          // normalised (below)
  render: (() => Element) | null;   // COMPONENT: closes over its props
  id: number;                   // component identity (section 7)
  target: UiNode | null;        // PORTAL
}
```

- **Host props** are one class `BoxProps` with every field optional (`className`, `id`, `disabled`, `tabIndex`,
  `data-*` as a string map, `value`, the `on*` handlers as nullable closures, `ref`, `style`). The compiler checks
  prop names and types at the call (a typo is a compile error, an improvement over untyped JSX strings).
- **Component props.** `<Menu items={x} />` is rewritten to `component(Menu_id, key, () => Menu({items: x}))`. The
  props object is an ordinary interface/class value the compiler already types, so components need no generic
  `Component<P>` type; the reconciler stores the closure and compares the *props object identity and fields shallowly*
  only for `memo`.
- **Children** (nodes, strings, numbers, lists, conditionals) are normalised by the rewrite so the type stays
  `Element[]`: a string or number child becomes `text("...")`, an array child becomes a `FRAGMENT`, `cond ? <A/> : null`
  becomes `cond ? <A/> : none()`, and `cond && <A/>` is accepted only when `cond` is boolean-typed (a number or string
  `cond` would render `0`/`""` in React; here it is a compile error that says to write the ternary).
- **Style** values are `number | string` in CSS (`width: 10` and `width: "50%"`). The subset has no general union,
  so `style` is a `PlatypusStyle` class with one `StyleValue` per property (`{ num, str, isNum }`), and an object literal in
  `style={{ ... }}` is rewritten to its constructor calls. A number keeps the meaning it has today (the unit rule in
  `style.ts`). Style objects are created every render, so the diff (section 5) is per property.
- **Event objects** (`PlatypusPointerEvent`, ...) are typed classes with the fields `events.ts` documents.

## 5. The reconciler and the diff

Per root (`Root { target, tree: Instance, scheduled }`), Rust keeps an `Instance` tree parallel to the element tree:

- `HostInstance { type, node_id, props (last), handlers, children: Vec<Instance> }`
- `TextInstance { node_id, text }`
- `ComponentInstance { id, render closure, hooks: Vec<Slot>, child: Instance, context links }`
- `PortalInstance { target, children }`

**Update** (a component's state changed, or its parent re-rendered): run `render`, then `reconcile(old, new_element)`:

1. Same `kind`, `type` (host) or `id` (component) and key → keep the instance and update it in place; otherwise unmount the
   old instance and mount a new one.
2. Host: diff props. Style: compare each `StyleValue` and emit one `SET_STYLE` with the changed properties and `null`
   for removed ones (the existing wire format, so Rust sees no change). Attributes the same with `SET_ATTRS`. Handlers
   are replaced in the instance's table with no op at all (they never cross to Rust today either). `value` on text
   fields keeps the controlled-field restore (the instance's text is compared with the prop after the handler).
3. Children: React's algorithm. Walk both lists while key (or position when unkeyed) and type match; on the first
   mismatch put the remaining old children in a map by key (position index for unkeyed) and match new children from
   it. A kept child that moved gets `INSERT_BEFORE`; new children `CREATE`+`APPEND_CHILD`/`INSERT_BEFORE`; removed
   children `REMOVE_CHILD` (Rust destroys the subtree; the reconciler only emits it for the top node of a removed
   subtree). Moves use the `lastPlacedIndex` rule: simple, linear, at most one move per reordered item.
4. Text: `SET_TEXT` when the string differs.

Bail-outs: a state setter whose new value equals the old (same rule as `Object.is`) schedules nothing; `memo` skips
a component whose props are shallow-equal. Without `memo`, a parent render re-renders its whole subtree, as React does
without `memo`; the cost is bounded by the element tree, not by the UiTree (ops are emitted only for real differences).

**Scheduling.** `setState` marks the component dirty and the root scheduled. Handlers dispatched from Rust flush
synchronously before returning (the discrete behaviour of `flushSyncWork` today); everything else flushes once per
frame, before layout, and after microtasks. Dirty components render top-down in depth order so a parent's re-render
drops a child's own pending update. All ops from one flush are applied as one batch, as `__platypus_commit` is today.
Updates scheduled inside effects run in the same flush loop (bounded; a depth limit reports an infinite update loop).

**Effects and cleanup ordering** (matched to React 19 and checked by a differential test, section 9):

1. Render phase: components run; no side effects.
2. Commit: ops are applied, refs are attached/detached (`ref` cleanup before attaching the new one).
3. Passive effects, after commit: for every component whose deps changed, all its old cleanups run first (in tree
   order, across the whole commit), then all new effects run children before parents, in hook order within a component.
4. Unmount: cleanups of the removed subtree run once, parent before child, before the node is removed from the tree.
5. `useSyncExternalStore` subscribes in the passive phase, re-reads the snapshot after subscribing and after every
   notification, and forces a synchronous re-render when it changed.

## 6. Portals, contexts and world-panel roots

- **Roots.** `createRoot(target)` keeps today's rule: one root per target; the screen is target 0 and a world panel is
  its own `UiTree` with its own `Root`. The reconciler's ops carry the target, exactly like `__platypus_commit(ops, target)`.
- **Portals.** `createPortal(children, node)` is an element with `kind = PORTAL` and a `UiNode` target (a ref to any host
  instance, in this root or another, or a panel root). Its children are mounted as the children of the target node, after
  the target's own children, and unmounted with the portal. Events bubble **through the component tree** as in React
  (a handler on the portal's logical parent sees them): each host instance stores a `logical_parent`, and dispatch walks
  that chain while Rust's path (UiTree ancestors) is used only to find the target. Capture/bubble order is then the
  logical chain, not the UiTree chain, for portal content only; ordinary content is identical either way.
- **Context.** `createContext<T>(default)`, `Provider` (kind `PROVIDER`), `useContext`. A consumer reads the nearest
  provider by walking `ComponentInstance` parents (cached per instance) and subscribes; a changed provider value
  re-renders its consumers directly, skipping unchanged intermediates. Slot kinds as in section 8.

## 7. State across hot reload

Today Fast Refresh (`react-refresh`, a dev-only hook installed before React loads) re-renders components whose code
changed and keeps state where hook signatures match. The native equivalent must exist before React can be dropped for
a dev loop without Bun, and it is simpler because the compiler knows everything:

- Each component gets a **stable id** from the compiler (module path + name) and a **hook signature**: the ordered
  list of hook kinds and their slot types (`state:num`, `state:ptr`, `effect`, `memo`, ...). Both are static data.
- On reload the new code registers its components by id. The reconciler keeps the whole `Instance` tree, swaps each
  component's `render` for the new function, and re-renders. A component whose signature is unchanged keeps its
  slots; one whose signature changed is remounted (its subtree resets), as React Refresh does. Effects re-run for
  every refreshed component with cleanup first.
- Slots live in Rust as `Slot { kind, word, cleanup }`, never in compiled-code memory, so a code swap (function-level
  swap in the dev loop item) cannot invalidate them; pointer slots are GC roots owned by the instance.
- Class and struct layouts of *user* data held in a state slot can change across a reload; a changed struct id in the
  signature remounts that component. (The same constraint as `@property` reflection in the dev loop note.)

This stays under "Dev loop without Bun"; it needs no UI work beyond keeping ids and signatures in the data.

## 8. Generics: `useState<T>` and friends

`platypus_ts` has no generics. What the hooks need is smaller than generics:

- **Slot kinds.** A hook slot holds one 8-byte word plus a kind: `NUM` (f64), `BOOL`, `PTR` (a GC object: string,
  array, object, closure). The Rust reconciler stores and roots slots by kind and never needs `T`.
- **Hooks as compiler intrinsics**, the same route as `getComponent`/`addComponent` (boundary note section 13): the
  compiler knows the argument's IR type at the call site and lowers `useState(0)` to `slot_state_num(...)` returning
  the value and building the setter closure, `useState<string | null>(null)` to `slot_state_ptr`, and so on. `useRef`,
  `useMemo`, `useCallback`, `useImperativeHandle`, `useSyncExternalStore`, `useContext` and `useEffect` (no type at all)
  work the same way. The type argument is inferred from the initial value or taken from an explicit `<T>` (needed for
  `useState<Item | null>(null)`).
- **Updater form** `setX(prev => ...)` is a closure over the same slot kind.
- **Generic components** (`TreeView<T>`, `VirtualList` props) need real generics. Two options: (1) monomorphisation by
  textual specialisation, the template mechanism already used for `Generator$K` and `Promise$K`, generalised to
  functions and classes, one copy per distinct `T` used; (2) in the meantime, port such components with a concrete
  interface (`TreeNode { id, label, children }`) and map at the call site. Recommendation: (2) first, (1) as its own
  compiler item; hooks do not wait for either.
- Until intrinsics land, a hook on a type the compiler cannot place in one of the three slot kinds is a compile error
  naming the type.

## 9. Test strategy

Differential, like the rest of `platypus_ts`: the same TSX components run under **real React with a recording host** in
bun and under the native reconciler; compare (a) the final `UiTree` dump (nodes, attributes, styles), (b) the log of
effect runs and cleanups, in order, (c) the op stream length and kinds for update scenarios (keyed reorders, insert in
the middle, removal, conditional children). Plus: the existing UI goldens and `examples/arcade`/editor panels
re-rendered natively and compared; `PLATYPUS_TS_GC=stress` on everything (instances, slots and closures are roots).

## 10. Migration path

1. `Op` enum and one applier in `platypus_ui`; React path unchanged. (Removes the decode duplication.)
2. The reconciler in Rust with a Rust-closure front end (no compiler): hosts, text, keyed children, state and effect
   slots, scheduling, portals, context; tested against the real-React recorder with the same descriptions written by
   hand in both.
3. Compiler: JSX rewrite, `Element` builders, hook intrinsics (state/ref/memo/callback/effect/sync-store/context/
   imperative-handle), stable ids and hook signatures. Compile and run a counter, a keyed todo list, a portal.
4. Native event dispatch (handler tables, capture/bubble in Rust), controlled fields, refs as `UiNode` handles with
   `focus()`/`blur()`/scroll as `#[ts_api]` methods (replacing the `op("focus", id)` calls).
5. Port the arcade HUD beside its React version; goldens must match. Then `@platypus/editor-ui` (needs the `TreeView<T>`
   decision above), then remove `js/react` when nothing imports it.

## 11. Risks

- Effect timing and cleanup order differ subtly from React's: the differential test in section 9 is the guard; any
  disagreement is resolved in favour of React's observable order.
- Re-rendering whole subtrees every update can be slow for big lists (`VirtualList` already windows; add `memo`).
- Exceptions in render, effects or handlers: native faults take the process down (boundary note section 13, answer 5);
  the reconciler needs a fault policy (log, unmount the failing root, keep the rest of the UI), the same as
  `NativeHost`'s component disable.
- GC: closures in element descriptions, slots, handlers and refs are all roots owned by instances; leaks or early
  frees show up only under `PLATYPUS_TS_GC=stress`, so every reconciler test runs there.

## 12. Open questions (answers change the work, none blocks review)

1. Should portal events bubble through the component tree (React) or the UiTree (DOM)? Recommendation: the component
   tree, as above.
2. Is a compile error for non-boolean `cond && <A/>` acceptable, or should it be allowed with React's `0`/`""`
   rendering? Recommendation: compile error.
3. `memo`, `forwardRef` and `useReducer` are not used today. Add them now (small, cheap) or when a port needs them?
   Recommendation: when needed.
