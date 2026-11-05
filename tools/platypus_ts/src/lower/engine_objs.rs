// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Engine objects made the way gameplay code writes them: `new GameObject("Main Camera", {
//! position: new Vector3(0, 0, 10) })` and `go.addComponent(Camera, { orthographic: true })`.
//!
//! Both are expanded at compile time over the engine API: `GameObject.create()` then the setter
//! of `name` and of each option (an option the object has no property for is looked up on its
//! `transform`, so `position` and `rotation` work), and `Camera.attach(go)` then the setter of
//! each option. A key with no setter is an error that names it.

use super::expr::mk;
use super::*;
use crate::manifest::ExternFn;

impl<'a> Lowerer<'a> {
    /// Does `new Class(..)` mean "create an engine object" (the class has a static `create`)?
    pub(crate) fn is_creatable(&self, class: &str) -> bool {
        self.manifest
            .classes
            .get(class)
            .is_some_and(|k| k.statics.contains_key("create"))
    }

    /// `new GameObject(name?, options?)`.
    pub(crate) fn lower_new_handle(&mut self, class: &str, n: &'a NewExpression<'a>) -> R<Expr> {
        self.lower_new_handle_args(class, &n.arguments, n.span.start)
    }

    /// `Class.create({ .. })` on a class whose `create` takes nothing: the same as
    /// `new Class({ .. })`, the object's keys set as properties.
    pub(crate) fn is_create_with_options(&self, class: &str, c: &CallExpression<'_>) -> bool {
        self.manifest
            .classes
            .get(class)
            .and_then(|k| k.statics.get("create"))
            .is_some_and(|f| f.params.is_empty())
            && matches!(c.arguments.as_slice(), [Argument::ObjectExpression(_)])
    }

    /// `new Class(args)` over the arguments (also `Class.create({ .. })`).
    pub(crate) fn lower_new_handle_args(
        &mut self,
        class: &str,
        arguments: &'a [Argument<'a>],
        start: u32,
    ) -> R<Expr> {
        let at = self.loc(start);
        // `new Material(color, emission, smoothness)`: positional arguments are the class's
        // `#[constructor]`.
        if !arguments.is_empty()
            && !matches!(arguments.first(), Some(Argument::ObjectExpression(_)))
            && let Some(ctor) = self.manifest.classes[class].ctor.clone()
        {
            let mut e =
                self.call_ext_args(ctor, None, &format!("new {class}"), start, arguments)?;
            e.ty = Ty::Handle(class.to_string());
            return Ok(e);
        }
        let k = self.manifest.classes[class].clone();
        let mut create = k.statics["create"].clone();
        // `create` declared more than once (`new Sprite(texture, rect, pivot)`): the version for
        // these arguments.
        if let Some(cands) = k
            .overloads
            .get(&("static".to_string(), "create".to_string()))
        {
            create = self.pick_ext_overload(cands, 0, arguments);
        }
        let ty = Ty::Handle(class.to_string());
        // `create(a, b)`: the constructor arguments are its parameters (`new Rng(seed)`).
        if !create.params.is_empty() {
            // A trailing `{ key: value }` after the required parameters sets properties once the
            // object is made (`new AnimationTransition("A", "B", { duration: 0.2 })`), unless its
            // keys are parameters of `create` (then call_ext_args maps them by name).
            let args: &'a [Argument<'a>] = arguments;
            let required = create.optional_from.min(create.params.len());
            let options = match args.last() {
                Some(Argument::ObjectExpression(o))
                    if args.len() > required
                        && !o.properties.iter().all(|p| {
                            matches!(p, ObjectPropertyKind::ObjectProperty(p)
                                if matches!(&p.key, PropertyKey::StaticIdentifier(k)
                                    if create.params.iter().any(|(n, _)| n == k.name.as_str())))
                        }) =>
                {
                    Some(&**o)
                }
                _ => None,
            };
            let positional = if options.is_some() {
                &args[..args.len() - 1]
            } else {
                args
            };
            let mut e =
                self.call_ext_args(create, None, &format!("new {class}"), start, positional)?;
            e.ty = ty.clone();
            let Some(o) = options else {
                return Ok(e);
            };
            let tmp = self.new_local(ty.clone());
            let mut stmts = vec![Stmt::Assign(tmp, e)];
            self.apply_options(class, tmp, o, &mut stmts, &at)?;
            let result = self.var_expr(tmp);
            return Ok(mk(ExprKind::Seq(stmts, Box::new(result)), ty));
        }
        let tmp = self.new_local(ty.clone());
        let made = self.emit_extern(&create, vec![]);
        let mut stmts = vec![Stmt::Assign(tmp, made)];
        for (i, a) in arguments.iter().enumerate() {
            let Some(e) = a.as_expression() else {
                bail!("{at}: new {class}: spread argument")
            };
            match e {
                Expression::ObjectExpression(o) if i > 0 || arguments.len() == 1 => {
                    self.apply_options(class, tmp, o, &mut stmts, &at)?;
                }
                // `new GameObject(name, parent ? { position, parent } : { position })`
                Expression::ConditionalExpression(_) if i > 0 => {
                    self.apply_options_expr(class, tmp, e, &mut stmts, &at)?;
                }
                _ if i == 0 => {
                    let Some(setter) = k.setters.get("name").cloned() else {
                        bail!("{at}: {class} has no `name` to set from new {class}(name)")
                    };
                    let v = self.lower_expr(e, Some(&Ty::Str))?;
                    let v = self.coerce(v, &Ty::Str)?;
                    let h = self.var_expr(tmp);
                    stmts.push(Stmt::Eval(self.emit_extern(&setter, vec![h, v])));
                }
                _ => {
                    // `new GameObject(name, opts)` with `opts` a variable of an object type.
                    let v = self.lower_expr(e, None)?;
                    self.apply_options_value(class, tmp, v, &mut stmts, &at)?;
                }
            }
        }
        let result = self.var_expr(tmp);
        Ok(mk(ExprKind::Seq(stmts, Box::new(result)), ty))
    }

    /// Is `m` a host-class call that looks a class up: `get`, `getAll` or, with `add`, `add`
    /// (`getComponent`, `getComponents`, `addComponent`)?
    pub(crate) fn is_host_lookup(&self, m: &str, add: bool) -> bool {
        self.manifest.is_host_method(m, |h| &h.get)
            || self.manifest.is_host_method(m, |h| &h.get_all)
            || (add && self.manifest.is_host_method(m, |h| &h.add))
    }

    /// Is `m` a method of the host's extension that takes a class (`getComponentInChildren`)?
    pub(crate) fn is_ext_class_method(&self, m: &str) -> bool {
        self.ext.is_some_and(|e| e.class_methods().contains(&m))
    }

    /// The base-class field that holds a host-class instance's object (`gameObject`).
    pub(crate) fn host_object_field(&self) -> Option<&str> {
        self.manifest.host_classes.as_ref()?.object_field.as_deref()
    }

    /// The object a host-class call is on: an object handle, or an instance (its object field).
    pub(crate) fn component_target(&self, recv: Expr) -> Option<Expr> {
        match recv.ty.strip() {
            Ty::Handle(_) => Some(recv),
            Ty::Obj(sid) => {
                let sid = *sid;
                let idx = self.m.structs[sid].field(self.host_object_field()?)?;
                let ty = self.m.structs[sid].fields[idx].ty.strip().clone();
                Some(mk(ExprKind::Field(Box::new(recv), sid, idx), ty))
            }
            _ => None,
        }
    }

    /// `go.getComponent(Player)` / `go.addComponent(Player, { speed: 3 })` for a class of this
    /// program (registered with `register`). `None` when the argument is not such a class.
    pub(crate) fn lower_user_component_call(
        &mut self,
        go: &Expr,
        m: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Option<Expr>> {
        let at = self.loc(c.span.start);
        let Some(Expression::Identifier(cls)) = c.arguments.first().and_then(|a| a.as_expression())
        else {
            return Ok(None);
        };
        let name = cls.name.to_string();
        if !self.class_decls.contains_key(&name) {
            // An engine component (`Camera`): its static `findOn(go)`.
            if self.manifest.is_host_method(m, |h| &h.get)
                && let Some(find) = self
                    .manifest
                    .classes
                    .get(&name)
                    .and_then(|k| k.statics.get("findOn"))
                    .cloned()
            {
                let mut e = self.emit_extern(&find, vec![go.clone()]);
                e.ty = Ty::Handle(name).nullable();
                return Ok(Some(e));
            }
            return Ok(None);
        }
        let Ty::Obj(sid) = self.named_type(&name)? else {
            return Ok(None);
        };
        let which = mk(ExprKind::Num(sid as f64), Ty::Num);
        if self.manifest.is_host_method(m, |h| &h.get) {
            let call = ExprKind::Rt(RtFn::Str("ts_rt_host_get"), vec![go.clone(), which]);
            return Ok(Some(mk(call, Ty::Obj(sid).nullable())));
        }
        if self.manifest.is_host_method(m, |h| &h.get_all) {
            let call = ExprKind::Rt(RtFn::Str("ts_rt_host_all_gc"), vec![go.clone(), which]);
            return Ok(Some(mk(call, Ty::Arr(Box::new(Ty::Obj(sid))))));
        }
        if self.is_ext_class_method(m)
            && let Some(ext) = self.ext
        {
            // The host's own class-taking method (a hierarchy search).
            return ext.class_call(self, go, m, &name);
        }
        let ty = Ty::Obj(sid);
        let tmp = self.new_local(ty.clone());
        let made = ExprKind::Rt(RtFn::Str("ts_rt_host_add_gc"), vec![go.clone(), which]);
        let mut stmts = vec![Stmt::Assign(tmp, mk(made, ty.clone()))];
        if let Some(e) = c.arguments.get(1).and_then(|a| a.as_expression()) {
            let Expression::ObjectExpression(o) = e else {
                // A variable of an object type (a `Partial<MyComponent>`): its fields that are there.
                let v = self.lower_expr(e, None)?;
                self.component_fields_from(sid, tmp, v, &mut stmts, &at)?;
                let result = self.var_expr(tmp);
                return Ok(Some(mk(ExprKind::Seq(stmts, Box::new(result)), ty)));
            };
            for p in &o.properties {
                let p = match p {
                    ObjectPropertyKind::ObjectProperty(p) => p,
                    ObjectPropertyKind::SpreadProperty(s) => {
                        let v = self.lower_expr(&s.argument, None)?;
                        self.component_fields_from(sid, tmp, v, &mut stmts, &at)?;
                        continue;
                    }
                };
                let PropertyKey::StaticIdentifier(key) = &p.key else {
                    bail!("{at}: option names are plain names")
                };
                let Some(idx) = self.m.structs[sid].field(key.name.as_str()) else {
                    bail!("{at}: {name} has no field `{}` to set", key.name)
                };
                let want = self.m.structs[sid].fields[idx].ty.clone();
                let v = self.lower_expr(&p.value, Some(&want))?;
                let v = self.coerce(v, &want)?;
                stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, idx, v));
            }
        }
        let result = self.var_expr(tmp);
        Ok(Some(mk(ExprKind::Seq(stmts, Box::new(result)), ty)))
    }

    /// `go.removeComponent(c)` and `c.destroy()` for a component of this program: it is gone for
    /// `getComponent` at once and its `onDestroy` runs when the calling hook returns.
    pub(crate) fn lower_remove_component(&mut self, component: Expr) -> Expr {
        mk(
            ExprKind::Rt(RtFn::Str("ts_rt_host_remove"), vec![component]),
            Ty::Void,
        )
    }

    /// `GameObject.findObjectOfType(Player)`: the first component of that class on any object.
    pub(crate) fn lower_find_object_of_type(
        &mut self,
        c: &'a CallExpression<'a>,
    ) -> R<Option<Expr>> {
        let Some(Expression::Identifier(cls)) = c.arguments.first().and_then(|a| a.as_expression())
        else {
            return Ok(None);
        };
        let name = cls.name.to_string();
        if !self.class_decls.contains_key(&name) {
            return Ok(None);
        }
        let Ty::Obj(sid) = self.named_type(&name)? else {
            return Ok(None);
        };
        let which = mk(ExprKind::Num(sid as f64), Ty::Num);
        let call = ExprKind::Rt(RtFn::Str("ts_rt_host_find_any"), vec![which]);
        Ok(Some(mk(call, Ty::Obj(sid).nullable())))
    }

    /// `go.addComponent(Camera, { .. })` for an engine component class (one with a static `attach`).
    pub(crate) fn lower_add_component(&mut self, recv: Expr, c: &'a CallExpression<'a>) -> R<Expr> {
        if let Some(e) = self.lower_user_component_call(&recv, "addComponent", c)? {
            return Ok(e);
        }
        let at = self.loc(c.span.start);
        let Some(Expression::Identifier(cls)) = c.arguments.first().and_then(|a| a.as_expression())
        else {
            bail!("{at}: addComponent takes a component class")
        };
        let class = cls.name.to_string();
        let Some(attach) = self
            .manifest
            .classes
            .get(&class)
            .and_then(|k| k.statics.get("attach"))
            .cloned()
        else {
            bail!("{at}: `{class}` is not a component the engine API can add")
        };
        let ty = Ty::Handle(class.clone());
        let tmp = self.new_local(ty.clone());
        let made = self.emit_extern(&attach, vec![recv]);
        let mut stmts = vec![Stmt::Assign(tmp, made)];
        if let Some(e) = c.arguments.get(1).and_then(|a| a.as_expression()) {
            self.apply_options_expr(&class, tmp, e, &mut stmts, &at)?;
        }
        let result = self.var_expr(tmp);
        Ok(mk(ExprKind::Seq(stmts, Box::new(result)), ty))
    }

    /// The fields of an options object (`{ ...look }`, a `Partial<MyComponent>`) that are there,
    /// stored into the script component in `tmp`.
    fn component_fields_from(
        &mut self,
        sid: StructId,
        tmp: VarId,
        value: Expr,
        stmts: &mut Vec<Stmt>,
        at: &str,
    ) -> R<()> {
        let Ty::Obj(ssid) = value.ty.strip().clone() else {
            bail!(
                "{at}: component options must be an object, found {}",
                value.ty
            )
        };
        let src = self.new_local(value.ty.clone());
        stmts.push(Stmt::Assign(src, value));
        for fi in 0..self.m.structs[ssid].fields.len() {
            let (key, fty) = {
                let f = &self.m.structs[ssid].fields[fi];
                (f.name.clone(), f.ty.clone())
            };
            let Some(idx) = self.m.structs[sid].field(&key) else {
                bail!(
                    "{at}: {} has no field `{key}` to set",
                    self.m.structs[sid].name
                )
            };
            let want = self.m.structs[sid].fields[idx].ty.clone();
            let read = self.field_read(src, ssid, fi, fty.clone());
            let read = self.coerce(read, &want)?;
            let store = Stmt::StoreField(self.var_expr(tmp), sid, idx, read);
            if fty.is_nullable() && !want.is_nullable() {
                let absent = mk(
                    ExprKind::IsNull(Box::new(self.field_read(src, ssid, fi, fty))),
                    Ty::Bool,
                );
                stmts.push(Stmt::If(absent, vec![], vec![store]));
            } else {
                stmts.push(store);
            }
        }
        Ok(())
    }

    /// The options of `Class(.., options)`: an object literal, a `cond ? { .. } : { .. }`, or a
    /// variable of an object type (a `Partial<LineRenderer2D>`).
    pub(crate) fn apply_options_expr(
        &mut self,
        class: &str,
        tmp: VarId,
        e: &'a Expression<'a>,
        stmts: &mut Vec<Stmt>,
        at: &str,
    ) -> R<()> {
        match e {
            Expression::ObjectExpression(o) => self.apply_options(class, tmp, o, stmts, at),
            Expression::ParenthesizedExpression(p) => {
                self.apply_options_expr(class, tmp, &p.expression, stmts, at)
            }
            Expression::ConditionalExpression(c) => {
                let test = self.lower_cond(&c.test)?;
                let (mut yes, mut no) = (Vec::new(), Vec::new());
                self.apply_options_expr(class, tmp, &c.consequent, &mut yes, at)?;
                self.apply_options_expr(class, tmp, &c.alternate, &mut no, at)?;
                stmts.push(Stmt::If(test, yes, no));
                Ok(())
            }
            _ => {
                let v = self.lower_expr(e, None)?;
                self.apply_options_value(class, tmp, v, stmts, at)
            }
        }
    }

    /// How one option key of an engine class is set.
    fn option_setter(&mut self, class: &str, key: &str, array: bool) -> R<Option<OptSet>> {
        if let Some((fid, want)) = self.ext_setter(class, key)? {
            return Ok(Some(OptSet::Facade(fid, want)));
        }
        let mut own = self.manifest.classes[class]
            .setters
            .get(key)
            .cloned()
            .or_else(|| self.set_method(class, key));
        // A setter of two versions (`startLifetime` takes a number or `[min, max]`): the one for
        // a list when the value is one.
        if let Some(versions) = self.manifest.classes[class]
            .overloads
            .get(&("set".to_string(), key.to_string()))
            && let Some(f) = versions
                .iter()
                .find(|f| f.params.get(1).is_some_and(|p| p.1.is_list_arg()) == array)
        {
            own = Some(f.clone());
        }
        if let Some(setter) = own {
            let want = self.ext_ir(&setter.params[1].1);
            return Ok(Some(OptSet::Own(setter, want)));
        }
        // `position` of a GameObject is its transform's.
        let transform = self.manifest.classes[class]
            .getters
            .get("transform")
            .cloned();
        let tr_setter = self
            .manifest
            .classes
            .get("Transform")
            .and_then(|t| t.setters.get(key))
            .cloned();
        if let (Some(get), Some(setter)) = (transform, tr_setter) {
            let want = self.ext_ir(&setter.params[1].1);
            return Ok(Some(OptSet::Transform(get, setter, want)));
        }
        Ok(None)
    }

    /// The type a value for this setter has.
    fn opt_want(set: &OptSet) -> &Ty {
        match set {
            OptSet::Facade(_, w) | OptSet::Own(_, w) | OptSet::Transform(_, _, w) => w,
        }
    }

    /// The statement that sets the property on the handle in `tmp`; `v` is already of the wanted type.
    fn opt_emit(&mut self, tmp: VarId, set: OptSet, v: Expr) -> Stmt {
        let h = self.var_expr(tmp);
        match set {
            OptSet::Facade(fid, _) => Stmt::Eval(Self::target_call(
                &classes::Target::Direct(fid),
                vec![h, v],
                Ty::Void,
            )),
            OptSet::Own(setter, _) => Stmt::Eval(self.emit_extern(&setter, vec![h, v])),
            OptSet::Transform(get, setter, _) => {
                let tr = self.emit_extern(&get, vec![h]);
                Stmt::Eval(self.emit_extern(&setter, vec![tr, v]))
            }
        }
    }

    /// `{ ...look }` / a non-literal options value of an object type (a `Partial<Class>`): every
    /// field that is there sets its property.
    pub(crate) fn apply_options_value(
        &mut self,
        class: &str,
        tmp: VarId,
        value: Expr,
        stmts: &mut Vec<Stmt>,
        at: &str,
    ) -> R<()> {
        let Ty::Obj(sid) = value.ty.strip().clone() else {
            bail!(
                "{at}: options for {class} must be an object, found {}",
                value.ty
            )
        };
        let src = self.new_local(value.ty.clone());
        stmts.push(Stmt::Assign(src, value));
        for fi in 0..self.m.structs[sid].fields.len() {
            let (key, fty) = {
                let f = &self.m.structs[sid].fields[fi];
                (f.name.clone(), f.ty.clone())
            };
            let array = matches!(fty.strip(), Ty::Arr(_));
            let Some(set) = self.option_setter(class, &key, array)? else {
                bail!("{at}: {class} has no property `{key}` to set")
            };
            let want = Self::opt_want(&set).clone();
            let read = mk(
                ExprKind::Field(Box::new(self.var_expr(src)), sid, fi),
                fty.clone(),
            );
            let plain = mk(ExprKind::Cast(Box::new(read.clone())), fty.strip().clone());
            let v = self.coerce_ext(if fty.is_nullable() { plain } else { read }, &want)?;
            let store = self.opt_emit(tmp, set, v);
            if fty.is_nullable() {
                let absent = mk(
                    ExprKind::IsNull(Box::new(self.field_read(src, sid, fi, fty))),
                    Ty::Bool,
                );
                stmts.push(Stmt::If(absent, vec![], vec![store]));
            } else {
                stmts.push(store);
            }
        }
        Ok(())
    }

    fn field_read(&self, src: VarId, sid: StructId, fi: usize, fty: Ty) -> Expr {
        mk(ExprKind::Field(Box::new(self.var_expr(src)), sid, fi), fty)
    }

    /// The setters for `{ key: value, .. }` on the handle in `tmp`.
    pub fn apply_options(
        &mut self,
        class: &str,
        tmp: VarId,
        o: &'a ObjectExpression<'a>,
        stmts: &mut Vec<Stmt>,
        at: &str,
    ) -> R<()> {
        // `parent` first: `position`, `rotation` and `scale` of a GameObject are local to it.
        let mut props: Vec<&ObjectPropertyKind<'a>> = o.properties.iter().collect();
        props.sort_by_key(|p| {
            !matches!(p, ObjectPropertyKind::ObjectProperty(p)
                if matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "parent"))
        });
        for p in props {
            let p = match p {
                ObjectPropertyKind::ObjectProperty(p) => p,
                ObjectPropertyKind::SpreadProperty(s) => {
                    let v = self.lower_expr(&s.argument, None)?;
                    self.apply_options_value(class, tmp, v, stmts, at)?;
                    continue;
                }
            };
            let PropertyKey::StaticIdentifier(key) = &p.key else {
                bail!("{at}: option names are plain names")
            };
            let key = key.name.as_str();
            if let Some(ext) = self.ext {
                let o = super::ext::OptionArg {
                    class,
                    tmp,
                    key,
                    value: &p.value,
                    stmts,
                    at,
                };
                if ext.option(self, o)? {
                    continue;
                }
            }
            let array = matches!(&p.value, Expression::ArrayExpression(_));
            let Some(set) = self.option_setter(class, key, array)? else {
                bail!("{at}: {class} has no property `{key}` to set")
            };
            let want = Self::opt_want(&set).clone();
            let v = self.lower_expr(&p.value, Some(&want))?;
            let v = if matches!(set, OptSet::Facade(..) | OptSet::Transform(..)) {
                self.coerce(v, &want)?
            } else {
                self.coerce_ext(v, &want)?
            };
            let st = self.opt_emit(tmp, set, v);
            stmts.push(st);
        }
        Ok(())
    }
}

/// How an option of an engine class is set (see `option_setter`).
pub(crate) enum OptSet {
    /// A facade function taking the object and the value.
    Facade(FuncId, Ty),
    /// The class's own setter (or `setX` method).
    Own(ExternFn, Ty),
    /// A property of the object's transform: its getter on the object, the transform's setter.
    Transform(ExternFn, ExternFn, Ty),
}
