# hello

The smallest Platypus game: it logs a greeting once and "tick" about every frame from a timer. It also shows
how a game splits code into modules: `src/main.ts` imports a function from `src/modules/greet.ts` by its
bare name. There is no UI and no scene; it is a check that the build, the module graph and timers work, and
it runs on the light `ui` renderer (no `GameObject` or component, see "Things to try").

APIs used: `console.log`, `setInterval`, and the `modules/` folder import rule of the module graph
(`docs/platypus-build.md`, "Module graph").

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/hello
```

Then, from the repo root (`cd ..`):

```bash
cargo run -p platypus_app -- examples/hello
```

A screenshot run (`cargo run -p platypus_app -- examples/hello --screenshot out.png --frames 30`) renders an empty
window and exits. To read the output, start `platypus-app` from a terminal: the log goes to stderr (and to
`Player.log` next to the game's `prefs.json` for a normal run). You can also build one executable with
`platypus-build package ../examples/hello` (see `docs/platypus-build.md`, "Packaging a single executable").

## Structure

| File | What |
|---|---|
| `project.json` | `{ "name": "hello", "title": "Platypus - Hello" }`: only `name` is required; everything else takes its default |
| `tsconfig.json` | The default of `platypus-build init`: `paths` maps bare names to `./src/modules/*` for `tsc` |
| `src/main.ts` | The entry (a `.ts` file, no JSX): the greeting and the timer |
| `src/modules/greet.ts` | `greeting(name, times)`, a module of this game |

No assets, scenes or CSS.

## How it works

1. `platypus-build` finds the entry (`src/main.tsx`, `src/main.ts`, ...), follows its imports and compiles the
   whole module graph with web_ui_ts. The import below resolves to `src/modules/greet.ts` because bare
   imports are files of the `modules/` folder next to the entry:

```ts
import { greeting } from "greet";
```

2. `console` and `setInterval` are globals; the example imports no engine names.
3. The top-level code runs once at start, then the timer every 16 ms:

```ts
console.log(greeting("world", 1));

setInterval(() => {
  console.log("tick");
}, 16);
```

`hello world` is printed once, then "tick" about once per frame (a lot of lines; remove it when you reuse
the file).

4. `greeting` builds a string with a template literal and `repeat`, clamping the count to at least 1:

```ts
return `hello ${name}`.repeat(Math.max(1, times));
```

## Controls

None.

## Things to try

- Pass `3` to `greeting` and see the string repeat.
- Add a second module `src/modules/math2.ts` and import it as `"math2"`.
- Make it a component: `class Hello extends Component { override onUpdate(dt: number) { .. } }` and
  `register({ Hello })` run per frame with the frame length (`dt`). A component, a `GameObject` or a scene
  file makes the game a `world2d` one, started on the world renderer (`docs/rendering.md`, "Choosing the
  renderer"); `scenes` and `animation` show components.
- Create a `GameObject` and give it a `MeshRenderer` to get a first 3D object (see `cornell`).

## Gotchas

- Relative imports (`./foo`) work too; the `modules/` folder is only for bare names.
- The game is compiled TypeScript, not run in a JS engine: only the supported subset compiles (see
  `docs/platypus-ts.md`). This example uses nothing outside it.
- `tsc` (optional type check) needs the generated `types/platypus.d.ts`; use `platypus-build check --tsc` or
  `platypus-build types` rather than plain `tsc`, because the engine names are globals.

Related examples: text, boxes, scenes
