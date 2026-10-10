//! User volume scales source mix levels; zero is an exact mute.
pub fn scale(source: i16, level: u8) -> i16 {
    (i32::from(source) * i32::from(level.min(10)) / 10) as i16
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mute_original_and_clamp() {
        for source in [i16::MIN, -16383, 0, 2, 16383, i16::MAX] {
            assert_eq!(scale(source, 0), 0);
            assert_eq!(scale(source, 10), source);
            assert_eq!(scale(source, 255), source);
            assert_eq!(scale(source, 5), source / 2);
        }
    }
}
