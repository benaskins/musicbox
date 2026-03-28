/// A musical note duration, expressed relative to a quarter note.
///
/// All values are defined as exact rational multiples of a quarter note so that
/// period calculations are exact in f64 arithmetic. Triplets are "three notes in
/// the space of two" of the plain value: a quarter triplet is 2/3 of a quarter
/// note, an eighth triplet is 1/3, and so on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoteValue {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    // Triplets
    QuarterTriplet,        // 2/3 of a quarter note
    EighthTriplet,         // 1/3 of a quarter note
    SixteenthTriplet,      // 1/6 of a quarter note
    ThirtySecondTriplet,   // 1/12 of a quarter note
}

impl NoteValue {
    /// Duration expressed in quarter notes.
    pub fn in_quarter_notes(self) -> f64 {
        match self {
            Self::Whole => 4.0,
            Self::Half => 2.0,
            Self::Quarter => 1.0,
            Self::Eighth => 0.5,
            Self::Sixteenth => 0.25,
            Self::ThirtySecond => 0.125,
            Self::QuarterTriplet => 2.0 / 3.0,
            Self::EighthTriplet => 1.0 / 3.0,
            Self::SixteenthTriplet => 1.0 / 6.0,
            Self::ThirtySecondTriplet => 1.0 / 12.0,
        }
    }
}

/// A musical time signature.
///
/// `beats_per_bar` is the numerator (e.g. 4 in 4/4, 6 in 6/8).
/// `beat_value` is the denominator — the note that receives one beat (e.g. 4 for
/// quarter, 8 for eighth). BPM is always expressed in quarter notes per minute
/// regardless of beat_value; beat_value only affects bar length.
#[derive(Debug, Clone, Copy)]
pub struct TimeSignature {
    pub beats_per_bar: u8,
    pub beat_value: u8,
}

impl TimeSignature {
    pub fn new(beats_per_bar: u8, beat_value: u8) -> Self {
        Self { beats_per_bar, beat_value }
    }

    pub fn four_four() -> Self { Self::new(4, 4) }
    pub fn three_four() -> Self { Self::new(3, 4) }
    pub fn six_eight() -> Self { Self::new(6, 8) }
    pub fn seven_eight() -> Self { Self::new(7, 8) }

    /// Bar duration expressed in quarter notes.
    ///
    /// Examples:
    /// - 4/4 → 4.0  (four quarter notes)
    /// - 3/4 → 3.0  (three quarter notes)
    /// - 6/8 → 3.0  (six eighth notes = three quarter notes)
    /// - 7/8 → 3.5  (seven eighth notes = 3.5 quarter notes)
    pub fn bar_in_quarter_notes(self) -> f64 {
        self.beats_per_bar as f64 * (4.0 / self.beat_value as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_value_quarter_notes() {
        assert_eq!(NoteValue::Quarter.in_quarter_notes(), 1.0);
        assert_eq!(NoteValue::Eighth.in_quarter_notes(), 0.5);
        assert_eq!(NoteValue::Sixteenth.in_quarter_notes(), 0.25);
        assert_eq!(NoteValue::ThirtySecond.in_quarter_notes(), 0.125);
        assert_eq!(NoteValue::Half.in_quarter_notes(), 2.0);
        assert_eq!(NoteValue::Whole.in_quarter_notes(), 4.0);
    }

    #[test]
    fn triplet_note_values() {
        // Three triplets must fill the same space as two plain notes.
        assert!((3.0 * NoteValue::QuarterTriplet.in_quarter_notes() - 2.0).abs() < 1e-12);
        assert!((3.0 * NoteValue::EighthTriplet.in_quarter_notes() - 1.0).abs() < 1e-12);
        assert!((3.0 * NoteValue::SixteenthTriplet.in_quarter_notes() - 0.5).abs() < 1e-12);
        assert!((3.0 * NoteValue::ThirtySecondTriplet.in_quarter_notes() - 0.25).abs() < 1e-12);
    }

    #[test]
    fn time_signature_bar_lengths() {
        assert_eq!(TimeSignature::four_four().bar_in_quarter_notes(), 4.0);
        assert_eq!(TimeSignature::three_four().bar_in_quarter_notes(), 3.0);
        assert_eq!(TimeSignature::six_eight().bar_in_quarter_notes(), 3.0);
        assert_eq!(TimeSignature::seven_eight().bar_in_quarter_notes(), 3.5);
    }
}
