//! Kernel-owned storage for the optional 386 interpreter, available after firmware exit.
use core::mem::MaybeUninit;
use linked_list_allocator::LockedHeap;
#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();
const BYTES: usize = 4 * (crate::guest::memory::RAM_BYTES + (2 << 20));
#[repr(align(4096))]
struct Backing([MaybeUninit<u8>; BYTES]);
static mut BACKING: Backing = Backing([MaybeUninit::uninit(); BYTES]);
pub(crate) fn initialize() {
    // SAFETY: kmain calls this once before starting any workers. The static
    // storage is aligned and is accessed exclusively through the allocator.
    unsafe {
        HEAP.lock()
            .init(core::ptr::addr_of_mut!(BACKING.0).cast(), BYTES);
    }
}
