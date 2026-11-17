//! Rust allocations use the process's musl allocator and its native page provider.
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn posix_memalign(out: *mut *mut u8, alignment: usize, size: usize) -> i32;
    fn free(p: *mut u8);
}
struct Musl;

unsafe impl GlobalAlloc for Musl {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= align_of::<usize>() {
            return unsafe { malloc(layout.size()) };
        }
        let mut data = ptr::null_mut();
        if unsafe { posix_memalign(&mut data, layout.align(), layout.size()) } != 0 {
            return ptr::null_mut();
        }
        data
    }
    unsafe fn dealloc(&self, data: *mut u8, _: Layout) {
        unsafe { free(data) };
    }
}
#[global_allocator]
static ALLOCATOR: Musl = Musl;
