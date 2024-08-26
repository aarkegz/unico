#[cfg(feature = "alloc")]
use alloc::boxed::Box;
#[cfg(feature = "alloc")]
use core::any::Any;

use unico_context::Resume;
#[cfg(feature = "alloc")]
use unico_context::Transfer;
#[cfg(feature = "alloc")]
use unwinding::panic;

#[cfg(feature = "alloc")]
use crate::Co;

#[cfg(feature = "alloc")]
#[repr(transparent)]
pub(crate) struct HandleDrop<C>(pub C);

// SAFETY: If the actual context is not `Send`, then the coroutine will also not
// be `Send`, thus, preventing sending the context to another thread and unwinds
// there.
#[cfg(feature = "alloc")]
unsafe impl<C> Send for HandleDrop<C> {}

#[cfg(feature = "alloc")]
#[allow(improper_ctypes_definitions)]
pub(crate) extern "C" fn unwind<R: Resume>(
    cx: R::Context,
    _: *mut (),
) -> Transfer<R::Context> {
    panic::begin_panic(Box::new(HandleDrop(cx)));
    unreachable!("Unwind erroneously returned.")
}

#[cfg(feature = "alloc")]
pub(crate) fn resume_unwind<R: Resume>(
    _: &Co<R>,
    payload: Box<dyn Any + Send>,
) -> Box<dyn Any + Send> {
    match payload.downcast::<HandleDrop<R::Context>>() {
        Ok(data) => {
            panic::begin_panic(data);
            unreachable!("Unwind erroneously returned.")
        }
        Err(p) => p,
    }
}

/// The source of some continuation if the current continuation panics.
///
/// The coroutine is built with some implementation of this trait. if the
/// coroutine panics, the panic payload will be handled to it, which then either
/// gives back a continuation to be passed on, or simply aborts the whole
/// control flow, depending on its implementation.
pub trait PanicHook<R: Resume> {
    /// The actual process of handling the panic. See [`PanicHook`] for more
    /// information.
    ///
    /// Every function whose signature matches this method automatically
    /// implements this trait. So no need to create some unit structure if the
    /// actual type is not used.
    #[cfg(feature = "alloc")]
    fn rewind(self, payload: Box<dyn Any + Send>) -> Co<R>;
}

/// Aborts the whole control flow if a panic is caught in the current
/// coroutine/continuation.
///
/// See [`PanicHook`] for more information.
pub struct AbortHook;

impl<R: Resume> PanicHook<R> for AbortHook {
    #[cfg(feature = "alloc")]
    fn rewind(self, _: Box<dyn Any + Send>) -> Co<R> {
        unreachable!("Uncaught panic in the root of a symmetric coroutine. Aborting.")
    }
}

#[cfg(feature = "alloc")]
impl<T, R> PanicHook<R> for T
where
    T: FnOnce(Box<dyn Any + Send>) -> Co<R>,
    R: Resume,
{
    fn rewind(self, payload: Box<dyn Any + Send>) -> Co<R> {
        self(payload)
    }
}