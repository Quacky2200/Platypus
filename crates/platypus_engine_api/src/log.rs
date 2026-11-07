// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Logging the way Unity (`Debug.Log`), s&box (`Log.Info`) and C# (`Console.WriteLine`) spell it.
//! Everything goes to one sink the host sets ([`set_sink`]); with none, to stderr. Each function
//! takes one string (compiled code writes a template literal; `js/engine`'s versions joined many
//! arguments and printed objects as JSON).

use std::cell::RefCell;
use ts_api::ts_api;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// `console.debug`: hidden unless debug logging is on.
    Trace,
    Log,
    Info,
    Warn,
    Error,
}

type Sink = Box<dyn FnMut(Level, &str)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// Where log lines go; `None` puts them on stderr again.
pub fn set_sink(sink: Option<Sink>) {
    // The `Window` API (web-ui's crate) logs through its own sink: it follows this one.
    web_api_window::set_log_sink(sink.is_some().then(|| {
        Box::new(|level, message: &str| {
            let level = match level {
                web_api_window::LogLevel::Warn => Level::Warn,
                web_api_window::LogLevel::Error => Level::Error,
            };
            emit(level, message);
        }) as Box<dyn FnMut(web_api_window::LogLevel, &str)>
    }));
    SINK.with(|s| *s.borrow_mut() = sink);
}

pub fn emit(level: Level, message: &str) {
    SINK.with(|s| match s.borrow_mut().as_mut() {
        Some(f) => f(level, message),
        None => eprintln!("[{level:?}] {message}"),
    });
}

/// Unity's `Debug`.
#[ts_api(namespace = "Debug")]
pub mod debug_api {
    use super::{Level, emit};

    pub fn log(message: &str) {
        emit(Level::Log, message);
    }

    pub fn log_warning(message: &str) {
        emit(Level::Warn, message);
    }

    pub fn log_error(message: &str) {
        emit(Level::Error, message);
    }

    /// Logs an error with its text.
    pub fn log_exception(message: &str) {
        emit(Level::Error, message);
    }

    /// Logs an error when `condition` is false.
    #[optional(message)]
    pub fn assert(condition: bool, message: &str) {
        if !condition {
            let text = if message.is_empty() {
                "assertion failed"
            } else {
                message
            };
            emit(Level::Error, &format!("Assertion failed: {text}"));
        }
    }
}

/// s&box's `Log`.
#[ts_api(namespace = "Log")]
pub mod log_api {
    use super::{Level, emit};

    pub fn info(message: &str) {
        emit(Level::Info, message);
    }

    pub fn warning(message: &str) {
        emit(Level::Warn, message);
    }

    pub fn error(message: &str) {
        emit(Level::Error, message);
    }

    /// Verbose detail, hidden unless debug logging is on.
    pub fn trace(message: &str) {
        emit(Level::Trace, message);
    }
}

/// C#'s `Console`, with the JS names too.
#[ts_api(namespace = "Console")]
pub mod console_api {
    use super::{Level, emit};

    pub fn write_line(message: &str) {
        emit(Level::Log, message);
    }

    pub fn log(message: &str) {
        emit(Level::Log, message);
    }

    pub fn warn(message: &str) {
        emit(Level::Warn, message);
    }

    pub fn warning(message: &str) {
        emit(Level::Warn, message);
    }

    pub fn error(message: &str) {
        emit(Level::Error, message);
    }
}
