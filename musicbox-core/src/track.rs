/// Unified fade/playback state used by all tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    FadingIn,
    Playing,
    FadingOut,
    Done,
}

/// Common interface implemented by all audio-generating tracks.
pub trait Track: Send {
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
    fn start_fade_out(&mut self);
    fn state(&self) -> State;
    fn is_done(&self) -> bool {
        self.state() == State::Done
    }
}
