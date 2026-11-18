//! Row ids of the `export.pdb` tables.
//!
//! djl-analysis uses id 0 for "none" in references, and no row of the real-stick fixture
//! has id 0 (827 track rows and 1,394 rows in tables 1 to 6 and 13, checked by
//! `tests/pdb_rows_local.rs`), so
//! ids are non-zero. A reference that may be empty, such as an album's artist, is an
//! `Option` of an id.

use std::num::{NonZeroU16, NonZeroU32};

/// Defines a non-zero id newtype over `$nonzero`, whose raw value is `$raw`.
macro_rules! id {
    ($(#[$doc:meta])* $name:ident($nonzero:ty, $raw:ty)) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name($nonzero);

        impl $name {
            /// The id with raw value `value`, or `None` for 0, which means "none".
            pub const fn new(value: $raw) -> Option<Self> {
                match <$nonzero>::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            /// The raw value, never 0.
            pub const fn get(self) -> $raw {
                self.0.get()
            }
        }

        impl From<$name> for u32 {
            /// The raw value, widened to `u32` for the narrower colour id.
            fn from(id: $name) -> u32 {
                u32::from(id.get())
            }
        }
    };
}

id! {
    /// The id of a track row (table 0). No track row of the real-stick fixture has id 0
    /// (827 rows).
    TrackId(NonZeroU32, u32)
}

id! {
    /// The id of a genre row (table 1).
    GenreId(NonZeroU32, u32)
}

id! {
    /// The id of an artist row (table 2).
    ArtistId(NonZeroU32, u32)
}

id! {
    /// The id of an album row (table 3).
    AlbumId(NonZeroU32, u32)
}

id! {
    /// The id of a label row (table 4).
    LabelId(NonZeroU32, u32)
}

id! {
    /// The id of a musical key row (table 5).
    KeyId(NonZeroU32, u32)
}

id! {
    /// The id of a colour label row (table 6): 1 to 8 for pink, red, orange, yellow,
    /// green, aqua, blue and purple. 0, "no colour", is never a row.
    ColorId(NonZeroU16, u16)
}

id! {
    /// The id of an artwork row (table 13).
    ArtworkId(NonZeroU32, u32)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn zero_is_none() {
        assert_eq!(GenreId::new(0), None);
        assert_eq!(ArtistId::new(0), None);
        assert_eq!(AlbumId::new(0), None);
        assert_eq!(LabelId::new(0), None);
        assert_eq!(KeyId::new(0), None);
        assert_eq!(ColorId::new(0), None);
        assert_eq!(ArtworkId::new(0), None);
        assert_eq!(TrackId::new(0), None);
    }

    #[test]
    fn every_id_converts_to_its_raw_u32() {
        assert_eq!(u32::from(TrackId::new(70_000).unwrap()), 70_000);
        assert_eq!(u32::from(GenreId::new(1).unwrap()), 1);
        assert_eq!(u32::from(ArtistId::new(2).unwrap()), 2);
        assert_eq!(u32::from(AlbumId::new(3).unwrap()), 3);
        assert_eq!(u32::from(LabelId::new(4).unwrap()), 4);
        assert_eq!(u32::from(KeyId::new(5).unwrap()), 5);
        assert_eq!(u32::from(ColorId::new(0x1234).unwrap()), 0x1234);
        assert_eq!(u32::from(ArtworkId::new(7).unwrap()), 7);
    }

    proptest! {
        #[test]
        fn non_zero_values_round_trip(value in 1u32.., small in 1u16..) {
            prop_assert_eq!(GenreId::new(value).map(GenreId::get), Some(value));
            prop_assert_eq!(ArtistId::new(value).map(ArtistId::get), Some(value));
            prop_assert_eq!(AlbumId::new(value).map(AlbumId::get), Some(value));
            prop_assert_eq!(LabelId::new(value).map(LabelId::get), Some(value));
            prop_assert_eq!(KeyId::new(value).map(KeyId::get), Some(value));
            prop_assert_eq!(ArtworkId::new(value).map(ArtworkId::get), Some(value));
            prop_assert_eq!(TrackId::new(value).map(TrackId::get), Some(value));
            prop_assert_eq!(ColorId::new(small).map(ColorId::get), Some(small));
        }
    }
}
