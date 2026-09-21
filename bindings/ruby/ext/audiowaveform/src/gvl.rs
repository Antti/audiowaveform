//! Synchronous native work without the GVL, with protected Ruby interrupts.
//! Keep task ownership outside Ruby non-local jumps and catch Rust panics
//! before returning across a C callback boundary.

use std::{
    cell::Cell,
    ffi::{c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use magnus::{Error, Ruby, rb_sys::protect};

use super::ruby_error;

struct NoGvlTask<F, T> {
    function: Option<F>,
    result: Option<T>,
    panicked: bool,
}

unsafe extern "C" fn call_without_gvl<F, T>(data: *mut c_void) -> *mut c_void
where
    F: FnOnce() -> T,
{
    // SAFETY: `data` points to a live `NoGvlTask` for the duration of the
    // synchronous `rb_thread_call_without_gvl` call, and Ruby cannot access it.
    let task = unsafe { &mut *data.cast::<NoGvlTask<F, T>>() };
    let Some(function) = task.function.take() else {
        return ptr::null_mut();
    };
    match catch_unwind(AssertUnwindSafe(function)) {
        Ok(result) => task.result = Some(result),
        Err(_) => task.panicked = true,
    }
    ptr::null_mut()
}

#[derive(Default)]
pub(super) struct Interrupts {
    wakeup: AtomicBool,
    // Only the decoding thread reads/writes the Ruby unwind tag.
    state: Cell<c_int>,
}

impl Interrupts {
    pub(super) fn cancelled(&self) -> bool {
        if self.state.get() != 0 {
            return true;
        }
        if self.wakeup.load(Ordering::Relaxed) && self.wakeup.swap(false, Ordering::Relaxed) {
            let mut state = 0;
            // SAFETY: called synchronously inside the no-GVL callback on the
            // same Ruby thread. `check_interrupts` catches Ruby unwinding and
            // returns no Ruby object; `state` stays alive until it returns.
            unsafe {
                rb_sys::rb_thread_call_with_gvl(
                    Some(check_interrupts),
                    (&mut state as *mut c_int).cast(),
                );
            }
            self.state.set(state);
        }
        self.state.get() != 0
    }
}

unsafe extern "C" fn check_interrupts(data: *mut c_void) -> *mut c_void {
    unsafe extern "C" fn check(_: rb_sys::VALUE) -> rb_sys::VALUE {
        // SAFETY: the surrounding with-GVL callback holds the GVL. This
        // function owns no Rust resources for Ruby's non-local jump to skip.
        unsafe { rb_sys::rb_thread_check_ints() };
        rb_sys::Qnil as rb_sys::VALUE
    }

    // Keep the exception/throw payload in Ruby's own GC-rooted error state.
    // Unlike Magnus's `protect`, raw `rb_protect` does not extract and clear
    // that state. Only its integer tag crosses back into no-GVL Rust code.
    // SAFETY: `data` points to the caller's live integer, and `check` cannot
    // panic. All Ruby non-local jumps are caught before returning without GVL.
    unsafe {
        rb_sys::rb_protect(Some(check), rb_sys::Qnil as rb_sys::VALUE, data.cast());
    }
    ptr::null_mut()
}

// Ruby may invoke this from another thread while decoding is running. An
// unblock is only a request to check interrupts: Thread#wakeup and returning
// signal handlers must not discard the decoder's buffers or progress.
unsafe extern "C" fn wake_generation(data: *mut c_void) {
    // SAFETY: this atomic outlives the synchronous GVL call and its unblock
    // callbacks. Only atomic access occurs here; no Ruby APIs or allocation.
    unsafe { &*data.cast::<AtomicBool>() }.store(true, Ordering::Relaxed);
}

pub(super) fn without_gvl<F, T>(
    ruby: &Ruby,
    interrupts: &Interrupts,
    function: F,
) -> Result<T, Error>
where
    F: FnOnce() -> T,
{
    let mut task = NoGvlTask {
        function: Some(function),
        result: None,
        panicked: false,
    };
    // Ruby can raise while checking interrupts before or after the callback.
    // Keep the task outside `protect` so its closure/result is dropped normally
    // even when Ruby exits the protected call with a non-local jump.
    protect(|| {
        // SAFETY: the callback only accesses the live stack-allocated task,
        // invokes Ruby only via the protected with-GVL interrupt check, and
        // catches Rust panics before they cross the C ABI boundary. rb-sys
        // tracks its allocations for Ruby GC.
        unsafe {
            rb_sys::rb_thread_call_without_gvl(
                Some(call_without_gvl::<F, T>),
                (&mut task as *mut NoGvlTask<F, T>).cast(),
                Some(wake_generation),
                (&interrupts.wakeup as *const AtomicBool).cast_mut().cast(),
            );
            // Decoding has returned and dropped all its resources. Resume the
            // captured interrupt here so the outer `protect` converts it to a
            // Magnus error without jumping across the decoder's Rust owners.
            if interrupts.state.get() != 0 {
                rb_sys::rb_jump_tag(interrupts.state.get());
            }
        }
        rb_sys::Qnil as rb_sys::VALUE
    })?;

    if task.panicked {
        Err(ruby_error(
            ruby,
            "native waveform operation failed unexpectedly",
        ))
    } else {
        task.result
            .ok_or_else(|| ruby_error(ruby, "native waveform operation did not complete"))
    }
}
