#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TaskContext {
    pub rsp: usize,
    pub r15: usize,
    pub r14: usize,
    pub r13: usize,
    pub r12: usize,
    pub rbx: usize,
    pub rbp: usize,
    pub rip: usize,
}

impl TaskContext {
    pub fn zero() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn goto_restore(kstack_ptr: usize) -> Self {
        let mut cx = Self::zero();
        cx.rsp = kstack_ptr;
        cx.rip = crate::task::switch::__restore as *const () as usize;
        cx
    }
}
