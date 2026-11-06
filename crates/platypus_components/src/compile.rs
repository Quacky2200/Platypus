// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus's compiler extension (`web_ui_ts::lower::ext`): what components and `GameObject`s
//! mean to the compiler beyond the generic host classes of the `hostClasses` manifest. All of it
//! runs while a program is lowered; compiled code is as before (direct calls, no options objects).
//!
//! - Hook slots: the Unity lifecycle hooks and the contact hooks (`onCollisionEnter`, with its
//!   `..2D` form), and the thunks that pass each hook what it takes.
//! - Hierarchy searches (`getComponentInChildren(Player)`) and the array properties `children` /
//!   `GameObject.roots`: TypeScript helpers appended to the program, compiled only when called.
//! - `GameObject.instantiate`, `dontDestroyOnLoad`, `createPrimitive`, `prefab.instantiate`,
//!   `go.components` / `go.scene`, `component.transform`, and the `new GameObject(name, { .. })`
//!   options (`parent`, local `position` / `rotation` / `scale`, `active`); any other option a
//!   component has no setter for goes to its transform (`option_through`).
//! - `Random.pick(items)`, an opaque `new Color(r, g, b)` (`field_default`), and the d.ts members
//!   of engine components (`transform`, `getComponent`: `dts_members`).

use web_ui_ts::ir::{CmpOp, Expr, ExprKind, RtFn, Stmt, Ty, UnOp};
use web_ui_ts::lower::ext::{Extension, HookThunk, OptionArg};
use web_ui_ts::lower::{Lowerer, R, mk};
use web_ui_ts::manifest::{ExternFn, Manifest};
use web_ui_ts::oxc_ast::ast::{
    Argument, CallExpression, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey,
};

/// The name the extension is installed under, which the `hostClasses` manifest names.
pub const EXTENSION: &str = "platypus";

/// The lifecycle hooks, in the order of the host's table (`HOOK_LIST`).
pub const HOOKS: [&str; 9] = [
    "onAwake",
    "onStart",
    "onUpdate",
    "onLateUpdate",
    "onFixedUpdate",
    "onPreRender",
    "onDestroy",
    "onApplicationFocus",
    "onApplicationPause",
];

/// The contact hooks, called when the physics step reports a contact. Each takes a `Collision`
/// (the other side; a record of `platypus_physics_api`); their thunks take the pieces.
pub const CONTACT_HOOKS: [&str; 6] = [
    "onCollisionEnter",
    "onCollisionExit",
    "onCollisionStay",
    "onTriggerEnter",
    "onTriggerExit",
    "onTriggerStay",
];

/// What a lifecycle hook's thunk passes it (the host gives every thunk `dt`) and the parameter it
/// declares: the frame hooks get the frame length, the focus and pause hooks a flag.
pub fn hook_param(hook: &str) -> Option<(&'static str, &'static str)> {
    match hook {
        "onUpdate" | "onLateUpdate" | "onFixedUpdate" => Some(("dt", "dt: number")),
        "onApplicationFocus" => Some(("dt !== 0", "hasFocus: boolean")),
        "onApplicationPause" => Some(("dt !== 0", "paused: boolean")),
        _ => None,
    }
}

/// The searches up and down the hierarchy and the prefix of the TypeScript helper that does each:
/// Unity's `GetComponent[s]In{Children,Parent}`.
const HIERARCHY_SEARCHES: [(&str, &str); 4] = [
    ("getComponentInChildren", "__cic_"),
    ("getComponentsInChildren", "__cac_"),
    ("getComponentInParent", "__cip_"),
    ("getComponentsInParent", "__cap_"),
];

const SEARCH_METHODS: [&str; 4] = [
    HIERARCHY_SEARCHES[0].0,
    HIERARCHY_SEARCHES[1].0,
    HIERARCHY_SEARCHES[2].0,
    HIERARCHY_SEARCHES[3].0,
];

/// Helper functions for the hierarchy searches: one per method and class named, appended to the
/// program (a class is not a run-time value here, so the walk is written per class).
fn children_helpers(src: &str) -> String {
    let mut out = String::new();
    for (method, prefix) in HIERARCHY_SEARCHES {
        let call = format!("{method}(");
        let mut names: Vec<String> = Vec::new();
        let mut rest = src;
        while let Some(i) = rest.find(&call) {
            rest = &rest[i + call.len()..];
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        for n in names {
            out += &match prefix {
                "__cic_" => format!(
                    "function __cic_{n}(go: GameObject): {n} | undefined {{\n  const own = go.getComponent({n});\n  if (own) return own;\n  for (let i = 0; i < go.childCount; i++) {{\n    const found = __cic_{n}(go.childAt(i));\n    if (found) return found;\n  }}\n  return undefined;\n}}\n"
                ),
                "__cac_" => format!(
                    "function __cac_{n}(go: GameObject): {n}[] {{\n  const out: {n}[] = [];\n  const own = go.getComponents({n});\n  for (let i = 0; i < own.length; i++) out.push(own[i]);\n  for (let i = 0; i < go.childCount; i++) {{\n    const sub = __cac_{n}(go.childAt(i));\n    for (let j = 0; j < sub.length; j++) out.push(sub[j]);\n  }}\n  return out;\n}}\n"
                ),
                "__cip_" => format!(
                    "function __cip_{n}(go: GameObject): {n} | undefined {{\n  let at: GameObject | null = go;\n  while (at) {{\n    const own = at.getComponent({n});\n    if (own) return own;\n    at = at.parent;\n  }}\n  return undefined;\n}}\n"
                ),
                _ => format!(
                    "function __cap_{n}(go: GameObject): {n}[] {{\n  const out: {n}[] = [];\n  let at: GameObject | null = go;\n  while (at) {{\n    const own = at.getComponents({n});\n    for (let i = 0; i < own.length; i++) out.push(own[i]);\n    at = at.parent;\n  }}\n  return out;\n}}\n"
                ),
            };
        }
    }
    if out.is_empty() {
        out
    } else {
        format!("\n{out}")
    }
}

/// Helpers for the `GameObject` properties that are arrays (`children`, `GameObject.roots`),
/// written over the engine's count / index functions, appended when the program names them.
fn tree_helpers(src: &str) -> String {
    let mut out = String::new();
    if src.contains(".children") {
        out += "\nfunction __go_children(go: GameObject): GameObject[] {\n  const out: GameObject[] = [];\n  for (let i = 0; i < go.childCount; i++) out.push(go.childAt(i));\n  return out;\n}\n";
    }
    if src.contains("GameObject.roots") {
        out += "\nfunction __go_roots(): GameObject[] {\n  const out: GameObject[] = [];\n  const n = GameObject.rootCount();\n  for (let i = 0; i < n; i++) out.push(GameObject.rootAt(i));\n  return out;\n}\n";
    }
    out
}

/// The thunk of a contact hook: the host passes the `Collision` handle and the other side's
/// `Collider` handle (`platypus_physics_api::contacts`), and the thunk hands the hook the one it
/// asks for. As in Unity, a trigger hook gets the other `Collider` (or `Collider2D`) and a
/// collision hook a `Collision` (or `Collision2D`) unless its parameter names the other kind.
fn contact_thunk(t: &HookThunk<'_>) -> String {
    let two_d = t.method.ends_with("2D");
    let wants = match t.param {
        Some(p @ ("Collider" | "Collider2D" | "Collision" | "Collision2D")) => p,
        _ if t.hook.starts_with("onTrigger") => {
            if two_d {
                "Collider2D"
            } else {
                "Collider"
            }
        }
        _ if two_d => "Collision2D",
        _ => "Collision",
    };
    let collision_ty = match wants {
        "Collision" | "Collision2D" => wants,
        _ if two_d => "Collision2D",
        _ => "Collision",
    };
    let collider_ty = match wants {
        "Collider" | "Collider2D" => wants,
        _ if two_d => "Collider2D",
        _ => "Collider",
    };
    let arg = if wants == collision_ty {
        "collision"
    } else {
        "collider"
    };
    format!(
        "export function {}(c: {}, collision: {collision_ty}, collider: {collider_ty}): void {{ c.{}({arg}); }}\n",
        t.name, t.class, t.method
    )
}

/// The object options of an `instantiate` call, if written as an object literal.
fn options_of<'a>(arg: Option<&'a Argument<'a>>, at: &str) -> R<Option<&'a ObjectExpression<'a>>> {
    match arg.and_then(|a| a.as_expression()) {
        Some(Expression::ObjectExpression(o)) => Ok(Some(&**o)),
        Some(_) => Err(format!("{at}: instantiate options are an object literal")),
        None => Ok(None),
    }
}

/// The call of an `instantiate` function (`lead`: the original or the prefab) with the options
/// object's `position`, `rotation` and `parent` as arguments (NaN: left out).
fn finish_instantiate<'a>(
    l: &mut Lowerer<'a>,
    f: &ExternFn,
    lead: Vec<Expr>,
    options: Option<&'a ObjectExpression<'a>>,
    at: &str,
) -> R<Expr> {
    let (mut position, mut rotation, mut parent) = (None, None, None);
    for p in options.map(|o| o.properties.iter()).into_iter().flatten() {
        let ObjectPropertyKind::ObjectProperty(p) = p else {
            return Err(format!("{at}: spread in options"));
        };
        let PropertyKey::StaticIdentifier(key) = &p.key else {
            return Err(format!("{at}: option names are plain names"));
        };
        match key.name.as_str() {
            "position" => {
                let sid = l.value_struct("Vector3");
                position = Some((l.lower_expr(&p.value, Some(&Ty::Obj(sid)))?, sid));
            }
            "rotation" => {
                let sid = l.value_struct("Quaternion");
                rotation = Some((l.lower_expr(&p.value, Some(&Ty::Obj(sid)))?, sid));
            }
            "parent" => parent = Some(l.lower_expr(&p.value, None)?),
            other => return Err(format!("{at}: instantiate has no option `{other}`")),
        }
    }
    let mut args = lead;
    // A vector or quaternion is read into a temporary once, then its fields are the arguments.
    let mut pre = Vec::new();
    let mut parts =
        |l: &mut Lowerer<'a>, v: Option<(Expr, usize)>, n: usize, args: &mut Vec<Expr>| match v {
            Some((e, sid)) => {
                let tmp = l.new_local(Ty::Obj(sid));
                pre.push(Stmt::Assign(tmp, e));
                for i in 0..n {
                    args.push(mk(
                        ExprKind::Field(Box::new(l.var_expr(tmp)), sid, i),
                        Ty::Num,
                    ));
                }
            }
            None => args.extend((0..n).map(|_| mk(ExprKind::Num(f64::NAN), Ty::Num))),
        };
    parts(l, position, 3, &mut args);
    parts(l, rotation, 4, &mut args);
    args.push(match parent {
        Some(p) => p,
        None => mk(ExprKind::Null, Ty::Handle("GameObject".into())),
    });
    let call = l.emit_extern(f, args);
    let ty = call.ty.clone();
    Ok(mk(ExprKind::Seq(pre, Box::new(call)), ty))
}

/// The extension itself (stateless).
pub struct Platypus;

impl Extension for Platypus {
    fn rewrite(&self, src: &str, manifest: &Manifest) -> R<Option<String>> {
        if manifest.host_object().is_none() {
            return Ok(None);
        }
        let extra = children_helpers(src) + &tree_helpers(src);
        Ok((!extra.is_empty()).then(|| format!("{src}{extra}")))
    }

    // The helpers above are lowered only once code calls them, so a program that never walks the
    // scene doesn't call the `GameObject` API (and gets no world tier from it).
    fn is_lazy_helper(&self, name: &str) -> bool {
        name.starts_with("__go_") || HIERARCHY_SEARCHES.iter().any(|(_, p)| name.starts_with(p))
    }

    fn hook_methods(&self, hook: &str) -> Vec<String> {
        if CONTACT_HOOKS.contains(&hook) {
            // `onCollisionEnter2D` fills the same slot as `onCollisionEnter`.
            vec![hook.to_string(), format!("{hook}2D")]
        } else {
            vec![hook.to_string()]
        }
    }

    fn hook_thunk(&self, t: &HookThunk<'_>, manifest: &Manifest) -> Option<String> {
        if CONTACT_HOOKS.contains(&t.hook) {
            // Contacts need the physics API's `Collision` (and an object to be on).
            let ok = manifest.host_object().is_some() && manifest.classes.contains_key("Collision");
            return Some(if ok { contact_thunk(t) } else { String::new() });
        }
        let call = hook_param(t.hook).map_or("", |(arg, _)| arg);
        Some(format!(
            "export function {}(c: {}, dt: number): void {{ c.{}({call}); }}\n",
            t.name, t.class, t.method
        ))
    }

    fn class_methods(&self) -> &[&str] {
        &SEARCH_METHODS
    }

    fn class_call<'a>(
        &self,
        l: &mut Lowerer<'a>,
        object: &Expr,
        m: &str,
        class: &str,
    ) -> R<Option<Expr>> {
        let Some((_, prefix)) = HIERARCHY_SEARCHES.iter().find(|(s, _)| *s == m) else {
            return Ok(None);
        };
        l.call_helper(&format!("{prefix}{class}"), vec![object.clone()])
            .map(Some)
    }

    fn static_member<'a>(&self, l: &mut Lowerer<'a>, owner: &str, prop: &str) -> R<Option<Expr>> {
        // `GameObject.roots`: the objects at the world root.
        if owner == "GameObject" && prop == "roots" && l.has_function("__go_roots") {
            return l.call_helper("__go_roots", vec![]).map(Some);
        }
        Ok(None)
    }

    /// The `GameObject` properties the engine API does not declare: `children` (a helper over
    /// `childAt`), `components` (every component of the program on it), `scene`.
    fn member<'a>(
        &self,
        l: &mut Lowerer<'a>,
        class: &str,
        obj: Expr,
        prop: &str,
    ) -> R<Option<Expr>> {
        if class != "GameObject" {
            return Ok(None);
        }
        match prop {
            "children" if l.has_function("__go_children") => {
                l.call_helper("__go_children", vec![obj]).map(Some)
            }
            "components" => {
                let base = l.manifest().host_base().unwrap_or_default().to_string();
                let Ty::Obj(sid) = l.named_type(&base)? else {
                    return Err("GameObject.components needs the program to use components".into());
                };
                let which = mk(ExprKind::Num(sid as f64), Ty::Num);
                let call = ExprKind::Rt(RtFn::Str("ts_rt_host_all_gc"), vec![obj, which]);
                Ok(Some(mk(call, Ty::Arr(Box::new(Ty::Obj(sid))))))
            }
            "scene" => {
                let f = l
                    .manifest()
                    .namespaces
                    .get("SceneManager")
                    .and_then(|n| n.get("sceneOf"))
                    .cloned()
                    .ok_or("GameObject.scene needs the scene manager API")?;
                Ok(Some(l.emit_extern(&f, vec![obj])))
            }
            _ => Ok(None),
        }
    }

    /// `camera.transform`: a component's object's transform.
    fn getter<'a>(
        &self,
        l: &mut Lowerer<'a>,
        class: &str,
        obj: &Expr,
        prop: &str,
    ) -> R<Option<Expr>> {
        if prop != "transform" || class == "GameObject" {
            return Ok(None);
        }
        let m = l.manifest();
        let go = m
            .classes
            .get(class)
            .and_then(|k| k.getters.get("gameObject"))
            .cloned();
        let tr = m
            .classes
            .get("GameObject")
            .and_then(|k| k.getters.get("transform"))
            .cloned();
        let (Some(go), Some(tr)) = (go, tr) else {
            return Ok(None);
        };
        let owner = l.emit_extern(&go, vec![obj.clone()]);
        Ok(Some(l.emit_extern(&tr, vec![owner])))
    }

    fn static_call<'a>(
        &self,
        l: &mut Lowerer<'a>,
        owner: &str,
        m: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Option<Expr>> {
        if owner == "Random" && m == "pick" {
            return random_pick(l, c);
        }
        if owner != "GameObject" {
            return Ok(None);
        }
        let at = l.loc(c.span.start);
        match m {
            "dontDestroyOnLoad" | "instantiate" => {
                let Some(ns) = l.manifest().namespaces.get("SceneManager").cloned() else {
                    return Ok(None);
                };
                if m == "dontDestroyOnLoad" {
                    let f = ns["dontDestroyOnLoad"].clone();
                    let call = l.call_ext_args(
                        f,
                        None,
                        "GameObject.dontDestroyOnLoad",
                        c.span.start,
                        &c.arguments,
                    )?;
                    return Ok(Some(call));
                }
                let Some(first) = c.arguments.first().and_then(|a| a.as_expression()) else {
                    return Err(format!("{at}: instantiate takes an object or a prefab"));
                };
                let original = l.lower_expr(first, None)?;
                let f = match original.ty.strip() {
                    Ty::Handle(h) if h == "Prefab" => {
                        l.manifest().classes["Prefab"].methods["instantiate"].clone()
                    }
                    _ => ns["instantiate"].clone(),
                };
                let options = options_of(c.arguments.get(1), &at)?;
                finish_instantiate(l, &f, vec![original], options, &at).map(Some)
            }
            // `GameObject.createPrimitive("cube", { position, scale, .. })`: `Primitives.create`,
            // then the options as for `new GameObject`.
            "createPrimitive" => {
                let Some(create) = l
                    .manifest()
                    .namespaces
                    .get("Primitives")
                    .and_then(|n| n.get("create"))
                    .cloned()
                else {
                    return Ok(None);
                };
                let Some(kind) = c.arguments.first().and_then(|a| a.as_expression()) else {
                    return Err(format!("{at}: createPrimitive takes a kind"));
                };
                let kind = l.lower_expr(kind, Some(&Ty::Str))?;
                let kind = l.coerce(kind, &Ty::Str)?;
                let ty = Ty::Handle("GameObject".to_string());
                let tmp = l.new_local(ty.clone());
                let none = mk(ExprKind::Null, Ty::Str.nullable());
                let made = l.emit_extern(&create, vec![kind, none]);
                let mut stmts = vec![Stmt::Assign(tmp, made)];
                match c.arguments.get(1).and_then(|a| a.as_expression()) {
                    Some(Expression::ObjectExpression(o)) => {
                        l.apply_options("GameObject", tmp, o, &mut stmts, &at)?
                    }
                    Some(_) => {
                        return Err(format!(
                            "{at}: createPrimitive options are an object literal"
                        ));
                    }
                    None => {}
                }
                let result = l.var_expr(tmp);
                Ok(Some(mk(ExprKind::Seq(stmts, Box::new(result)), ty)))
            }
            _ => Ok(None),
        }
    }

    /// `prefab.instantiate({ position, rotation, parent })`.
    fn method_call<'a>(
        &self,
        l: &mut Lowerer<'a>,
        class: &str,
        recv: &Expr,
        m: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Option<Expr>> {
        if class != "Prefab" || m != "instantiate" {
            return Ok(None);
        }
        let f = l.manifest().classes["Prefab"].methods["instantiate"].clone();
        let at = l.loc(c.span.start);
        // A non-literal argument is left to the plain call.
        let options = match c.arguments.first().and_then(|a| a.as_expression()) {
            Some(Expression::ObjectExpression(o)) => Some(&**o),
            _ => None,
        };
        finish_instantiate(l, &f, vec![recv.clone()], options, &at).map(Some)
    }

    /// A `new GameObject(name, { .. })` option the plain setter lookup would not find: `parent`
    /// (`setParent(p, false)`), `position` / `rotation` / `scale` (local to the parent:
    /// `localPosition`, ...) and `active`.
    fn option<'a>(&self, l: &mut Lowerer<'a>, o: OptionArg<'a, '_>) -> R<bool> {
        if o.class != "GameObject" {
            return Ok(false);
        }
        let Some(go) = l.manifest().classes.get("GameObject").cloned() else {
            return Ok(false);
        };
        if o.key == "parent" {
            let Some(f) = go.methods.get("setParent").cloned() else {
                return Err(format!("{}: GameObject has no setParent", o.at));
            };
            let want = l.ext_ir(&f.params[1].1);
            let v = l.lower_expr(o.value, Some(&want))?;
            let v = l.coerce_ext(v, &want)?;
            let h = l.var_expr(o.tmp);
            let keep = mk(ExprKind::Bool(false), Ty::Bool);
            o.stmts
                .push(Stmt::Eval(l.emit_extern(&f, vec![h, v, keep])));
            return Ok(true);
        }
        let local = match o.key {
            "position" => "localPosition",
            "rotation" => "localRotation",
            "scale" => "localScale",
            "active" => {
                let Some(setter) = go.setters.get("enabled").cloned() else {
                    return Ok(false);
                };
                let v = l.lower_expr(o.value, Some(&Ty::Bool))?;
                let v = l.coerce(v, &Ty::Bool)?;
                let h = l.var_expr(o.tmp);
                o.stmts.push(Stmt::Eval(l.emit_extern(&setter, vec![h, v])));
                return Ok(true);
            }
            _ => return Ok(false),
        };
        let get = go.getters.get("transform").cloned();
        let set = l
            .manifest()
            .classes
            .get("Transform")
            .and_then(|t| t.setters.get(local))
            .cloned();
        let (Some(get), Some(set)) = (get, set) else {
            return Ok(false);
        };
        let want = l.ext_ir(&set.params[1].1);
        let v = l.lower_expr(o.value, None)?;
        // An optional value (`rotation?: Quaternion` passed on) is left alone when it is not there.
        // (An optional parameter of an object type is the object type with a null pointer.)
        if v.ty.is_nullable()
            || matches!(v.ty.strip(), Ty::Obj(_) if matches!(v.kind, ExprKind::Var(_)))
        {
            let is_null = mk(ExprKind::IsNull(Box::new(v.clone())), Ty::Bool);
            let present = mk(ExprKind::Un(UnOp::Not, Box::new(is_null)), Ty::Bool);
            let inner = l.coerce(v, &want)?;
            let h = l.var_expr(o.tmp);
            let tr = l.emit_extern(&get, vec![h]);
            let write = Stmt::Eval(l.emit_extern(&set, vec![tr, inner]));
            o.stmts.push(Stmt::If(present, vec![write], vec![]));
            return Ok(true);
        }
        let v = l.coerce(v, &want)?;
        let h = l.var_expr(o.tmp);
        let tr = l.emit_extern(&get, vec![h]);
        o.stmts.push(Stmt::Eval(l.emit_extern(&set, vec![tr, v])));
        Ok(true)
    }

    /// An option a component or object has no setter for is its transform's
    /// (`new Light({ position })`, `{ ...placement }`).
    fn option_through(
        &self,
        manifest: &Manifest,
        class: &str,
        key: &str,
    ) -> Option<(ExternFn, ExternFn)> {
        let get = manifest.classes.get(class)?.getters.get("transform")?;
        let set = manifest.classes.get("Transform")?.setters.get(key)?;
        Some((get.clone(), set.clone()))
    }

    /// A `Color` left without its alpha is opaque (`new Color(r, g, b)`, `new Color(...rgb)`).
    fn field_default(&self, class: &str, field: usize) -> Option<f64> {
        (class == "Color" && field == 3).then_some(1.0)
    }

    /// An engine component (a class with a `readonly gameObject: GameObject`, not a hit or a
    /// collider that may name none) has what every component has.
    fn dts_members(&self, _class: &str, declared: &[&str]) -> Vec<String> {
        if !declared.contains(&"readonly gameObject: GameObject;") {
            return Vec::new();
        }
        // A member line's name: `readonly transform: ..`, `getComponent<T>(..)`.
        let has = |n: &str| {
            declared.iter().any(|l| {
                let l = l.trim_start();
                let l = l.strip_prefix("readonly ").unwrap_or(l);
                l.strip_prefix(n)
                    .is_some_and(|r| r.starts_with([':', '(', '<', '?']))
            })
        };
        let mut out = Vec::new();
        if !has("transform") {
            out.push("readonly transform: Transform;".to_string());
        }
        if !has("getComponent") {
            out.push("getComponent<T>(cls: { prototype: T }): T | undefined;".to_string());
        }
        out
    }
}

/// `Random.pick(items)`: the item at `Random.rangeInt(0, length)`, or `undefined` for an empty
/// list (no number is drawn then).
fn random_pick<'a>(l: &mut Lowerer<'a>, c: &'a CallExpression<'a>) -> R<Option<Expr>> {
    let at = l.loc(c.span.start);
    let Some(range) = l
        .manifest()
        .namespaces
        .get("Random")
        .and_then(|n| n.get("rangeInt"))
        .cloned()
    else {
        return Ok(None);
    };
    let [arg] = c.arguments.as_slice() else {
        return Err(format!("{at}: Random.pick takes one array"));
    };
    let Some(e) = arg.as_expression() else {
        return Err(format!("{at}: Random.pick takes an array, not a spread"));
    };
    let items = l.lower_expr(e, None)?;
    let el = match items.ty.strip() {
        Ty::Arr(el) => (**el).clone(),
        t => return Err(format!("{at}: Random.pick takes an array, not {t}")),
    };
    let num = |n: f64| mk(ExprKind::Num(n), Ty::Num);
    let want = el.clone().nullable();
    let a = l.new_local(items.ty.clone());
    let len = mk(ExprKind::Len(Box::new(l.var_expr(a))), Ty::Num);
    let i = l.emit_extern(&range, vec![num(0.0), len.clone()]);
    // As `items[i]` reads it (null past the end for pointers, numbers and handles).
    let raw = if el.is_ptr() || el == Ty::Num || matches!(el.strip(), Ty::Handle(_)) {
        el.nullable()
    } else {
        el
    };
    let read = mk(ExprKind::Index(Box::new(l.var_expr(a)), Box::new(i)), raw);
    let read = l.coerce(read, &want)?;
    let none = l.coerce(mk(ExprKind::Null, Ty::Null), &want)?;
    let some = mk(
        ExprKind::Cmp(CmpOp::Gt, Box::new(len), Box::new(num(0.0))),
        Ty::Bool,
    );
    let pick = mk(
        ExprKind::Cond(Box::new(some), Box::new(read), Box::new(none)),
        want.clone(),
    );
    Ok(Some(mk(
        ExprKind::Seq(vec![Stmt::Assign(a, items)], Box::new(pick)),
        want,
    )))
}

/// The `Component` base class as `types/platypus.d.ts` declares it: the hooks with what their
/// thunks pass, then the members every component has.
pub fn component_class() -> String {
    let mut s = String::from(
        "/**\n * The base of a script component: `class Player extends Component { .. }`, then `register({ Player })`.\n * The contact hooks (",
    );
    let contacts: Vec<String> = CONTACT_HOOKS.iter().map(|h| format!("`{h}`")).collect();
    s += &contacts.join(", ");
    s += ", and their `..2D` forms) take a `Collision` (`Collision2D`), a trigger's the other `Collider` (`Collider2D`); they are not\n * declared, so a game writes them without `override`.\n */\ndeclare abstract class Component {\n  /** Set by the engine when the component is created. */\n  readonly id: number;\n  /** The object this component is attached to. */\n  readonly gameObject: GameObject;\n  /** The object's transform. */\n  readonly transform: Transform;\n";
    for h in HOOKS {
        let param = hook_param(h).map_or("", |(_, p)| p);
        s += &format!("  {h}?({param}): void;\n");
    }
    s += "  /** After a hot reload, for a component restored from the old code (instead of `onAwake`). */\n  onReload?(): void;\n  /** Another component on the same object. */\n  getComponent<T>(cls: ClassOf<T>): T | undefined;\n  /** Removes this component from its object. */\n  destroy(): void;\n}";
    s
}

/// Where a declaration goes (`"declarations"` in the manifest, `web_ui_ts::builtins`).
#[derive(Clone, Copy)]
enum At {
    Top(&'static str),
    Static(&'static str),
    Member(&'static str),
    Ctor(&'static str),
}

/// The TypeScript the extension adds to `types/platypus.d.ts` (besides `component_class`).
const DECLARATIONS: &[(At, &str, &str)] = &[
    (
        At::Top("ComponentClass"),
        "type ComponentClass<T = Component> = new () => T;",
        "A component class (what `register` takes).",
    ),
    (
        At::Top("ClassOf"),
        "type ClassOf<T> = { prototype: T };",
        "A class, engine or script, by its instances' type (what `getComponent` takes).",
    ),
    (
        At::Top("register"),
        "declare function register(components: Record<string, ComponentClass>, options?: { spawn?: boolean }): void;",
        "Registers component classes by name (scene files use the names).",
    ),
    (
        At::Top("property"),
        "declare function property(value: unknown, context: ClassFieldDecoratorContext | ClassGetterDecoratorContext): void;",
        "Marks a field as saved in scenes and kept across hot reloads.",
    ),
    (
        At::Top("GameObjectOptions"),
        "interface GameObjectOptions {\n  parent?: GameObject | null;\n  position?: Vector3Like;\n  rotation?: QuaternionLike;\n  scale?: Vector3Like;\n}",
        "Where a new object goes: its parent, and its position, rotation and scale (local to the parent).",
    ),
    (
        At::Top("InstantiateObjectOptions"),
        "interface InstantiateObjectOptions {\n  parent?: GameObject | null;\n  position?: Vector3Like;\n  rotation?: QuaternionLike;\n}",
        "Where a copy goes (`GameObject.instantiate`).",
    ),
    (
        At::Ctor("GameObject"),
        "constructor(name?: string, options?: GameObjectOptions);",
        "",
    ),
    (
        At::Member("GameObject"),
        "readonly components: Component[];",
        "The components of this program on the object.",
    ),
    (
        At::Member("GameObject"),
        "readonly children: GameObject[];",
        "Its children, in order.",
    ),
    (
        At::Member("GameObject"),
        "readonly scene: Scene;",
        "The scene it is in.",
    ),
    (
        At::Member("GameObject"),
        "addComponent<T>(cls: ClassOf<T>, props?: Partial<T>): T;",
        "Adds a component of a class (engine or script), with some of its properties set.",
    ),
    (
        At::Member("GameObject"),
        "getComponent<T>(cls: ClassOf<T>): T | undefined;",
        "The first component of a class on the object.",
    ),
    (
        At::Member("GameObject"),
        "getComponents<T>(cls: ClassOf<T>): T[];",
        "Every component of a class on the object.",
    ),
    (
        At::Member("GameObject"),
        "getComponentInChildren<T>(cls: ClassOf<T>): T | undefined;",
        "The first component of a class on the object or below it.",
    ),
    (
        At::Member("GameObject"),
        "getComponentsInChildren<T>(cls: ClassOf<T>): T[];",
        "Every component of a class on the object and below it.",
    ),
    (
        At::Member("GameObject"),
        "getComponentInParent<T>(cls: ClassOf<T>): T | undefined;",
        "The first component of a class on the object or above it.",
    ),
    (
        At::Member("GameObject"),
        "getComponentsInParent<T>(cls: ClassOf<T>): T[];",
        "Every component of a class on the object and above it.",
    ),
    (
        At::Member("GameObject"),
        "removeComponent(component: Component): void;",
        "Removes a component of this program from the object.",
    ),
    (
        At::Static("GameObject"),
        "findObjectOfType<T>(cls: ClassOf<T>): T | undefined;",
        "The first component of a class on any object.",
    ),
    (
        At::Static("GameObject"),
        "readonly roots: readonly GameObject[];",
        "The objects without a parent.",
    ),
    (
        At::Static("GameObject"),
        "dontDestroyOnLoad(go: GameObject): void;",
        "Keeps an object (and what is under it) when a single-mode scene load replaces the others.",
    ),
    (
        At::Static("GameObject"),
        "instantiate(original: GameObject | Prefab, options?: InstantiateObjectOptions): GameObject;",
        "A copy of an object or a prefab, with its components.",
    ),
    (
        At::Static("GameObject"),
        "createPrimitive(type: PrimitiveType, options?: GameObjectOptions & { name?: string }): GameObject;",
        "An object with a mesh renderer of a built-in mesh.",
    ),
    // `random_pick`.
    (
        At::Static("Random"),
        "pick<T>(items: readonly T[]): T | undefined;",
        "A random item from a list (`undefined` when empty).",
    ),
    // Wider than the native types: a string where a handle with `fromString` goes (the
    // compiler's `coerce_ext`), and `Color`'s optional alpha (`field_default`).
    (
        At::Member("MeshRenderer"),
        "mesh: Mesh | PrimitiveType;",
        "A mesh, or the name of a primitive.",
    ),
    (
        At::Member("AudioSource"),
        "playOneShot(clip: Sound | string, volumeScale?: number): void;",
        "Plays `clip` once over whatever else plays (footsteps, hits), at `volumeScale` x `volume`.",
    ),
    (
        At::Ctor("Color"),
        "constructor(r: number, g: number, b: number, a?: number);",
        "",
    ),
];

/// The declarations as manifest JSON (`"declarations": [..]`), the `Component` class first.
pub fn declarations() -> serde_json::Value {
    let mut out = vec![serde_json::json!({
        "top": "Component",
        "sig": component_class(),
        "doc": "",
    })];
    for (at, sig, doc) in DECLARATIONS {
        let (k, v) = match at {
            At::Top(n) => ("top", n),
            At::Static(o) => ("static", o),
            At::Member(o) => ("member", o),
            At::Ctor(o) => ("ctor", o),
        };
        out.push(serde_json::json!({ k: v, "sig": sig, "doc": doc }));
    }
    serde_json::Value::Array(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-TS-3360
    #[test]
    fn the_component_class_has_every_hook_the_host_calls() {
        let c = component_class();
        for h in HOOKS {
            assert!(c.contains(&format!("  {h}?(")), "{h}: {c}");
        }
        assert!(c.contains("  onUpdate?(dt: number): void;"), "{c}");
        assert!(
            c.contains("  onApplicationFocus?(hasFocus: boolean): void;"),
            "{c}"
        );
        assert!(c.contains("`onTriggerStay`"), "{c}");
        // Every top-level entry names what it declares.
        for (at, sig, _) in DECLARATIONS {
            if let At::Top(n) = at {
                assert!(sig.contains(n), "{n}: {sig}");
            }
        }
    }
}
