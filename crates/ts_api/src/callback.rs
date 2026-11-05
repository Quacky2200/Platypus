// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Closures given to the engine: `input.onKeyDown((key) => ...)`.
//!
//! A shim for a function with a closure parameter wraps the script closure in a [`Callback`]. The
//! engine keeps it (the closure stays rooted until the `Callback` is dropped) and calls it with
//! `cb.call((a, b))`. The script runtime tells this crate how to root a closure and how to make a
//! script string through a [`ScriptBridge`], so engine crates never link to the runtime.

use crate::Handle;
use std::rc::Rc;
use std::sync::OnceLock;

/// What the script runtime gives the engine to handle closures.
pub struct ScriptBridge {
    /// Keeps a script object alive (a host root); returns the root's index.
    pub root_add: fn(*mut u8) -> usize,
    pub root_remove: fn(usize),
    /// A new script string from UTF-8 text.
    pub str_new: fn(&str) -> *mut u8,
    /// Takes a pending exception; true when there was one.
    pub exc_take: fn() -> bool,
}

static BRIDGE: OnceLock<ScriptBridge> = OnceLock::new();

/// Installs the runtime's bridge (the first call wins).
pub fn install_bridge(b: ScriptBridge) {
    let _ = BRIDGE.set(b);
}

/// The layout of a script closure (`platypus_ts_rt::Closure`).
#[repr(C)]
struct ScriptClosure {
    header: u64,
    func: *const u8,
    env: *mut u8,
}

/// A closure rooted for as long as it lives.
struct Rooted {
    ptr: *mut u8,
    root: usize,
}

impl Drop for Rooted {
    fn drop(&mut self) {
        if let Some(b) = BRIDGE.get() {
            (b.root_remove)(self.root);
        }
    }
}

/// One argument type of a callback.
pub trait CallArg {
    /// The value on the C ABI.
    type Abi: Copy;
    /// Makes the ABI value; a string allocates a script string (rooted: its root index goes on
    /// `roots` and is released after the call).
    fn to_abi(&self, roots: &mut Vec<usize>) -> Self::Abi;
}

impl CallArg for f64 {
    type Abi = f64;
    fn to_abi(&self, _: &mut Vec<usize>) -> f64 {
        *self
    }
}
impl CallArg for i32 {
    type Abi = f64;
    fn to_abi(&self, _: &mut Vec<usize>) -> f64 {
        *self as f64
    }
}
impl CallArg for u32 {
    type Abi = f64;
    fn to_abi(&self, _: &mut Vec<usize>) -> f64 {
        *self as f64
    }
}
impl CallArg for bool {
    type Abi = bool;
    fn to_abi(&self, _: &mut Vec<usize>) -> bool {
        *self
    }
}
impl CallArg for String {
    type Abi = *const u8;
    fn to_abi(&self, roots: &mut Vec<usize>) -> *const u8 {
        match BRIDGE.get() {
            Some(b) => {
                let p = (b.str_new)(self);
                roots.push((b.root_add)(p));
                p
            }
            None => std::ptr::null(),
        }
    }
}
/// A handle argument travels as its raw 64 bits.
pub struct HandleArg<H: Handle>(pub H);
impl<H: Handle> CallArg for HandleArg<H> {
    type Abi = u64;
    fn to_abi(&self, _: &mut Vec<usize>) -> u64 {
        self.0.to_raw()
    }
}

/// A tuple of [`CallArg`]s: the arguments of one call.
pub trait CallArgs: 'static {
    /// # Safety
    /// `c` must be a live script closure taking exactly these arguments and returning nothing.
    unsafe fn invoke(&self, c: *mut u8, roots: &mut Vec<usize>);
}

macro_rules! call_args {
    ($(($($t:ident $i:tt),*)),*) => {$(
        impl<$($t: CallArg + 'static),*> CallArgs for ($($t,)*) {
            #[allow(unused_variables)]
            unsafe fn invoke(&self, c: *mut u8, roots: &mut Vec<usize>) {
                // SAFETY: by contract.
                unsafe {
                    let c = c as *mut ScriptClosure;
                    let f: unsafe extern "C" fn(*mut u8 $(, <$t as CallArg>::Abi)*) =
                        std::mem::transmute((*c).func);
                    f((*c).env $(, self.$i.to_abi(roots))*);
                }
            }
        }
    )*};
}
call_args!(
    (),
    (A 0),
    (A 0, B 1),
    (A 0, B 1, C 2),
    (A 0, B 1, C 2, D 3)
);

/// A function the engine can call with `A`: a script closure, or a Rust one in tests and when the
/// host itself subscribes. Cloning shares the closure.
pub struct Callback<A> {
    f: Rc<dyn Fn(A)>,
}

impl<A> Clone for Callback<A> {
    fn clone(&self) -> Self {
        Callback { f: self.f.clone() }
    }
}

impl<A: 'static> Callback<A> {
    /// A callback that runs a Rust closure.
    pub fn from_fn(f: impl Fn(A) + 'static) -> Callback<A> {
        Callback { f: Rc::new(f) }
    }

    /// Calls it. An exception in a script closure is taken and reported through [`take_errors`].
    pub fn call(&self, args: A) {
        (self.f)(args)
    }
}

impl<A: CallArgs> Callback<A> {
    /// Wraps a script closure (the pointer a shim received). It is rooted until the last clone
    /// is dropped.
    ///
    /// # Safety
    /// `p` must be a live script closure that takes the arguments `A` and returns nothing.
    pub unsafe fn from_script(p: *const u8) -> Callback<A> {
        let Some(b) = BRIDGE.get() else {
            return Callback::from_fn(|_| {});
        };
        if p.is_null() {
            return Callback::from_fn(|_| {});
        }
        let rooted = Rooted {
            ptr: p as *mut u8,
            root: (b.root_add)(p as *mut u8),
        };
        Callback::from_fn(move |args: A| {
            // Using `rooted` as a whole makes the closure own it (a closure otherwise captures
            // only the pointer field and the root would be released at once).
            let rooted = &rooted;
            let mut roots = Vec::new();
            // SAFETY: by the contract of `from_script`.
            unsafe { args.invoke(rooted.ptr, &mut roots) };
            if let Some(b) = BRIDGE.get() {
                for r in roots {
                    (b.root_remove)(r);
                }
                if (b.exc_take)() {
                    ERRORS.with(|e| {
                        e.borrow_mut()
                            .push("a callback given to the engine threw an exception".into())
                    });
                }
            }
        })
    }
}

thread_local! {
    static ERRORS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Exceptions that script callbacks threw since the last call.
pub fn take_errors() -> Vec<String> {
    ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

impl<H: Handle> Clone for HandleArg<H> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<H: Handle> Copy for HandleArg<H> {}

thread_local! {
    static NEXT_TOKEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The callbacks one event has, by token (`input.onKeyDown(f)` gives a token, `input.off(token)`
/// removes it). Tokens are never 0 and never reused, and differ between events too.
pub struct Listeners<A> {
    list: Vec<(u32, Callback<A>)>,
}

impl<A> Default for Listeners<A> {
    fn default() -> Self {
        Listeners { list: Vec::new() }
    }
}

impl<A: Clone + 'static> Listeners<A> {
    pub fn add(&mut self, cb: Callback<A>) -> u32 {
        let t = NEXT_TOKEN.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        self.list.push((t, cb));
        t
    }

    /// Removes a listener; false when the token is unknown.
    pub fn remove(&mut self, token: u32) -> bool {
        let n = self.list.len();
        self.list.retain(|(t, _)| *t != token);
        self.list.len() != n
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// The callbacks now, to call after the state they live in is released (a listener may
    /// subscribe or unsubscribe).
    pub fn snapshot(&self) -> Vec<Callback<A>> {
        self.list.iter().map(|(_, c)| c.clone()).collect()
    }
}

/// Calls every callback of a snapshot with the arguments.
pub fn emit<A: Clone + 'static>(callbacks: &[Callback<A>], args: A) {
    for c in callbacks {
        c.call(args.clone());
    }
}
