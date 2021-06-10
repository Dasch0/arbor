//! Adapted from https://github.com/matklad/once_cell/blob/500dc2a530b690ac92a75f8969b7236fcbd0d5e7/src/race.rs
use core::{
    marker::PhantomData,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use std::boxed::Box;

/// A thread-safe cell which can be written to only once.
#[derive(Debug)]
pub struct OnceBox<T> {
    inner: AtomicPtr<T>,
    ghost: PhantomData<Option<Box<T>>>,
}

impl<T> Default for OnceBox<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for OnceBox<T> {
    fn drop(&mut self) {
        let ptr = *self.inner.get_mut();
        if !ptr.is_null() {
            drop(unsafe { Box::from_raw(ptr) })
        }
    }
}

impl<T> OnceBox<T> {
    /// Creates a new empty cell.
    pub const fn new() -> OnceBox<T> {
        OnceBox {
            inner: AtomicPtr::new(ptr::null_mut()),
            ghost: PhantomData,
        }
    }

    /// Gets a reference to the underlying value.
    pub fn get(&self) -> Option<&T> {
        let ptr = self.inner.load(Ordering::Acquire);
        if ptr.is_null() {
            return None;
        }
        Some(unsafe { &*ptr })
    }

    /// Sets the contents of this cell to `value`.
    ///
    /// Returns `Ok(())` if the cell was empty and `Err(value)` if it was
    /// full.
    pub fn set(&self, value: Box<T>) -> Result<(), Box<T>> {
        let ptr = Box::into_raw(value);
        let exchange =
            self.inner
                .compare_exchange(ptr::null_mut(), ptr, Ordering::AcqRel, Ordering::Acquire);
        if let Err(_) = exchange {
            let value = unsafe { Box::from_raw(ptr) };
            return Err(value);
        }
        Ok(())
    }

    /// Gets the contents of the cell, initializing it with `f` if the cell was
    /// empty.
    ///
    /// If several threads concurrently run `get_or_init`, more than one `f` can
    /// be called. However, all threads will return the same value, produced by
    /// some `f`.
    pub fn get_or_init<F>(&self, f: F) -> &T
    where
        F: FnOnce() -> Box<T>,
    {
        enum Void {}
        match self.get_or_try_init(|| Ok::<Box<T>, Void>(f())) {
            Ok(val) => val,
            Err(void) => match void {},
        }
    }

    /// Gets the contents of the cell, initializing it with `f` if
    /// the cell was empty. If the cell was empty and `f` failed, an
    /// error is returned.
    ///
    /// If several threads concurrently run `get_or_init`, more than one `f` can
    /// be called. However, all threads will return the same value, produced by
    /// some `f`.
    pub fn get_or_try_init<F, E>(&self, f: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<Box<T>, E>,
    {
        let mut ptr = self.inner.load(Ordering::Acquire);

        if ptr.is_null() {
            let val = f()?;
            ptr = Box::into_raw(val);
            let exchange = self.inner.compare_exchange(
                ptr::null_mut(),
                ptr,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            if let Err(old) = exchange {
                drop(unsafe { Box::from_raw(ptr) });
                ptr = old;
            }
        };
        Ok(unsafe { &*ptr })
    }
}

unsafe impl<T: Sync + Send> Sync for OnceBox<T> {}

/// ```compile_fail
/// struct S(*mut ());
/// unsafe impl Sync for S {}
///
/// fn share<T: Sync>(_: &T) {}
/// share(&once_cell::race::OnceBox::<S>::new());
/// ```
fn _dummy() {}
