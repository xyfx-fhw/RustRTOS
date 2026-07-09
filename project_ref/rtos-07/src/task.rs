use core::arch::global_asm;

pub const STACK_SIZE: usize = 512;
pub const MAX_TASKS: usize = 8;

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

/// 构造初始帧：lr = wrapper（任务包装器），r0 = arg（用户函数指针）
pub fn create_task_with_arg(wrapper: usize, arg: usize) -> Task {
    unsafe {
        let id = TASK_COUNT;
        TASK_COUNT += 1;

        let stack = &mut TASK_STACKS[id];
        // 16 字初始帧，布局与 fiq_handler 保存的帧相同：
        //   [STACK_SIZE-16..STACK_SIZE-4]  r0-r12
        //   [STACK_SIZE-3]                 lr_svc
        //   [STACK_SIZE-2]                 resume_pc  ← rfeia 加载 PC 的位置
        //   [STACK_SIZE-1]                 cpsr       ← rfeia 加载 CPSR 的位置
        stack[STACK_SIZE - 16] = arg as u32;     // r0 = 用户函数指针（wrapper 进入时读取）
        // r1-r12 已为 0（static 零初始化）
        stack[STACK_SIZE - 3]  = 0;              // lr_svc（新任务无调用链，rfeia 不会用到它）
        stack[STACK_SIZE - 2]  = wrapper as u32; // resume_pc = task_entry_wrapper 地址
        stack[STACK_SIZE - 1]  = 0x13;           // cpsr = 0x13：SVC 模式，F/I 位 = 0（FIQ/IRQ 使能）

        Task {
            stack_ptr: &mut stack[STACK_SIZE - 16] as *mut u32,
        }
    }
}

global_asm!(r#"
    .global context_switch
    .type   context_switch, %function
context_switch:                  // r0=curr, r1=next
    sub  sp, sp, #64             // 预留 16 字
    stmia sp, {r0-r12}           // [sp+0..sp+48] = r0-r12
    str  lr, [sp, #52]           // [sp+52] = lr_svc（协作式 = return addr）
    str  lr, [sp, #56]           // [sp+56] = resume_pc（同上）
    mrs  r2, cpsr
    str  r2, [sp, #60]           // [sp+60] = cpsr
    str  sp, [r0]                // curr->stack_ptr = sp（r0 仍 = curr）
    ldr  sp, [r1]                // sp = next->stack_ptr
    pop  {r0-r12, lr}            // 恢复 r0-r12 + lr_svc
    rfeia sp!                    // 恢复 PC + CPSR，跳入目标任务
    .ltorg

    .global start_first_task
    .type   start_first_task, %function
start_first_task:
    ldr  sp, [r0]
    pop  {{r0-r12, lr}}
    bx   lr
"#);

unsafe extern "C" {
    pub fn context_switch(curr: *mut Task, next: *const Task);
    pub fn start_first_task(task: *const Task) -> !;
}