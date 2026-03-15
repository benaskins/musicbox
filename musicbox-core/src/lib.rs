/// musicbox-core: platform-agnostic generative audio DSP engine.
pub fn hello() -> &'static str {
    "musicbox-core"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_exists() {
        assert_eq!(hello(), "musicbox-core");
    }
}
