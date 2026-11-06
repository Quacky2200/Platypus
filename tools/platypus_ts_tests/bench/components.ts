// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Compiled components driven by the Rust host (`web_ui_rt::NativeHost`) with no script engine:
// `class X extends Component` plus `register(X)`. The host creates, updates and destroys them;
// the components move their GameObject through the real scene API.

let awakes = 0;
let destroys = 0;
let order = 0;

class Mover extends Component {
  speed: number = 2;
  ticks: number = 0;

  onAwake(): void {
    awakes++;
  }

  onStart(): void {
    order = order * 10 + 1;
  }

  onUpdate(dt: number): void {
    this.ticks++;
    order = order * 10 + 2;
    const t = this.gameObject!.transform;
    const p = t.localPosition;
    t.localPosition = new Vector3(p.x + this.speed * dt, p.y, p.z);
  }

  onLateUpdate(dt: number): void {
    order = order * 10 + 3;
  }

  onDestroy(): void {
    destroys++;
  }
}

/** Inherits every hook and overrides one. */
class Fast extends Mover {
  onUpdate(dt: number): void {
    super.onUpdate(dt);
    super.onUpdate(dt);
  }
}

class Faulty extends Component {
  onUpdate(dt: number): void {
    throw new Error("boom");
  }
}

/** Only a fixed update: the other hooks cost nothing. */
class Physics extends Component {
  total: number = 0;
  onFixedUpdate(dt: number): void {
    this.total += dt;
    const p = this.gameObject!.transform.localPosition;
    this.gameObject!.transform.localPosition = new Vector3(p.x, this.total, p.z);
  }
}

register(Mover);
register(Fast);
register(Faulty);
register(Physics);

export function awakeCount(): number {
  return awakes;
}

export function destroyCount(): number {
  return destroys;
}

export function hookOrder(): number {
  return order;
}
