//! Tempo and position in the bar.

use std::fmt;

/// A tempo in hundredths of a beat per minute, as Pioneer stores it: 120.00 BPM is 12000.
///
/// Stored as a `u32`, the width of the `export.pdb` track tempo. The ANLZ beat grid stores
/// tempos as `u16`, so encoding a beat grid fails for tempos above 655.35 BPM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Bpm(u32);

impl Bpm {
    /// Creates a tempo from hundredths of a BPM.
    pub const fn from_centi(centi: u32) -> Self {
        Self(centi)
    }

    /// The tempo in hundredths of a BPM.
    pub const fn centi(self) -> u32 {
        self.0
    }

    /// The tempo in BPM.
    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

impl fmt::Display for Bpm {
    /// Shows the tempo with two decimals, e.g. `120.00`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

/// The position of a beat in its four-beat bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BeatInBar {
    /// The downbeat.
    One,
    /// The second beat.
    Two,
    /// The third beat.
    Three,
    /// The fourth beat.
    Four,
}

impl BeatInBar {
    /// The position for beat number `n` (1 to 4), or `None` for any other number.
    pub const fn new(n: u16) -> Option<Self> {
        match n {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            3 => Some(Self::Three),
            4 => Some(Self::Four),
            _ => None,
        }
    }

    /// The beat number, 1 to 4.
    pub const fn number(self) -> u16 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpm_keeps_hundredths() {
        let bpm = Bpm::from_centi(12_000);
        assert_eq!(bpm.centi(), 12_000);
        assert!((bpm.as_f64() - 120.0).abs() < f64::EPSILON);
        assert_eq!(bpm.to_string(), "120.00");
        assert_eq!(Bpm::from_centi(12_805).to_string(), "128.05");
        assert_eq!(Bpm::from_centi(100_000).to_string(), "1000.00");
    }

    #[test]
    fn beat_in_bar_accepts_one_to_four_only() {
        for n in 1..=4 {
            assert_eq!(BeatInBar::new(n).map(BeatInBar::number), Some(n));
        }
        assert_eq!(BeatInBar::new(0), None);
        assert_eq!(BeatInBar::new(5), None);
        assert_eq!(BeatInBar::new(u16::MAX), None);
    }
}
