use core::arch::global_asm;

pub const STACK_SIZE: usize = 512;
pub const MAX_TASKS: usize = 4;

#[repr(C)]
pub struct Task {
    pub stack_ptr: *mut u32,
}

static mut TASK_STACKS: [[u32; STACK_SIZE]; MAX_TASKS] = [[0; STACK_SIZE]; MAX_TASKS];
static mut TASK_COUNT: usize = 0;

pub fn create_task(entry: fn() -> !) -> Task {
    unsafe {
        let id = TASK_COUNT;
        TASK_COUNT += 1;

        let stack = &mut TASK_STACKS[id];
        stack[STACK_SIZE - 1] = entry as u32; // lr = 入口函数地址
        // r0-r12 已为 0（static 零初始化）

        Task {
            stack_ptr: &mut stack[STACK_SIZE - 14] as *mut u32,
        }
    }
}

global_asm!(
    ".global context_switch",
    ".type   context_switch, %function",
    "context_switch:",
    "push {{r0-r12, lr}}",
    "str  sp, [r0]",
    "ldr  sp, [r1]",
    "pop  {{r0-r12, lr}}",
    "bx   lr",

    ".global start_first_task",
    ".type   start_first_task, %function",
    "start_first_task:",
    "ldr  sp, [r0]",
    "pop  {{r0-r12, lr}}",
    "bx   lr",
);

unsafe extern "C" {
    pub fn context_switch(curr: *mut Task, next: *const Task);
    pub fn start_first_task(task: *const Task) -> !;
}