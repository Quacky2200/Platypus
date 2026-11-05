// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            pub const fn raw(self) -> u32 {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

typed_id!(
    /// An entity in the game world. Owned by Rust; scripts only hold the number.
    EntityId
);
typed_id!(
    /// A script component instance. Scripts look instances up by this ID.
    ComponentId
);
typed_id!(
    /// A UI node created by the React reconciler.
    NodeId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_types_with_readable_debug() {
        let e = EntityId(7);
        let c = ComponentId(7);
        assert_eq!(e.raw(), c.raw());
        assert_eq!(format!("{e:?}"), "EntityId(7)");
        assert_eq!(format!("{c}"), "7");
    }
}
