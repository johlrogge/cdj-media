//! Times in milliseconds.

use std::time::Duration;

/// A time or duration in milliseconds, as Pioneer stores cue and beat positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Millis(u32);

impl Millis {
    /// Creates a time from a number of milliseconds.
    pub const fn new(ms: u32) -> Self {
        Self(ms)
    }

    /// The number of milliseconds.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for Millis {
    fn from(ms: u32) -> Self {
        Self(ms)
    }
}

impl From<Millis> for Duration {
    fn from(ms: Millis) -> Self {
        Duration::from_millis(u64::from(ms.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millis_converts_to_duration() {
        assert_eq!(
            Duration::from(Millis::new(1_500)),
            Duration::from_millis(1_500)
        );
        assert_eq!(Millis::from(7).get(), 7);
    }
}
