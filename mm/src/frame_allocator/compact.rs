use x86_64::instructions::interrupts;

use super::ALLOCATOR;

pub fn compact_now() {
    interrupts::without_interrupts(|| {
        ALLOCATOR.compact();
    });
}
