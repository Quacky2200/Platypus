# canvas-view

A 3D view inside a React layout. A camera draws into a `<canvas>` element (`Camera.targetCanvas`) instead of the
screen: its render texture is the element's box at the window's pixel scale, made again when the layout changes
it (the button resizes the sidebar, so the view). The sidebar is ordinary screen UI; the canvas's rounded corners
clip the view like any image.

APIs used: `Camera.targetCanvas`, `useRef` on a `<canvas>`, `GameObject`, `MeshRenderer`, `Light`, `register`.

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/canvas-view
```

Then, from the repo root (`cd ..`):

```bash
cargo run --release -p platypus_app -- examples/canvas-view
```

## Things to try

- Put two canvases side by side with a camera each.
- Not yet: the pointer and keys in the canvas don't reach the view (TODO.md, the canvas UI element's "Input").
