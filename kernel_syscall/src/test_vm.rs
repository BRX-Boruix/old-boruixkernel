use kernel_platform::memory::addr_space::{MemoryArea, MemorySet, PageTableFlags};
use kernel_platform::memory::mapper::Mapper;
use logger::println;
use spin::Mutex;
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::VirtAddr;

#[allow(dead_code)]
static TEST_MEMORY_SET: Mutex<Option<MemorySet>> = Mutex::new(None);

#[allow(dead_code)]
pub fn test_mm1() -> isize {
    use x86_64::structures::paging::PhysFrame;
    use x86_64::PhysAddr as X86PhysAddr;

    println!("TEST: mm1");
    println!("TEST: mm1 step=1 new_bare");

    let mut ms = match MemorySet::new_bare() {
        Ok(m) => m,
        Err(e) => {
            println!("TEST: mm1 FAIL step=1 reason=new_bare {}", e);
            return -1;
        }
    };

    println!("TEST: mm1 step=2 push");
    let start = kernel_platform::memory::addr_space::VirtAddr::new(0x0000_4000_0000);
    let end = kernel_platform::memory::addr_space::VirtAddr::new(start.as_u64() + 4096);
    let mut data = [0u8; 128];
    for b in data.iter_mut() {
        *b = 0x5a;
    }

    let area = MemoryArea::new(
        start,
        end,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
    );

    if let Err(e) = ms.push(area, Some(&data)) {
        println!("TEST: mm1 FAIL step=2 reason=push {:?}", e);
        return -1;
    }

    println!("TEST: mm1 step=3 translate");
    let phys = match ms.translate(start) {
        Some(p) => p,
        None => {
            println!("TEST: mm1 FAIL step=3 reason=translate");
            return -1;
        }
    };

    println!("TEST: mm1 step=4 verify_data");
    let phys_offset = match kernel_platform::memory::addr_space::PHYS_OFFSET.get() {
        Some(v) => *v,
        None => {
            println!("TEST: mm1 FAIL step=4 reason=phys_offset_missing");
            return -1;
        }
    };

    let ptr = (phys.as_u64() + phys_offset) as *const u8;
    unsafe {
        for i in 0..data.len() {
            if *ptr.add(i) != 0x5a {
                println!("TEST: mm1 FAIL step=4 reason=data_mismatch at={}", i);
                return -1;
            }
        }
        for i in data.len()..4096 {
            if *ptr.add(i) != 0 {
                println!("TEST: mm1 FAIL step=4 reason=zeroing_mismatch at={}", i);
                return -1;
            }
        }
    }

    println!("TEST: mm1 step=5 unmap");
    unsafe {
        let mut mapper = match ms.mapper() {
            Ok(m) => m,
            Err(_) => {
                println!("TEST: mm1 FAIL step=5 reason=mapper");
                return -1;
            }
        };
        match mapper.unmap_page(start) {
            Ok(p) => {
                let frame = PhysFrame::containing_address(X86PhysAddr::new(p.as_u64()));
                kernel_platform::memory::pmm::frame_allocator::deallocate_frame(frame);
            }
            Err(e) => {
                println!("TEST: mm1 FAIL step=5 reason=unmap {:?}", e);
                return -1;
            }
        }
    }

    println!("TEST: mm1 step=6 translate_after_unmap");
    if ms.translate(start).is_some() {
        println!("TEST: mm1 FAIL step=6 reason=translate_not_none");
        return -1;
    }

    println!("TEST: mm1 PASS");
    0
}

#[allow(dead_code)]
fn page_fault_handler(addr: VirtAddr, error_code: PageFaultErrorCode) -> Result<(), ()> {
    println!(
        "Simulated Page Fault at {:?} with error {:?}",
        addr, error_code
    );
    Ok(())
}
