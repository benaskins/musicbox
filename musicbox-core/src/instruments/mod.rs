pub mod bass;
pub mod drums;
pub mod granular;
pub mod oscillator;
pub mod pads;
pub mod pluck;
pub mod stabs;

pub use bass::MonoSynth;
pub use drums::{ClaveVoice, HiHat, Kick, Snare808};
pub use granular::{Grain, GranularEngine};
pub use oscillator::Oscillator;
pub use pads::SynthPad;
pub use pluck::{PluckEngine, PluckVoice};
pub use stabs::DubStab;
