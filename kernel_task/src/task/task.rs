use super::context::TaskContext;
use crate::error::TaskError;
use kernel_platform::hal::arch;
use kernel_platform::memory::addr_space::MemorySet;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use arch::TrapFrame;
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::registers::rflags::RFlags;
use crate::fd::FdTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
    Exited,
    Waiting,
}

#[repr(align(16))]
#[allow(dead_code)]
pub struct FxSaveArea(pub [u8; 512]);

impl FxSaveArea {
    pub const fn new() -> Self {
        Self([0; 512])
    }
}

pub struct TaskControlBlock {
    pub pid: usize,
    pub kernel_stack: usize,
    pub kernel_stack_bottom: usize,
    pub memory_set: UnsafeCell<MemorySet>,
    pub task_cx: UnsafeCell<TaskContext>,
    pub fxsave_area: UnsafeCell<FxSaveArea>,
    pub task_status: Mutex<TaskStatus>,
    pub in_run_queue: AtomicBool,
    pub exit_code: Mutex<i32>,
    #[allow(dead_code)]
    pub parent: Mutex<Option<Weak<TaskControlBlock>>>,
    pub children: Mutex<Vec<Arc<TaskControlBlock>>>,
    pub priority: usize,
    pub stride: usize,
    pub pass: Mutex<usize>,
    pub exec_arg: AtomicUsize,
    pub heap_start: Mutex<usize>,
    pub heap_end: Mutex<usize>,
    pub heap_max: Mutex<usize>,
    pub fds: Mutex<FdTable>,
}

unsafe impl Sync for TaskControlBlock {}
unsafe impl Send for TaskControlBlock {}

impl TaskControlBlock {
    pub fn new(elf_data: &[u8], pid: usize) -> Result<Self, TaskError> {
        // Load ELF
        let (memory_set, entry_point, user_stack_top, heap_start, heap_end) =
            MemorySet::from_elf(elf_data).map_err(TaskError::Memory)?;

        // Allocate kernel stack (16KB)
        let kernel_stack_size = 4096 * 4;
        let layout = Layout::from_size_align(kernel_stack_size, 4096)
            .map_err(|_| TaskError::KernelStackLayout)?;
        let kernel_stack_bottom = unsafe { alloc::alloc::alloc(layout) } as usize;
        if kernel_stack_bottom == 0 {
            return Err(TaskError::KernelStackAlloc);
        }
        let kernel_stack_top = kernel_stack_bottom + kernel_stack_size;

        // Construct TrapFrame on kernel stack
        let trap_frame_ptr =
            (kernel_stack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;

        let trap_frame = TrapFrame {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbp: 0,
            rbx: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rsi: 0,
            rdi: 0,
            rdx: 0,
            rcx: 0,
            rax: 0,
            rip: entry_point as usize,
            cs: (arch::gdt::get_user_code_selector().0 | 3) as usize,
            rflags: (RFlags::INTERRUPT_FLAG.bits() | 0x2) as usize, // Enable interrupts
            rsp: user_stack_top as usize,
            ss: (arch::gdt::get_user_data_selector().0 | 3) as usize,
        };

        unsafe {
            // SAFETY: trap_frame_ptr points to the reserved TrapFrame slot on the kernel stack.
            *trap_frame_ptr = trap_frame;
        }

        // Construct TaskContext
        let mut task_cx = TaskContext::zero();
        task_cx.rsp = (trap_frame_ptr as usize)
            .checked_sub(core::mem::size_of::<usize>())
            .ok_or(TaskError::InvalidTrapFrame)?;
        task_cx.rip = super::switch::__restore as *const () as usize;

        let priority = 16;
        let stride = 100000 / priority;

        Ok(Self {
            pid,
            kernel_stack: kernel_stack_top,
            kernel_stack_bottom,
            memory_set: UnsafeCell::new(memory_set),
            task_cx: UnsafeCell::new(task_cx),
            fxsave_area: UnsafeCell::new(FxSaveArea::new()),
            task_status: Mutex::new(TaskStatus::Ready),
            in_run_queue: AtomicBool::new(false),
            exit_code: Mutex::new(0),
            parent: Mutex::new(None),
            children: Mutex::new(Vec::new()),
            priority,
            stride,
            pass: Mutex::new(0),
            exec_arg: AtomicUsize::new(0),
            heap_start: Mutex::new(heap_start as usize),
            heap_end: Mutex::new(heap_end as usize),
            heap_max: Mutex::new((user_stack_top - 0x20000) as usize),
            fds: Mutex::new(FdTable::new()),
        })
    }

    #[allow(dead_code)]
    pub fn get_task_cx_ptr(&self) -> *mut TaskContext {
        self.task_cx.get()
    }

    #[allow(dead_code)]
    pub fn get_memory_set(&self) -> &mut MemorySet {
        unsafe { &mut *self.memory_set.get() }
    }

    pub fn fork(self: &Arc<Self>, new_pid: usize) -> Result<Arc<Self>, TaskError> {
        // 1. Fork MemorySet
        let memory_set = unsafe { (*self.memory_set.get()).fork().map_err(TaskError::Memory)? };

        // 2. Alloc Kernel Stack
        let kernel_stack_size = 4096 * 4;
        let layout = Layout::from_size_align(kernel_stack_size, 4096)
            .map_err(|_| TaskError::KernelStackLayout)?;
        let kernel_stack_bottom = unsafe { alloc::alloc::alloc(layout) } as usize;
        if kernel_stack_bottom == 0 {
            return Err(TaskError::KernelStackAlloc);
        }
        let kernel_stack_top = kernel_stack_bottom + kernel_stack_size;

        // 3. Copy TrapFrame
        let trap_frame_ptr =
            (kernel_stack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
        let parent_trap_frame = self.trap_frame();

        unsafe {
            // SAFETY: trap_frame_ptr points to the child's kernel stack TrapFrame slot.
            *trap_frame_ptr = *parent_trap_frame;
            (*trap_frame_ptr).rax = 0; // Child return value
        }

        // 4. Construct TaskContext
        let mut task_cx = TaskContext::zero();
        task_cx.rsp = (trap_frame_ptr as usize)
            .checked_sub(core::mem::size_of::<usize>())
            .ok_or(TaskError::InvalidTrapFrame)?;
        task_cx.rip = super::switch::__restore as *const () as usize;

        // 5. Create TCB
        let priority = self.priority;
        let stride = 100000 / priority;
        let exec_arg = self.exec_arg.load(Ordering::Relaxed);
        let heap_start = *self.heap_start.lock();
        let heap_end = *self.heap_end.lock();
        let heap_max = *self.heap_max.lock();

        let child = Arc::new(Self {
            pid: new_pid,
            kernel_stack: kernel_stack_top,
            kernel_stack_bottom,
            memory_set: UnsafeCell::new(memory_set),
            task_cx: UnsafeCell::new(task_cx),
            fxsave_area: UnsafeCell::new(FxSaveArea::new()),
            task_status: Mutex::new(TaskStatus::Ready),
            in_run_queue: AtomicBool::new(false),
            exit_code: Mutex::new(0),
            parent: Mutex::new(Some(Arc::downgrade(self))),
            children: Mutex::new(Vec::new()),
            priority,
            stride,
            pass: Mutex::new(*self.pass.lock()),
            exec_arg: AtomicUsize::new(exec_arg),
            heap_start: Mutex::new(heap_start),
            heap_end: Mutex::new(heap_end),
            heap_max: Mutex::new(heap_max),
            fds: Mutex::new(self.fds.lock().fork_clone()),
        });

        // Add to parent's children
        self.children.lock().push(child.clone());

        Ok(child)
    }

    pub fn exec(&self, elf_data: &[u8]) -> Result<(), TaskError> {
        let (memory_set, entry_point, user_stack_top, heap_start, heap_end) =
            MemorySet::from_elf(elf_data).map_err(TaskError::Memory)?;

        unsafe {
            // Activate new page table first
            memory_set.activate();
            crate::smp::set_current_cr3(memory_set.token());
            // Replace memory set (old one dropped)
            *self.memory_set.get() = memory_set;
        }

        // Close CLOEXEC fds on exec
        self.fds.lock().close_on_exec();

        *self.heap_start.lock() = heap_start as usize;
        *self.heap_end.lock() = heap_end as usize;
        *self.heap_max.lock() = (user_stack_top - 0x20000) as usize;

        let trap_frame = self.trap_frame_mut();
        trap_frame.rip = entry_point as usize;
        trap_frame.rsp = user_stack_top as usize;
        // Reset registers
        trap_frame.rax = 0;
        trap_frame.rbx = 0;
        trap_frame.rcx = 0;
        trap_frame.rdx = 0;
        trap_frame.rsi = 0;
        trap_frame.rdi = 0;
        trap_frame.rbp = 0;
        trap_frame.r8 = 0;
        trap_frame.r9 = 0;
        trap_frame.r10 = 0;
        trap_frame.r11 = 0;
        trap_frame.r12 = 0;
        trap_frame.r13 = 0;
        trap_frame.r14 = 0;
        trap_frame.r15 = 0;
        Ok(())
    }

    pub fn trap_frame(&self) -> &TrapFrame {
        let ptr = (self.kernel_stack - core::mem::size_of::<TrapFrame>()) as *const TrapFrame;
        // SAFETY: kernel_stack always reserves space for TrapFrame at its top.
        unsafe { &*ptr }
    }

    pub fn trap_frame_mut(&self) -> &mut TrapFrame {
        let ptr = (self.kernel_stack - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
        // SAFETY: kernel_stack always reserves space for TrapFrame at its top.
        unsafe { &mut *ptr }
    }

    pub fn set_exec_arg(&self, val: usize) {
        self.exec_arg.store(val, Ordering::Relaxed);
    }

    pub fn get_exec_arg(&self) -> usize {
        self.exec_arg.load(Ordering::Relaxed)
    }
}

impl Drop for TaskControlBlock {
    fn drop(&mut self) {
        let kernel_stack_size = 4096 * 4;
        let layout = match Layout::from_size_align(kernel_stack_size, 4096) {
            Ok(l) => l,
            Err(_) => return,
        };
        if self.kernel_stack_bottom != 0 {
            unsafe {
                alloc::alloc::dealloc(self.kernel_stack_bottom as *mut u8, layout);
            }
        }
    }
}
