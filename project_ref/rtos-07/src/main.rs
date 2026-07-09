#![no_std]
#![no_main]

mod uart;
mod gic;
mod timer;
mod tick;
mod task;
mod scheduler;

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(r#"
    // 向量表
    .section .text.vector_table, "ax"
    .global _vectors
    _vectors:
    b reset_handler
    b undef_handler
    b svc_handler
    b prefetch_handler
    b data_handler
    b hang
    b irq_handler
    b fiq_handler

    // 异常处理函数
    .section .text.handlers, "ax"
    undef_handler:
    push {{r0-r12, lr}}
    bl rust_undef_handler
    pop {{r0-r12, lr}}
    movs pc, lr

    svc_handler:
    push {{r0-r12, lr}}
    bl rust_svc_handler
    pop {{r0-r12, lr}}
    movs pc, lr

    prefetch_handler:
    sub lr, lr, #4
    push {{r0-r12, lr}}
    bl rust_prefetch_handler
    pop {{r0-r12, lr}}
    movs pc, lr

    data_handler:
    sub lr, lr, #8
    push {{r0-r12, lr}}
    bl rust_data_handler
    pop {{r0-r12, lr}}
    movs pc, lr

    hang:
    wfi
    b hang

    irq_handler:
    push {{r0-r12, lr}}
    bl rust_irq_handler
    pop {{r0-r12, lr}}
    subs pc, lr, #4

// ── fiq_handler：SRSDB + CPS + PUSH，在任务 SVC 栈上直接建 16 字帧 ──────────
fiq_handler:
    sub  lr, lr, #4              // ① lr_fiq = 被中断的 PC
    srsdb sp!, #0x13             // ② {lr_fiq, spsr_fiq} → SVC 栈顶，sp_svc -= 8
    cps  #0x13                   // ③ 切到 SVC 模式；r8-r12 现在是任务的
    push {r0-r12, lr}            // ④ 保存 r0-r12 + lr_svc（sp -= 56）
    // 现在 sp_svc 指向完整 16 字帧底部（r0 处）

    ldr  r0, =CURRENT_TASK       // r0 = &CURRENT_TASK
    ldr  r0, [r0]                // r0 = CURRENT_TASK（当前任务 Task 指针）
    str  sp, [r0]                // Task.stack_ptr = sp（保存帧地址到 TCB）

    bl   scheduler_tick          // tick++ + ACK + EOI + 优先级选下一任务 + 更新 CURRENT_TASK

    ldr  r0, =CURRENT_TASK       // 重新读（scheduler_tick 可能已切换）
    ldr  r0, [r0]
    ldr  sp, [r0]                // sp = 下一任务的 stack_ptr

    pop  {r0-r12, lr}            // 恢复 r0-r12 + lr_svc
    rfeia sp!                    // PC=[sp], CPSR=[sp+4], sp+=8，跳入目标任务
    .ltorg                       // 此处刷新 literal pool，保证偏移正确

    // Reset handler
    .section .text.reset_handler, "ax"
    .global reset_handler
    reset_handler:
    // mps3-an536 以 HYP 模式启动，需切换到 SVC 模式才能使用普通向量表
    mrs r0, cpsr
    and r0, r0, #0x1f
    cmp r0, #0x1a          // 0x1a = HYP 模式
    bne .Lnormal_init
    mov r0, #0xd3           // SVC 模式，禁 IRQ/FIQ
    msr spsr_cxsf, r0
    adr r0, .Lnormal_init
    msr elr_hyp, r0
    eret                    // 切换到 SVC 模式，跳到 .Lnormal_init
    .Lnormal_init:
    // 初始化各异常模式的栈指针（共享同一个栈顶，仅用于简单 fault 处理）
    msr cpsr_c, #0xdb
    ldr sp, =_stack_start  // Undefined 模式
    msr cpsr_c, #0xd7
    ldr sp, =_stack_start  // Abort 模式
    msr cpsr_c, #0xd2
    ldr sp, =_stack_start  // IRQ 模式
    msr cpsr_c, #0xd1
    ldr sp, =_stack_start  // FIQ 模式
    msr cpsr_c, #0xd3
    ldr sp, =_stack_start  // 回到 SVC 模式
    ldr r0, =_sbss
    ldr r1, =_ebss
    mov r2, #0
    1:
    cmp r0, r1
    bhs 2f
    str r2, [r0]
    add r0, r0, #4
    b 1b
    2:
    ldr r0, =_sdata
    ldr r1, =_edata
    ldr r2, =_sidata
    3:
    cmp r0, r1
    bhs 4f
    ldr r3, [r2]
    str r3, [r0]
    add r0, r0, #4
    add r2, r2, #4
    b 3b
    4:
    bl rust_main
    5:
    wfi
    b 5b
"#);

static mut TASK_A: task::Task = task::Task { stack_ptr: core::ptr::null_mut() };
static mut TASK_B: task::Task = task::Task { stack_ptr: core::ptr::null_mut() };

fn task_10ms() {   // priority 3，最高
    loop {
        println!("[10ms p3] <<< PREEMPT >>> tick={}", tick::get_ticks());
        scheduler::sleep_ticks(10);
    }
}

fn task_20ms() {   // priority 2，用 delay_ticks 忙等模拟长时间工作
    loop {
        println!("[20ms p2] START tick={}", tick::get_ticks());
        tick::delay_ticks(15);   // 忙等 15 tick，期间不主动让出 CPU
        println!("[20ms p2] END   tick={}", tick::get_ticks());
        scheduler::sleep_ticks(5);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart::uart_init();
    println!("Hello from RTOS!");
    gic::gic_init();
    timer::timer_init();

    scheduler::add_task(task_20ms, 2);
    scheduler::add_task(task_10ms, 3);
    scheduler::start();
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_undef_handler() -> ! {
    println!("FAULT: Undefined Instruction");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_svc_handler() -> ! {
    println!("FAULT: SVC (not implemented)");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_prefetch_handler() -> ! {
    println!("FAULT: Prefetch Abort");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_data_handler() -> ! {
    println!("FAULT: Data Abort");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_irq_handler() {}

#[unsafe(no_mangle)]
pub extern "C" fn rust_fiq_handler() {
    let intid = gic::gic_ack0();
    if intid == 33 {
        timer::timer_clear_interrupt();
        tick::tick_increment();
    }
    gic::gic_eoi0(intid);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("PANIC!");
    loop {}
}