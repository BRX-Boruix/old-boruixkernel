use core::mem::MaybeUninit;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::PrivilegeLevel;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub const KERNEL_CODE_SELECTOR_INDEX: u16 = 1;
pub const KERNEL_DATA_SELECTOR_INDEX: u16 = 2;
pub const USER_DATA_SELECTOR_INDEX: u16 = 3;
pub const USER_CODE_SELECTOR_INDEX: u16 = 4;

const MAX_CPUS: usize = 64;
const DOUBLE_FAULT_STACK_SIZE: usize = 4096;

#[repr(align(16))]
#[derive(Copy, Clone)]
struct Stack([u8; DOUBLE_FAULT_STACK_SIZE]);

struct CpuGdt {
    gdt: GlobalDescriptorTable,
    tss: TaskStateSegment,
    selectors: Selectors,
}

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

static mut CPU_GDT: [MaybeUninit<CpuGdt>; MAX_CPUS] = [const { MaybeUninit::uninit() }; MAX_CPUS];
static mut DF_STACKS: [Stack; MAX_CPUS] = [Stack([0; DOUBLE_FAULT_STACK_SIZE]); MAX_CPUS];

pub fn init() {
    init_cpu(0);
}

pub fn init_cpu(cpu_id: usize) {
    let cpu = cpu_id % MAX_CPUS;
    unsafe {
        let cpu_gdt_ptr = CPU_GDT[cpu].as_mut_ptr();

        let mut tss = TaskStateSegment::new();
        let stack_start = VirtAddr::from_ptr(&DF_STACKS[cpu].0 as *const _);
        let stack_end = stack_start + DOUBLE_FAULT_STACK_SIZE;
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;

        core::ptr::addr_of_mut!((*cpu_gdt_ptr).tss).write(tss);
        let tss_ref: &'static TaskStateSegment = &*core::ptr::addr_of!((*cpu_gdt_ptr).tss);

        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        let _user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let _user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(tss_ref));

        let selectors = Selectors {
            code_selector,
            data_selector,
            tss_selector,
        };

        core::ptr::addr_of_mut!((*cpu_gdt_ptr).gdt).write(gdt);
        core::ptr::addr_of_mut!((*cpu_gdt_ptr).selectors).write(selectors);

        (*cpu_gdt_ptr).gdt.load();
        CS::set_reg((*cpu_gdt_ptr).selectors.code_selector);
        load_tss((*cpu_gdt_ptr).selectors.tss_selector);
        SS::set_reg((*cpu_gdt_ptr).selectors.data_selector);
        DS::set_reg((*cpu_gdt_ptr).selectors.data_selector);
        ES::set_reg((*cpu_gdt_ptr).selectors.data_selector);
        FS::set_reg((*cpu_gdt_ptr).selectors.data_selector);
        GS::set_reg((*cpu_gdt_ptr).selectors.data_selector);
    }
}

pub fn set_kernel_stack(stack: VirtAddr) {
    let cpu = crate::syscall::current_cpu_id_raw() % MAX_CPUS;
    unsafe {
        let cpu_gdt = &mut *CPU_GDT[cpu].as_mut_ptr();
        cpu_gdt.tss.privilege_stack_table[0] = stack;
    }
}

pub fn get_user_code_selector() -> SegmentSelector {
    SegmentSelector::new(USER_CODE_SELECTOR_INDEX, PrivilegeLevel::Ring3)
}

pub fn get_user_data_selector() -> SegmentSelector {
    SegmentSelector::new(USER_DATA_SELECTOR_INDEX, PrivilegeLevel::Ring3)
}
