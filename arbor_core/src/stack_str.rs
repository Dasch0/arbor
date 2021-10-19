#![allow(dead_code)]

//! Stack allocated string that transparently dereferences into a mutable string slice
//! Additionally supports nanoserde via the [StackStrProxy]
use std::{
    fmt,
    ops::{Deref, DerefMut},
    str,
};

/// Result type for StackStr
type Result<T> = std::result::Result<T, Error>;

/// Errors for StackStr
#[derive(Debug)]
pub enum Error {
    RejectPush,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::RejectPush => {
                write!(f, "push rejected, str input too large to fit into StackStr")
            }
        }
    }
}

/// Constant string length, if used as the [StackStr] size the `mem::size_of` will equal a `String`
pub const SMALL: u8 = 23;
/// Constant string length, if used as the [StackStr] size the data will fit in a single cache line
pub const CACHE: u8 = 63;
/// Constant string length, This is the maximum allowed stack string size. This allows len to be
/// constrained to a single byte
pub const MAX: usize = 255;

/// Stack allocated string of varying size.
///
/// Transparently referenced as a str, or dereferenced as a string slice.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StackStr<const N: usize> {
    data: [u8; N],
    len: u8,
}

impl<const N: usize> Default for StackStr<N> {
    fn default() -> Self {
        Self {
            // invariant, 0's are valid utf-8 NUL values
            data: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> StackStr<N> {
    /// Create an empty [StackStr]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new StackStr from existing data. This fails if the data will not fit, and returns
    /// an error.
    #[inline]
    pub fn from_str(data: &str) -> Result<Self> {
        let mut new = Self::new();
        new.push(data)?;
        Ok(new)
    }
    /// Push new data to the end of the [StackStr]. This fails if the data will not fit and returns
    /// an error.
    pub fn push(&mut self, data: &str) -> Result<()> {
        let raw_bytes = data.as_bytes();
        let space = N - self.len as usize;
        if space < raw_bytes.len() {
            return Err(Error::RejectPush);
        }

        let new_len = self.len + raw_bytes.len() as u8;
        let slice = &mut self.data[self.len as usize..new_len as usize];
        slice.copy_from_slice(raw_bytes);
        self.len = new_len;
        Ok(())
    }
}

impl<const N: usize> Deref for StackStr<N> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        // invariant, access to data is ONLY provided via new, or via mutable str slices, such that
        // raw data is only interactable through str implementations, and does not need to be
        // checked on deref
        unsafe { str::from_utf8_unchecked(&self.data[0..self.len as usize]) }
    }
}

impl<const N: usize> DerefMut for StackStr<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // invariant, access to data is ONLY provided via new or via mutable str slices, such that
        // raw data is only interactable through str implementations, and does not need to be
        // checked on deref
        unsafe { str::from_utf8_unchecked_mut(&mut self.data[0..self.len as usize]) }
    }
}
