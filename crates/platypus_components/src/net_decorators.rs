// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `@sync` fields and `@rpc` methods (M8, `docs/multiplayer.md` "Decorators"): the networking
//! schema written where the data is, instead of as text passed to `Network.defineClass`.
//!
//! ```ts
//! class Player extends NetworkBehaviour {
//!   @sync({ quantize: 0.01, interpolate: true }) pos: Vector3 = Vector3.zero;
//!   @sync hp: number = 100;
//!   @sync({ owner: true }) yaw: number = 0;
//!   @rpc("server") fire(aim: number): void { /* runs on the server */ }
//! }
//! Player.defineNetwork();                    // the schema: every peer runs this first
//! const p = Player.spawnNet(owner);          // the server: spawns the object, sets the initial values
//! p.hp = 50;                                 // a read or write of the replicated field
//! p.fire(1.5);                               // a client's call is sent; the server runs the body
//! p.netDispatch(name, args);                 // an `rpc` event for this object arrives: run the body
//! ```
//!
//! A decorator is compile-time metadata, not code that runs, so this is a rewrite of the program
//! text before it is parsed (`Extension::rewrite`), as the compiler does for `register(..)`:
//! each `@sync` field becomes a getter and setter over `Network.get*` / `set*` of the object's
//! `netId`, each `@rpc` method a wrapper that sends the call (or runs it, where the call lands
//! here) beside the original body, and the class gains `defineNetwork`, `spawnNet`, `netInit` and
//! `netDispatch`. A class with a decorator must extend `NetworkBehaviour` (declared in the d.ts and
//! added to the program here) and have a constructor with no required parameters.
//!
//! Field types: `number` (`f32`, or `i32` with `{ int: true }`), `boolean`, `string`, `Vector3`,
//! `Quaternion`. Field options: `quantize`, `interpolate`, `owner`, `int`. RPC: `@rpc(target?,
//! options?)` with the target `"server"` (the default), `"owner"` or `"all"` and the options
//! `anyone` and `unreliable`; its parameters are the same types, and it returns nothing.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use web_ui_ts::oxc_ast::ast::{
    Argument, BindingPattern, ClassElement, Declaration, Decorator, Expression,
    MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement, TSType,
};

type R<T> = Result<T, String>;

/// The base class of networked classes, added to the program when a decorator is used.
const BASE_CLASS: &str = "let __net_listen: boolean = false;\nclass NetworkBehaviour {\n  /** A listen server (a server whose script also plays, as player 0) calls this with true: what the server sends to `owner` and `all` methods then also runs on it. */\n  static setListenServer(on: boolean): void { __net_listen = on; }\n  /** The network id of this object (0 before it is spawned or bound). */\n  netId: number = 0;\n  /** Sets the initial values of the synced fields (what `spawnNet` runs). */\n  netInit(): void {}\n  /** Runs the remote-call method `name` with its arguments (the text of an `rpc` event's arguments); false if there is none. */\n  netDispatch(name: string, args: string[]): boolean { return false; }\n}\n";

/// Helpers the generated code calls.
const HELPERS: &str = "function __net_send(id: number, name: string, target: string, args: string): boolean {\n  if (target === \"server\") {\n    // The server (or a listen server) runs it; a client sends it.\n    if (Network.isServer()) return true;\n    Network.rpc(id, name, args);\n    return false;\n  }\n  // Only the server calls `owner` and `all` RPCs; a listen server (one that plays) is a target too.\n  if (!Network.isServer()) return false;\n  Network.rpc(id, name, args);\n  return __net_listen && (target === \"all\" || Network.owner(id) === 0);\n}\nfunction __net_vec(s: string): Vector3 {\n  const p = s.slice(2).split(\",\");\n  return new Vector3(parseFloat(p[0] ?? \"0\"), parseFloat(p[1] ?? \"0\"), parseFloat(p[2] ?? \"0\"));\n}\nfunction __net_quat(s: string): Quaternion {\n  const p = s.slice(2).split(\",\");\n  return new Quaternion(parseFloat(p[0] ?? \"0\"), parseFloat(p[1] ?? \"0\"), parseFloat(p[2] ?? \"0\"), parseFloat(p[3] ?? \"1\"));\n}\nfunction __net_v3(a: number[]): Vector3 {\n  return new Vector3(a[0] ?? 0, a[1] ?? 0, a[2] ?? 0);\n}\nfunction __net_q4(a: number[]): Quaternion {\n  return new Quaternion(a[0] ?? 0, a[1] ?? 0, a[2] ?? 0, a[3] ?? 1);\n}\n";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NetTy {
    Number,
    Bool,
    Str,
    Vec3,
    Quat,
}

impl NetTy {
    fn parse(text: &str) -> Option<NetTy> {
        Some(match text.trim() {
            "number" => NetTy::Number,
            "boolean" => NetTy::Bool,
            "string" => NetTy::Str,
            "Vector3" => NetTy::Vec3,
            "Quaternion" => NetTy::Quat,
            _ => return None,
        })
    }

    fn ts(self) -> &'static str {
        match self {
            NetTy::Number => "number",
            NetTy::Bool => "boolean",
            NetTy::Str => "string",
            NetTy::Vec3 => "Vector3",
            NetTy::Quat => "Quaternion",
        }
    }

    /// The type as the schema text names it.
    fn wire(self, int: bool) -> &'static str {
        match self {
            NetTy::Number if int => "i32",
            NetTy::Number => "f32",
            NetTy::Bool => "bool",
            NetTy::Str => "str",
            NetTy::Vec3 => "vec3",
            NetTy::Quat => "quat",
        }
    }

    /// The expression that reads field `key` of this object.
    fn read(self, key: &str) -> String {
        match self {
            NetTy::Number => format!("Network.getNumber(this.netId, \"{key}\")"),
            NetTy::Bool => format!("Network.getBool(this.netId, \"{key}\")"),
            NetTy::Str => format!("Network.getString(this.netId, \"{key}\")"),
            NetTy::Vec3 => format!("__net_v3(Network.getVec3(this.netId, \"{key}\"))"),
            NetTy::Quat => format!("__net_q4(Network.getQuat(this.netId, \"{key}\"))"),
        }
    }

    /// The statement that writes `v` to field `key`.
    fn write(self, key: &str, v: &str) -> String {
        match self {
            NetTy::Number => format!("Network.setNumber(this.netId, \"{key}\", {v});"),
            NetTy::Bool => format!("Network.setBool(this.netId, \"{key}\", {v});"),
            NetTy::Str => format!("Network.setString(this.netId, \"{key}\", {v});"),
            NetTy::Vec3 => {
                format!("Network.setVec3(this.netId, \"{key}\", {v}.x, {v}.y, {v}.z);")
            }
            NetTy::Quat => {
                format!("Network.setQuat(this.netId, \"{key}\", {v}.x, {v}.y, {v}.z, {v}.w);")
            }
        }
    }

    /// The RPC argument text of a value (`f:1.5`, `v:1,2,3`).
    fn encode(self, v: &str) -> String {
        match self {
            NetTy::Number => format!("f:${{{v}}}"),
            NetTy::Bool => format!("b:${{{v} ? 1 : 0}}"),
            NetTy::Str => format!("s:${{{v}}}"),
            NetTy::Vec3 => format!("v:${{{v}.x}},${{{v}.y}},${{{v}.z}}"),
            NetTy::Quat => format!("q:${{{v}.x}},${{{v}.y}},${{{v}.z}},${{{v}.w}}"),
        }
    }

    /// The expression that reads an RPC argument back from its text `a`.
    fn decode(self, a: &str) -> String {
        match self {
            NetTy::Number => format!("parseFloat(({a} ?? \"\").slice(2))"),
            NetTy::Bool => format!("({a} ?? \"\") === \"b:1\""),
            NetTy::Str => format!("({a} ?? \"\").slice(2)"),
            NetTy::Vec3 => format!("__net_vec({a} ?? \"\")"),
            NetTy::Quat => format!("__net_quat({a} ?? \"\")"),
        }
    }
}

/// The options object of a decorator, `{ a: 1, b: true, c: "x" }`, as plain values.
#[derive(Default, Debug)]
struct Opts {
    numbers: Vec<(String, f64)>,
    bools: Vec<(String, bool)>,
}

impl Opts {
    fn num(&self, k: &str) -> Option<f64> {
        self.numbers.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
    }

    fn flag(&self, k: &str) -> bool {
        self.bools.iter().any(|(n, v)| n == k && *v)
    }
}

fn read_opts(arg: Option<&Argument<'_>>, what: &str) -> R<Opts> {
    let mut o = Opts::default();
    let Some(arg) = arg else { return Ok(o) };
    let Argument::ObjectExpression(obj) = arg else {
        return Err(format!("{what}: the options must be an object literal"));
    };
    for p in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = p else {
            return Err(format!(
                "{what}: the options take plain `name: value` pairs"
            ));
        };
        let PropertyKey::StaticIdentifier(k) = &p.key else {
            return Err(format!("{what}: an option name must be an identifier"));
        };
        match &p.value {
            Expression::NumericLiteral(n) => o.numbers.push((k.name.to_string(), n.value)),
            Expression::BooleanLiteral(b) => o.bools.push((k.name.to_string(), b.value)),
            _ => {
                return Err(format!(
                    "{what}: option `{}` must be a number or true / false (it is read at compile time)",
                    k.name
                ));
            }
        }
    }
    Ok(o)
}

/// What a decorator is: its name and its arguments.
fn decorator<'a, 'b>(d: &'b Decorator<'a>) -> Option<(&'b str, &'b [Argument<'a>])> {
    match &d.expression {
        Expression::Identifier(id) => Some((id.name.as_str(), &[])),
        Expression::CallExpression(c) => match &c.callee {
            Expression::Identifier(id) => Some((id.name.as_str(), &c.arguments[..])),
            _ => None,
        },
        _ => None,
    }
}

struct SyncField {
    key: String,
    ty: NetTy,
    int: bool,
    quantize: Option<f64>,
    interpolate: bool,
    owner: bool,
    init: Option<String>,
}

struct RpcMethod {
    key: String,
    target: String,
    flags: Vec<&'static str>,
    params: Vec<(String, NetTy)>,
}

/// A change to the text: replace `start..end` with `text`.
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

fn slice(src: &str, span: oxc_span::Span) -> &str {
    &src[span.start as usize..span.end as usize]
}

/// The program with every `@sync` / `@rpc` class rewritten; `None` when there are none (or the
/// program does not parse, which the compiler then reports itself).
pub fn rewrite(src: &str) -> R<Option<String>> {
    if !src.contains("@sync") && !src.contains("@rpc") {
        return Ok(None);
    }
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, src, SourceType::ts()).parse();
    if !ret.diagnostics.is_empty() {
        return Ok(None);
    }
    let mut edits: Vec<Edit> = Vec::new();
    for st in &ret.program.body {
        let class = match st {
            Statement::ClassDeclaration(c) => Some(c),
            Statement::ExportDeclaration(x) => match &x.declaration {
                Declaration::ClassDeclaration(c) => Some(c),
                _ => None,
            },
            _ => None,
        };
        let Some(c) = class else { continue };
        let Some(id) = &c.id else { continue };
        let name = id.name.to_string();
        let mut fields: Vec<SyncField> = Vec::new();
        let mut rpcs: Vec<RpcMethod> = Vec::new();
        for el in &c.body.body {
            match el {
                ClassElement::PropertyDefinition(p) if !p.decorators.is_empty() => {
                    // `@property` and the like are the compiler's; only `@sync` is ours.
                    let mut marked = None;
                    for d in &p.decorators {
                        if let Some(("sync", args)) = decorator(d) {
                            marked = Some(args);
                        }
                    }
                    let Some(args) = marked else { continue };
                    if p.decorators.len() > 1 {
                        return Err(format!(
                            "class {name}: a @sync field takes no other decorator"
                        ));
                    }
                    let key = match &p.key {
                        PropertyKey::StaticIdentifier(k) => k.name.to_string(),
                        _ => return Err(format!("class {name}: a @sync field needs a plain name")),
                    };
                    let what = format!("class {name}: @sync {key}");
                    let ann = p
                        .type_annotation
                        .as_ref()
                        .map(|t| slice(src, t.type_annotation.span()))
                        .ok_or_else(|| format!("{what} needs a type"))?;
                    let ty = NetTy::parse(ann).ok_or_else(|| {
                        format!("{what}: the type must be number, boolean, string, Vector3 or Quaternion, not `{ann}`")
                    })?;
                    let o = read_opts(args.first(), &what)?;
                    if p.r#static {
                        return Err(format!("{what}: a static field cannot be replicated"));
                    }
                    let start = p
                        .decorators
                        .iter()
                        .map(|d| d.span.start)
                        .min()
                        .unwrap_or(p.span.start)
                        .min(p.span.start) as usize;
                    // The accessors take the member's place.
                    let t = ty.ts();
                    edits.push(Edit {
                        start,
                        end: p.span.end as usize,
                        text: format!(
                            "get {key}(): {t} {{ return {}; }}\n  set {key}(v: {t}) {{ {} }}",
                            ty.read(&key),
                            ty.write(&key, "v")
                        ),
                    });
                    fields.push(SyncField {
                        key,
                        ty,
                        int: o.flag("int"),
                        quantize: o.num("quantize"),
                        interpolate: o.flag("interpolate"),
                        owner: o.flag("owner"),
                        init: p.value.as_ref().map(|v| slice(src, v.span()).to_string()),
                    });
                }
                ClassElement::MethodDefinition(m) if !m.decorators.is_empty() => {
                    let mut marked = None;
                    for d in &m.decorators {
                        if let Some(("rpc", args)) = decorator(d) {
                            marked = Some(args);
                        }
                    }
                    let Some(args) = marked else { continue };
                    if m.decorators.len() > 1 {
                        return Err(format!(
                            "class {name}: an @rpc method takes no other decorator"
                        ));
                    }
                    let key = match &m.key {
                        PropertyKey::StaticIdentifier(k) => k.name.to_string(),
                        _ => {
                            return Err(format!("class {name}: an @rpc method needs a plain name"));
                        }
                    };
                    let what = format!("class {name}: @rpc {key}");
                    if m.kind != MethodDefinitionKind::Method || m.r#static {
                        return Err(format!(
                            "{what}: only a plain instance method can be an RPC"
                        ));
                    }
                    let target = match args.first() {
                        None => "server".to_string(),
                        Some(Argument::StringLiteral(s))
                            if matches!(s.value.as_str(), "server" | "owner" | "all") =>
                        {
                            s.value.to_string()
                        }
                        _ => {
                            return Err(format!(
                                "{what}: the target must be \"server\", \"owner\" or \"all\""
                            ));
                        }
                    };
                    let o = read_opts(args.get(1), &what)?;
                    let mut flags = Vec::new();
                    if o.flag("anyone") {
                        flags.push("anyone");
                    }
                    if o.flag("unreliable") {
                        flags.push("unreliable");
                    }
                    let f = &m.value;
                    let Some(body) = &f.body else {
                        return Err(format!("{what} needs a body"));
                    };
                    let mut params = Vec::new();
                    for p in &f.params.items {
                        let BindingPattern::BindingIdentifier(b) = &p.pattern else {
                            return Err(format!("{what}: parameters must be plain names"));
                        };
                        let ann = p
                            .type_annotation
                            .as_ref()
                            .map(|t| slice(src, t.type_annotation.span()))
                            .ok_or_else(|| format!("{what}: parameter {} needs a type", b.name))?;
                        let ty = NetTy::parse(ann).ok_or_else(|| {
                            format!("{what}: parameter {} must be number, boolean, string, Vector3 or Quaternion, not `{ann}`", b.name)
                        })?;
                        params.push((b.name.to_string(), ty));
                    }
                    if let Some(t) = &f.return_type
                        && !matches!(t.type_annotation, TSType::TSVoidKeyword(_))
                    {
                        return Err(format!("{what}: an RPC returns nothing (`void`)"));
                    }
                    let start = m
                        .decorators
                        .iter()
                        .map(|d| d.span.start)
                        .min()
                        .unwrap_or(m.span.start)
                        .min(m.span.start) as usize;
                    // The wrapper sends (or runs) the call; the original body moves to `__rpc_<key>`.
                    let decl: Vec<String> = params
                        .iter()
                        .map(|(n, t)| format!("{n}: {}", t.ts()))
                        .collect();
                    let names: Vec<&str> = params.iter().map(|(n, _)| n.as_str()).collect();
                    let sent = if params.is_empty() {
                        "\"\"".to_string()
                    } else {
                        let parts: Vec<String> = params.iter().map(|(n, t)| t.encode(n)).collect();
                        format!("`{}`", parts.join("\\u001f"))
                    };
                    let body_text = slice(src, body.span).to_string();
                    edits.push(Edit {
                        start,
                        end: m.span.end as usize,
                        text: format!(
                            "{key}({decl}): void {{ if (__net_send(this.netId, \"{key}\", \"{target}\", {sent})) this.__rpc_{key}({call}); }}\n  __rpc_{key}({decl}): void {body_text}",
                            decl = decl.join(", "),
                            call = names.join(", "),
                        ),
                    });
                    rpcs.push(RpcMethod {
                        key,
                        target,
                        flags,
                        params,
                    });
                }
                _ => {}
            }
        }
        if fields.is_empty() && rpcs.is_empty() {
            continue;
        }
        let extends_ok = matches!(
            c.heritage.as_ref().map(|h| &h.expression),
            Some(Expression::Identifier(p)) if p.name == "NetworkBehaviour"
        );
        if !extends_ok {
            return Err(format!(
                "class {name}: a class with @sync fields or @rpc methods must extend NetworkBehaviour"
            ));
        }
        // What the class gains, just before its closing brace.
        let mut extra = String::new();
        let spec: Vec<String> = fields
            .iter()
            .map(|f| {
                let mut s = format!("{}:{}", f.key, f.ty.wire(f.int));
                if let Some(q) = f.quantize {
                    s += &format!(":q={q}");
                }
                if f.owner {
                    s += ":owner";
                }
                if f.interpolate {
                    s += ":lerp";
                }
                s
            })
            .collect();
        let mut define = format!("Network.defineClass(\"{name}\", \"{}\")", spec.join(";"));
        for r in &rpcs {
            define += &format!(
                " && Network.defineRpc(\"{}\", \"{}\", \"{}\")",
                r.key,
                r.target,
                r.flags.join(",")
            );
        }
        extra += &format!("\n  static defineNetwork(): boolean {{ return {define}; }}");
        let inits: Vec<String> = fields
            .iter()
            .filter_map(|f| f.init.as_ref().map(|v| format!("this.{} = {v};", f.key)))
            .collect();
        extra += &format!("\n  netInit(): void {{ {} }}", inits.join(" "));
        extra += &format!(
            "\n  static spawnNet(owner: number): {name} {{ const o = new {name}(); o.netId = Network.spawn(\"{name}\", \"{name}\", owner); o.netInit(); return o; }}"
        );
        let arms: Vec<String> = rpcs
            .iter()
            .map(|r| {
                let args: Vec<String> = r
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, (_, t))| t.decode(&format!("args[{i}]")))
                    .collect();
                format!(
                    "if (name === \"{}\") {{ this.__rpc_{}({}); return true; }}",
                    r.key,
                    r.key,
                    args.join(", ")
                )
            })
            .collect();
        extra += &format!(
            "\n  netDispatch(name: string, args: string[]): boolean {{ {} return false; }}\n",
            arms.join(" ")
        );
        let close = c.body.span.end as usize - 1;
        edits.push(Edit {
            start: close,
            end: close,
            text: extra,
        });
    }
    if edits.is_empty() {
        return Ok(None);
    }
    // Later edits first, so earlier offsets stay true.
    edits.sort_by_key(|e| std::cmp::Reverse((e.start, e.end)));
    let mut out = src.to_string();
    for e in edits {
        out.replace_range(e.start..e.end, &e.text);
    }
    // The base class goes first (a class must be declared before its subclasses).
    if !src.contains("class NetworkBehaviour") {
        out.insert_str(0, BASE_CLASS);
    }
    out.push_str(HELPERS);
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = r#"
class Player extends NetworkBehaviour {
  @sync({ quantize: 0.01, interpolate: true }) pos: Vector3 = Vector3.zero;
  @sync hp: number = 100;
  @sync({ owner: true, int: true }) kills: number = 0;
  @sync name: string = "x";
  other = 1;
  @rpc("server") fire(aim: number, hit: boolean): void { this.other = aim; }
  @rpc("all", { unreliable: true }) beam(to: Vector3): void { }
}
"#;

    // M8-DEC-01
    #[test]
    fn sync_fields_become_accessors_and_the_class_defines_its_schema() {
        let out = rewrite(PLAYER).unwrap().unwrap();
        assert!(out.contains("get hp(): number { return Network.getNumber(this.netId, \"hp\"); }"));
        assert!(out.contains("set hp(v: number) { Network.setNumber(this.netId, \"hp\", v); }"));
        assert!(out.contains(
            "set pos(v: Vector3) { Network.setVec3(this.netId, \"pos\", v.x, v.y, v.z); }"
        ));
        assert!(out.contains(
            "Network.defineClass(\"Player\", \"pos:vec3:q=0.01:lerp;hp:f32;kills:i32:owner;name:str\")"
        ));
        assert!(out.contains("Network.defineRpc(\"fire\", \"server\", \"\")"));
        assert!(out.contains("Network.defineRpc(\"beam\", \"all\", \"unreliable\")"));
        // The decorators are gone, the plain field stays.
        assert!(!out.contains("@sync") && !out.contains("@rpc"));
        assert!(out.contains("other = 1;"));
        // Initial values are set by spawnNet through netInit.
        assert!(out.contains("this.hp = 100;") && out.contains("this.name = \"x\";"));
    }

    // M8-DEC-02
    #[test]
    fn rpc_methods_get_a_sending_wrapper_a_moved_body_and_a_dispatch_arm() {
        let out = rewrite(PLAYER).unwrap().unwrap();
        assert!(out.contains("fire(aim: number, hit: boolean): void { if (__net_send(this.netId, \"fire\", \"server\", `f:${aim}\\u001fb:${hit ? 1 : 0}`)) this.__rpc_fire(aim, hit); }"));
        assert!(out.contains("__rpc_fire(aim: number, hit: boolean): void { this.other = aim; }"));
        assert!(out.contains("if (name === \"fire\") { this.__rpc_fire(parseFloat((args[0] ?? \"\").slice(2)), (args[1] ?? \"\") === \"b:1\"); return true; }"));
        assert!(out.contains("__net_vec(args[0] ?? \"\")"));
        // The base class and the helpers are added once.
        assert_eq!(out.matches("class NetworkBehaviour").count(), 1);
        assert_eq!(out.matches("function __net_send").count(), 1);
    }

    // M8-DEC-03
    #[test]
    fn misuse_is_refused_with_the_reason() {
        let bad = |src: &str| rewrite(src).unwrap_err();
        assert!(bad("class A { @sync hp: number = 1; }").contains("must extend NetworkBehaviour"));
        assert!(
            bad("class A extends NetworkBehaviour { @sync hp: Map<number, number>; }")
                .contains("the type must be")
        );
        assert!(
            bad("class A extends NetworkBehaviour { @rpc(\"nobody\") f(): void {} }")
                .contains("the target must be")
        );
        assert!(
            bad("class A extends NetworkBehaviour { @rpc() f(): number { return 1; } }")
                .contains("returns nothing")
        );
        assert!(
            bad("class A extends NetworkBehaviour { @sync({ quantize: x }) hp: number = 1; }")
                .contains("compile time")
        );
        assert!(
            bad("class A extends NetworkBehaviour { @sync @other hp: number = 1; }")
                .contains("no other decorator")
        );
        // Nothing decorated, or not a parse: left alone.
        assert_eq!(rewrite("class A {}").unwrap(), None);
        assert_eq!(rewrite("@sync @@@ not valid").unwrap(), None);
    }
}
