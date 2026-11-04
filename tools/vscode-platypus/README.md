# Platypus debugger for VS Code

No code: this only registers the `platypus` debugger type so `launch.json` can attach to a game
running with `platypus-app <game> --dap-port 4711` (VS Code connects straight to the port through
`debugServer`). The adapter is the host itself (`crates/platypus_app/src/dap.rs`); the guide is
`docs/debugger.md`.

Try it without publishing: copy this folder to `~/.vscode/extensions/platypus-debug`, restart VS Code, open the
game's folder, add to `.vscode/launch.json`

```json
{
  "version": "0.2.0",
  "configurations": [
    { "type": "platypus", "request": "attach", "name": "Attach to Platypus game", "debugServer": 4711 }
  ]
}
```

start the game (`platypus-app <game> --dap-port 4711`; it waits for the debugger), set breakpoints in the `.ts` files and press F5.
