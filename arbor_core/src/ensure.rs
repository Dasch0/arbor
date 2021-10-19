/// Ensure that a condition is true. Used in place of `assert!` when failure should return an
/// error rather than panic
#[inline]
pub fn ensure<E>(test: bool, err: E) -> Result<(), E> {
    match test {
        true => Ok(()),
        false => Err(err),
    }
}
