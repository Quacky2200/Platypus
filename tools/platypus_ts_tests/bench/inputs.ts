// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Keyboard, mouse and gamepad input after the engine delivered events and ticked, compiled against
// the native API and run in bun against js/engine/src/input.ts.

function f(x: number): string {
  return x.toFixed(3);
}

export function keys(): string {
  return `${input.isKeyDown("KeyW")}${input.getKey("Space")}${input.getKey("KeyD")}|${input.getKeyDown("KeyW")}${input.getKeyDown("Space")}${input.getKeyDown("KeyD")}|${input.getKeyUp("Space")}${input.getKeyUp("KeyW")}`;
}

export function mouse(): string {
  return `${f(input.mousePosition.x)},${f(input.mousePosition.y)}|${f(input.mouseDelta.x)},${f(input.mouseDelta.y)}|${f(input.mouseScrollDelta.x)},${f(input.mouseScrollDelta.y)}|${input.getMouseButton(0)}${input.getMouseButton(1)}${input.getMouseButton(3)}|${input.getMouseButtonDown(0)}${input.getMouseButtonDown(3)}|${input.getMouseButtonUp(0)}`;
}

export function axes(): string {
  return `${f(input.getAxisRaw("Horizontal"))},${f(input.getAxisRaw("Vertical"))},${f(input.getAxisRaw("Mouse X"))},${f(input.getAxisRaw("Mouse Y"))},${f(input.getAxisRaw("Mouse ScrollWheel"))},${f(input.getAxisRaw("Nope"))}`;
}

export function buttons(): string {
  return `${input.getButton("Jump")}${input.getButton("Fire1")}${input.getButton("Fire2")}${input.getButton("Submit")}${input.getButton("Cancel")}|${input.getButtonDown("Jump")}${input.getButtonDown("Fire1")}${input.getButtonDown("Submit")}|${input.getButton("Nope")}`;
}

export function pads(): string {
  let s = `${input.gamepadCount()}`;
  for (let i = 0; i < input.gamepadCount(); i++) {
    const id = input.gamepadId(i);
    s += `;${id}=${input.gamepadName(id)}:${f(input.gamepadButton(id, "South"))},${f(input.gamepadButton(id, "East"))},${f(input.gamepadButton(id, "RightTrigger"))},${f(input.gamepadAxis(id, "LeftStickX"))},${f(input.gamepadAxis(id, "LeftStickY"))}`;
  }
  s += `;${input.gamepadConnected(0)}${input.gamepadConnected(1)}${input.gamepadConnected(7)};${input.isButtonDown("South")}${input.isButtonDown("East")}${input.isButtonDown("RightTrigger")}`;
  return s;
}
