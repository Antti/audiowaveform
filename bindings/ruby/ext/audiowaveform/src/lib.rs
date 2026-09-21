use std::panic::{AssertUnwindSafe, catch_unwind};
use std::{
    cell::Cell,
    ffi::{c_int, c_long, c_void},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use magnus::{
    DataTypeFunctions, Error, ExceptionClass, Module, Object, RArray, Ruby, TypedData, function,
    method, rb_sys::protect, value::ReprValue,
};
use waveform_core::{
    ChannelMode, Error as CoreError, Gain, Options, Resolution, Waveform, generate_with_cancel,
};

#[derive(TypedData)]
#[magnus(class = "AudioWaveform::Waveform", free_immediately, size)]
struct RubyWaveform(Waveform);

impl DataTypeFunctions for RubyWaveform {
    fn size(&self) -> usize {
        std::mem::size_of_val(self) + self.0.allocated_bytes()
    }
}

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
struct Interrupts {
    wakeup: AtomicBool,
    // Only the decoding thread reads/writes the Ruby unwind tag.
    state: Cell<c_int>,
}

impl Interrupts {
    fn cancelled(&self) -> bool {
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

fn without_gvl<F, T>(ruby: &Ruby, interrupts: &Interrupts, function: F) -> Result<T, Error>
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

impl RubyWaveform {
    fn sample_rate(&self) -> u32 {
        self.0.sample_rate()
    }

    fn samples_per_pixel(&self) -> u64 {
        self.0.frames_per_point()
    }

    fn channels(&self) -> usize {
        self.0.channels()
    }

    fn storage_bits(&self) -> u8 {
        16
    }

    fn length(&self) -> usize {
        self.0.len()
    }

    fn empty(&self) -> bool {
        self.0.is_empty()
    }

    fn duration(&self) -> f64 {
        self.0.duration()
    }

    fn data(ruby: &Ruby, waveform: &Self, bits: u8) -> Result<RArray, Error> {
        match bits {
            16 => peak_array(ruby, waveform.0.data16().iter().copied()),
            8 => peak_array(ruby, waveform.0.data8().map(i16::from)),
            _ => Err(argument_error(ruby, "bits must be either 8 or 16")),
        }
    }

    fn point(&self, channel: usize, index: usize) -> Option<(i16, i16)> {
        self.0.point(index, channel).map(|[min, max]| (min, max))
    }
}

fn peak_array(ruby: &Ruby, values: impl ExactSizeIterator<Item = i16>) -> Result<RArray, Error> {
    let length = values.len();
    if length > c_long::MAX as usize / std::mem::size_of::<rb_sys::VALUE>() {
        return Err(ruby_error(ruby, "waveform exceeds Ruby array capacity"));
    }
    // Protect array allocation as well as appends: Ruby allocation failures
    // must not jump across Rust owners. Only small immediate integers enter
    // this stack buffer; no Ruby heap objects are hidden in a Rust allocation.
    let mut result = None;
    protect(|| {
        result = Some(ruby.ary_new_capa(length));
        rb_sys::Qnil as rb_sys::VALUE
    })?;
    let array = result.expect("protected array allocation returned");
    let mut buffer = [ruby.qnil().as_value(); 256];
    let mut filled = 0;
    for (index, value) in values.enumerate() {
        buffer[filled] = ruby.into_value(value);
        filled += 1;
        if filled == buffer.len() {
            array.cat(&buffer)?;
            filled = 0;
        }
        if index % 16_384 == 16_383 {
            ruby.thread_check_ints()?;
        }
    }
    array.cat(&buffer[..filled])?;
    Ok(array)
}

fn generate(
    ruby: &Ruby,
    input: String,
    scale_kind: String,
    scale_value: u32,
    split_channels: bool,
    amplitude_kind: String,
    amplitude_value: f64,
) -> Result<RubyWaveform, Error> {
    let resolution = match scale_kind.as_str() {
        "samples_per_pixel" => Resolution::FramesPerPoint(scale_value),
        "pixels_per_second" => Resolution::PointsPerSecond(scale_value),
        "points" => Resolution::Points(scale_value),
        _ => return Err(argument_error(ruby, "unsupported waveform scale")),
    };
    let gain = match amplitude_kind.as_str() {
        "none" => Gain::Fixed(1.0),
        "auto" => Gain::Normalize,
        "fixed" => Gain::Fixed(amplitude_value),
        _ => return Err(argument_error(ruby, "unsupported amplitude scale")),
    };
    let options = Options {
        resolution,
        channels: if split_channels {
            ChannelMode::Split
        } else {
            ChannelMode::Mono
        },
        gain,
    };
    let interrupts = Interrupts::default();
    without_gvl(ruby, &interrupts, || {
        generate_with_cancel(input, options, || interrupts.cancelled())
    })?
    .map(RubyWaveform)
    .map_err(|error| core_error(ruby, error))
}

fn core_error(ruby: &Ruby, error: CoreError) -> Error {
    if matches!(error, CoreError::InvalidOption(_)) {
        argument_error(ruby, error.to_string())
    } else {
        ruby_error(ruby, error.to_string())
    }
}

fn argument_error(ruby: &Ruby, message: impl AsRef<str>) -> Error {
    Error::new(ruby.exception_arg_error(), message.as_ref().to_owned())
}

fn ruby_error(ruby: &Ruby, message: impl AsRef<str>) -> Error {
    let error_class = ruby
        .eval::<ExceptionClass>("AudioWaveform::Error")
        .unwrap_or_else(|_| ruby.exception_standard_error());
    Error::new(error_class, message.as_ref().to_owned())
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("AudioWaveform")?;
    module.define_error("Error", ruby.exception_standard_error())?;

    let native = module.define_module("Native")?;
    native.define_singleton_method("generate", function!(generate, 6))?;

    let waveform = module.define_class("Waveform", ruby.class_object())?;
    waveform.define_method("sample_rate", method!(RubyWaveform::sample_rate, 0))?;
    waveform.define_method(
        "samples_per_pixel",
        method!(RubyWaveform::samples_per_pixel, 0),
    )?;
    waveform.define_method("channels", method!(RubyWaveform::channels, 0))?;
    waveform.define_method("storage_bits", method!(RubyWaveform::storage_bits, 0))?;
    waveform.define_method("length", method!(RubyWaveform::length, 0))?;
    waveform.define_method("empty?", method!(RubyWaveform::empty, 0))?;
    waveform.define_method("duration", method!(RubyWaveform::duration, 0))?;
    waveform.define_private_method("__data", method!(RubyWaveform::data, 1))?;
    waveform.define_private_method("__point", method!(RubyWaveform::point, 2))?;
    Ok(())
}
