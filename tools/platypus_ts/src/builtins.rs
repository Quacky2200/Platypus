// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the compiler itself gives a program, beyond the engine manifests: the component base class,
//! `register` and `@property`, timers, the generic `GameObject` members, the typed window
//! messages, `Random.pick<T>`, and a few members whose script type is wider than the native one.
//! One entry per declaration (its TypeScript signature and doc; decision 1 of
//! docs/research/platypus-generated-dts-plan.md: a raw signature in this table counts as generated
//! from Rust). `types_dts.rs` prints them into `types/platypus.d.ts`: a member inside the generated
//! class or namespace of its owner (it replaces a native member of the same name), a top-level
//! declaration after the engine's. Each entry's comment names the code that lowers it.

/// Where a built-in declaration goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A top-level declaration of this name (its text is the whole declaration, `declare`
    /// included); `@platypus/engine` re-exports it.
    Top(&'static str),
    /// A static member of a generated class, or a member of a generated namespace. The signature
    /// is written as a class member (`pick<T>(..): T;`, `readonly roots: T;`); inside a namespace it
    /// becomes `function ..` / `const ..`.
    Static(&'static str),
    /// An instance member of a generated class.
    Member(&'static str),
    /// A constructor of a generated class (they replace the generated ones).
    Ctor(&'static str),
}

/// One declaration.
#[derive(Clone, Copy, Debug)]
pub struct Builtin {
    pub place: Place,
    pub sig: &'static str,
    pub doc: &'static str,
}

const fn top(name: &'static str, sig: &'static str, doc: &'static str) -> Builtin {
    Builtin {
        place: Place::Top(name),
        sig,
        doc,
    }
}

const fn stat(owner: &'static str, sig: &'static str, doc: &'static str) -> Builtin {
    Builtin {
        place: Place::Static(owner),
        sig,
        doc,
    }
}

const fn member(owner: &'static str, sig: &'static str, doc: &'static str) -> Builtin {
    Builtin {
        place: Place::Member(owner),
        sig,
        doc,
    }
}

const fn ctor(owner: &'static str, sig: &'static str) -> Builtin {
    Builtin {
        place: Place::Ctor(owner),
        sig,
        doc: "",
    }
}

pub const BUILTINS: &[Builtin] = &[
    // ---- Timers (`lower/prelude.rs` `TIMERS`, over the runtime's timer queue).
    top(
        "setTimeout",
        "declare function setTimeout(callback: (arg?: number) => void, ms?: number): number;",
        "Calls `callback` once after `ms` milliseconds of engine time (0 by default).",
    ),
    top(
        "setInterval",
        "declare function setInterval(callback: (arg?: number) => void, ms?: number): number;",
        "Calls `callback` every `ms` milliseconds of engine time.",
    ),
    top(
        "clearTimeout",
        "declare function clearTimeout(id: number | undefined): void;",
        "Cancels a `setTimeout` (`undefined` does nothing).",
    ),
    top(
        "clearInterval",
        "declare function clearInterval(id: number | undefined): void;",
        "Cancels a `setInterval` (`undefined` does nothing).",
    ),
    top(
        "queueMicrotask",
        "declare function queueMicrotask(callback: () => void): void;",
        "Runs `callback` after the current script turn, before timers.",
    ),
    // ---- Window messages (`lower/messages.rs` over the JSON natives; `facade.rs` picks them).
    member(
        "Window",
        "postMessage(data: unknown): boolean;",
        "Queues `data` (structured values only: no handles, functions, Map or Set) for this window's domain; delivered at the start of that domain's next dispatch, in order per sender. False when the data is over 1 MiB as JSON or the window is closed.",
    ),
    member(
        "Window",
        "onMessage<T = unknown>(f: (from: Window, data: T) => void): number;",
        "A message posted to this window. Runs in this window's domain. Give `T` (or annotate the listener's `data`).",
    ),
    member(
        "Window",
        "onRequest<T = unknown, R = unknown>(f: (from: Window, data: T, reply: (answer: R) => void) => void): number;",
        "A request posted to this window; call `reply` once (a second call is ignored).",
    ),
    member(
        "Window",
        "request<R = unknown>(data: unknown, timeoutMs?: number): Promise<R>;",
        "Posts `data` as a request and resolves with the reply. Rejects with an `Error` when the window closes, no `onRequest` listener exists in its domain when it is delivered, or nobody replies within `timeoutMs` (default 10000).",
    ),
    member(
        "Window",
        "emit(name: string, payload?: unknown): boolean;",
        "Named events on top of the raw messages: sends event `name` (with an optional payload) to this window. Same limits as `postMessage`; false when refused. Throws on an empty name.",
    ),
    member(
        "Window",
        "on<T = unknown>(name: string, f: (payload: T, from: Window) => void): number;",
        "Runs `f(payload, from)` for each event `name` posted to this window, in its domain; other names are ignored. The handler's parameter type decodes the payload (a handler without parameters takes none).",
    ),
    member(
        "Window",
        "ask<R = unknown>(name: string, payload?: unknown, timeoutMs?: number): Promise<R>;",
        "Asks event `name`; resolves with what `answer(name, ..)` returned. Rejects like `request`, or at once with `no answer for \"name\"` when the window has no `answer` for it.",
    ),
    member(
        "Window",
        "answer<T = unknown, R = unknown>(name: string, f: (payload: T, from: Window) => R): number;",
        "Answers the asks of event `name` with the handler's return value.",
    ),
    stat(
        "Window",
        "onMessage<T = unknown>(f: (from: Window, data: T) => void): number;",
        "Messages to any window of the current domain (after the per-window listeners of the target).",
    ),
    stat(
        "Window",
        "onRequest<T = unknown, R = unknown>(f: (from: Window, data: T, reply: (answer: R) => void) => void): number;",
        "Requests to any window of the current domain; `reply` answers the sender's promise (once).",
    ),
    stat(
        "Window",
        "broadcast(data: unknown): number;",
        "Posts `data` to the root window of every other family. Returns how many were queued.",
    ),
    stat(
        "Window",
        "emitAll(name: string, payload?: unknown): number;",
        "`broadcast` of a named event (see `Window.emit`).",
    ),
    // ---- Types `#[ts_param]` names that no manifest declares (`Mesh.createFlat`'s lists; the
    // compiler reads a typed array or a plain one, `expr.rs` `coerce_ext`).
    top(
        "NumberList",
        "type NumberList = readonly number[] | Float32Array | Float64Array | Uint32Array | Uint16Array | Int32Array;",
        "Numbers as a plain array or a typed array (for meshes made in bulk).",
    ),
    // ---- `Random.pick<T>` (`lower/facade.rs` `lower_random_pick`).
    stat(
        "Random",
        "pick<T>(items: readonly T[]): T | undefined;",
        "A random item from a list (`undefined` when empty).",
    ),
    // ---- Wider than the native types: a string where a handle with `fromString` goes
    // (`lower/expr.rs` `coerce_ext`), and `Color`'s optional alpha (the value class's fields).
    member(
        "MeshRenderer",
        "mesh: Mesh | PrimitiveType;",
        "A mesh, or the name of a primitive.",
    ),
    member(
        "AudioSource",
        "playOneShot(clip: Sound | string, volumeScale?: number): void;",
        "Plays `clip` once over whatever else plays (footsteps, hits), at `volumeScale` x `volume`.",
    ),
    ctor(
        "Color",
        "constructor(r: number, g: number, b: number, a?: number);",
    ),
];

/// The name a member signature declares (`pick<T>(..)` is `pick`, `readonly roots: T` is `roots`).
pub fn member_name(sig: &str) -> &str {
    let s = sig.trim_start();
    let s = s.strip_prefix("readonly ").unwrap_or(s);
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(s.len());
    &s[..end]
}

/// The built-ins plus the declarations the manifests add: a manifest may list
/// `"declarations": [{"top": "register", "sig": "..", "doc": ".."}, {"member": "GameObject", ..}]`
/// (also `"static"` and `"ctor"`), the host's TypeScript names that no `#[ts_api]` item declares
/// (Platypus: the `Component` class, `register`, the `GameObject` conveniences of its compiler
/// extension).
#[derive(Clone, Debug, Default)]
pub struct Builtins {
    extra: Vec<Builtin>,
}

impl Builtins {
    /// The built-ins and every manifest's `declarations`. The strings are leaked: the generator
    /// runs once per process.
    pub fn from_manifests(manifests: &[String]) -> Result<Self, String> {
        let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
        let mut extra = Vec::new();
        for m in manifests {
            let v: serde_json::Value =
                serde_json::from_str(m).map_err(|e| format!("manifest: {e}"))?;
            for d in v["declarations"].as_array().into_iter().flatten() {
                let s = |k: &str| d[k].as_str();
                let place = if let Some(n) = s("top") {
                    Place::Top(leak(n))
                } else if let Some(o) = s("static") {
                    Place::Static(leak(o))
                } else if let Some(o) = s("member") {
                    Place::Member(leak(o))
                } else if let Some(o) = s("ctor") {
                    Place::Ctor(leak(o))
                } else {
                    return Err(format!("manifest: a declaration needs a place: {d}"));
                };
                let sig = s("sig").ok_or("manifest: a declaration needs a `sig`")?;
                extra.push(Builtin {
                    place,
                    sig: leak(sig),
                    doc: leak(s("doc").unwrap_or("")),
                });
            }
        }
        Ok(Builtins { extra })
    }

    /// Every declaration: the built-ins, then the manifests'.
    pub fn all(&self) -> impl Iterator<Item = &Builtin> {
        BUILTINS.iter().chain(&self.extra)
    }

    fn of(&self, place: impl Fn(Place) -> bool) -> impl Iterator<Item = &Builtin> {
        self.all().filter(move |b| place(b.place))
    }

    /// The built-ins of an owner's static side.
    pub fn statics<'s>(&'s self, owner: &'s str) -> impl Iterator<Item = &'s Builtin> {
        self.of(move |p| matches!(p, Place::Static(o) if o == owner))
    }

    /// The built-in instance members of a class.
    pub fn members<'s>(&'s self, owner: &'s str) -> impl Iterator<Item = &'s Builtin> {
        self.of(move |p| matches!(p, Place::Member(o) if o == owner))
    }

    /// The built-in constructors of a class.
    pub fn ctors<'s>(&'s self, owner: &'s str) -> impl Iterator<Item = &'s Builtin> {
        self.of(move |p| matches!(p, Place::Ctor(o) if o == owner))
    }

    /// The top-level declarations, each with its doc.
    pub fn top_level(&self) -> String {
        let mut s = String::new();
        for b in self.of(|p| matches!(p, Place::Top(_))) {
            if !b.doc.is_empty() {
                s += &format!("/** {} */\n", b.doc);
            }
            s += b.sig;
            s += "\n";
        }
        s
    }

    /// The names the top-level declarations give (`@platypus/engine` re-exports them).
    pub fn top_names(&self) -> Vec<&'static str> {
        self.of(|p| matches!(p, Place::Top(_)))
            .map(|b| match b.place {
                Place::Top(n) => n,
                _ => unreachable!("filtered to top-level entries"),
            })
            .collect()
    }
}

/// A class member signature as a namespace member (`function pick<T>(..)`, `const roots: T`).
pub fn as_namespace_member(sig: &str) -> String {
    match sig.strip_prefix("readonly ") {
        Some(rest) => format!("const {rest}"),
        None if member_is_property(sig) => format!("let {sig}"),
        None => format!("function {sig}"),
    }
}

fn member_is_property(sig: &str) -> bool {
    let name = member_name(sig);
    sig.trim_start()[name.len()..].starts_with(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_signatures_become_namespace_members() {
        assert_eq!(member_name("pick<T>(a: T[]): T;"), "pick");
        assert_eq!(member_name("readonly roots: X[];"), "roots");
        assert_eq!(
            as_namespace_member("pick<T>(a: T[]): T;"),
            "function pick<T>(a: T[]): T;"
        );
        assert_eq!(
            as_namespace_member("readonly roots: X[];"),
            "const roots: X[];"
        );
        assert_eq!(as_namespace_member("count: number;"), "let count: number;");
    }
}
