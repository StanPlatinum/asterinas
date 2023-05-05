use core::ops::Range;

use align_ext::AlignExt;
use alloc::vec::Vec;
use buddy_system_allocator::FrameAllocator;
use log::{debug, info};
use spin::{Mutex, Once};

use crate::{config::PAGE_SIZE, vm::Paddr};

use super::{frame::VmFrameFlags, MemoryRegions, MemoryRegionsType, VmFrame};

static FRAME_ALLOCATOR: Once<Mutex<FrameAllocator>> = Once::new();

pub fn alloc() -> Option<VmFrame> {
    FRAME_ALLOCATOR
        .get()
        .unwrap()
        .lock()
        .alloc(1)
        .map(|pa| unsafe { VmFrame::new(pa * PAGE_SIZE, VmFrameFlags::NEED_DEALLOC) })
}

pub fn alloc_continuous(frame_count: usize) -> Option<Vec<VmFrame>> {
    FRAME_ALLOCATOR
        .get()
        .unwrap()
        .lock()
        .alloc(frame_count)
        .map(|start| {
            let mut vector = Vec::new();
            unsafe {
                for i in 0..frame_count {
                    vector.push(VmFrame::new(
                        (start + i) * PAGE_SIZE,
                        VmFrameFlags::NEED_DEALLOC,
                    ))
                }
            }
            vector
        })
}

pub(crate) fn alloc_zero() -> Option<VmFrame> {
    let frame = alloc()?;
    frame.zero();
    Some(frame)
}

/// Dealloc a frame.
///
/// # Safety
///
/// User should ensure the index is valid
///
pub(crate) unsafe fn dealloc(index: usize) {
    FRAME_ALLOCATOR.get().unwrap().lock().dealloc(index, 1);
}

pub(crate) fn init(regions: &Vec<MemoryRegions>) {
    let mut allocator = FrameAllocator::<32>::new();
    for region in regions.iter() {
        if region.typ == MemoryRegionsType::Usable {
            assert_eq!(region.base % PAGE_SIZE as u64, 0);
            assert_eq!(region.len % PAGE_SIZE as u64, 0);
            let start = region.base as usize / PAGE_SIZE;
            let end = start + region.len as usize / PAGE_SIZE;
            allocator.add_frame(start, end);
            info!(
                "Found usable region, start:{:x}, end:{:x}",
                region.base,
                region.base + region.len
            );
        }
    }
    FRAME_ALLOCATOR.call_once(|| Mutex::new(allocator));
}
