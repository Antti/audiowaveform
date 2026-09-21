#![cfg(feature = "wav")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use waveform_core::{Options, Resolution, generate};

// Test-only instrumentation; the production core forbids unsafe code.
struct Allocations;
thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn record() {
    let _ = ENABLED.try_with(|enabled| {
        if enabled.get() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Allocations {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the requested allocation is passed unchanged to System.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the requested allocation is passed unchanged to System.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: this allocator always allocates through System with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        // SAFETY: this pointer came from System; the caller supplies its layout.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocations = Allocations;

fn count<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ENABLED.with(|enabled| enabled.set(false));
        }
    }
    COUNT.with(|count| count.set(0));
    ENABLED.with(|enabled| enabled.set(true));
    let guard = Reset;
    let result = operation();
    drop(guard);
    (result, COUNT.with(Cell::get))
}

#[test]
fn target_count_does_not_add_per_point_allocations_and_data8_allocates_nothing() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generated/uneven.wav");
    generate(&path, Options::default()).unwrap(); // initialize the decoder registry outside measurement
    let mut counts = Vec::new();
    for points in [2, 110, 10000] {
        let (waveform, allocations) = count(|| {
            generate(
                &path,
                Options {
                    resolution: Resolution::Points(points),
                    ..Options::default()
                },
            )
            .unwrap()
        });
        counts.push(allocations);
        let (_, reads) = count(|| waveform.data8().map(i64::from).sum::<i64>());
        assert_eq!(reads, 0);
        assert_eq!(waveform.len(), points as usize);
    }
    assert!(
        counts.windows(2).all(|pair| pair[0] == pair[1]),
        "allocation counts: {counts:?}"
    );
}

#[test]
fn pcm_push_reuses_decode_storage_without_per_block_allocations() {
    use waveform_core::{PcmFormat, PcmStream};
    let mut stream = PcmStream::new(
        PcmFormat::S16Le,
        48000,
        1,
        Options {
            resolution: Resolution::FramesPerPoint(1_000_000),
            ..Options::default()
        },
    )
    .unwrap();
    let block = [0_u8; 8192];
    let (_, allocations) = count(|| {
        for _ in 0..100 {
            stream.push(&block).unwrap();
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(stream.finish().unwrap().source_frames(), 409600);
}
