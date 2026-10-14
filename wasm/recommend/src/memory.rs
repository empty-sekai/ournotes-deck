//! The global allocator of the Worker build: dlmalloc over linear memory that grows by at least an eighth of its
//! current size at a time.
//!
//! The default wasm32 system layer grows linear memory by exactly what each allocation lacks, so a search that
//! reaches a large heap grows memory in many small steps, and a host's cost of one `memory.grow` can rise with the
//! size of the memory. Growing in proportion to the current size keeps the number of grows logarithmic in the peak
//! heap, with at most an eighth of the memory reserved ahead of use.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::wasm32;
use core::cell::UnsafeCell;
use core::ptr;

const PAGE: usize = 64 * 1024;
/// The fewest pages one grow adds.
const MIN_GROW_PAGES: usize = 16;

/// The system layer of [`Heap`]: fresh zeroed pages from `memory.grow`, never returned.
struct ProportionalGrowth;

// SAFETY: every segment returned is fresh memory owned by the allocator alone, and none is ever released.
unsafe impl dlmalloc::Allocator for ProportionalGrowth {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        let needed = size.div_ceil(PAGE);
        let ahead = (wasm32::memory_size(0) / 8).max(MIN_GROW_PAGES).max(needed);
        // the proportional step first; near the memory limit, only what the allocation lacks
        for pages in [ahead, needed] {
            let previous = wasm32::memory_grow(0, pages);
            if previous != usize::MAX {
                let base = previous * PAGE;
                let mut bytes = pages * PAGE;
                // a segment ending at the top of the 32-bit address space would end at address 0
                if base.wrapping_add(bytes) == 0 {
                    bytes -= 16;
                }
                return (base as *mut u8, bytes, 0);
            }
        }
        (ptr::null_mut(), 0, 0)
    }

    fn remap(&self, _ptr: *mut u8, _old_size: usize, _new_size: usize, _can_move: bool) -> *mut u8 {
        ptr::null_mut()
    }

    fn free_part(&self, _ptr: *mut u8, _old_size: usize, _new_size: usize) -> bool {
        false
    }

    fn free(&self, _ptr: *mut u8, _size: usize) -> bool {
        false
    }

    fn can_release_part(&self, _flags: u32) -> bool {
        false
    }

    fn allocates_zeros(&self) -> bool {
        true
    }

    fn page_size(&self) -> usize {
        PAGE
    }
}

/// dlmalloc over [`ProportionalGrowth`].
pub struct Heap(UnsafeCell<dlmalloc::Dlmalloc<ProportionalGrowth>>);

// SAFETY: this module is built only for wasm32 without the atomics feature, which runs on one thread.
unsafe impl Sync for Heap {}

impl Heap {
    pub const fn new() -> Self {
        Self(UnsafeCell::new(dlmalloc::Dlmalloc::new_with_allocator(ProportionalGrowth)))
    }
}

// SAFETY: dlmalloc's contract matches `GlobalAlloc`'s, and the single thread never enters it twice at once.
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { (*self.0.get()).malloc(layout.size(), layout.align()) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { (*self.0.get()).calloc(layout.size(), layout.align()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (*self.0.get()).free(ptr, layout.size(), layout.align()) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { (*self.0.get()).realloc(ptr, layout.size(), layout.align(), new_size) }
    }
}
