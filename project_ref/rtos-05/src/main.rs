#![no_std]
#![no_main]

mod uart;
mod gic;
mod timer;
mod tick;

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(r#"
    @ 向量表
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

    @ 异常处理函数
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

fiq_handler:
    push {{r0-r12, lr}}
    bl rust_fiq_handler
    pop {{r0-r12, lr}}
    subs pc, lr, #4

    @ Reset handler
    .section .text.reset_handler, "ax"
    .global reset_handler
reset_handler:
    @ mps3-an536 以 HYP 模式启动，需切换到 SVC 模式才能使用普通向量表
    mrs r0, cpsr
    and r0, r0, #0x1f
    cmp r0, #0x1a          @ 0x1a = HYP 模式
    bne .Lnormal_init
    mov r0, #0xd3           @ SVC 模式，禁 IRQ/FIQ
    msr spsr_cxsf, r0
    adr r0, .Lnormal_init
    msr elr_hyp, r0
    eret                    @ 切换到 SVC 模式，跳到 .Lnormal_init
.Lnormal_init:
    @ 初始化各异常模式的栈指针（共享同一个栈顶，仅用于简单 fault 处理）
    msr cpsr_c, #0xdb
    ldr sp, =_stack_start  @ Undefined 模式
    msr cpsr_c, #0xd7
    ldr sp, =_stack_start  @ Abort 模式
    msr cpsr_c, #0xd2
    ldr sp, =_stack_start  @ IRQ 模式
    msr cpsr_c, #0xd1
    ldr sp, =_stack_start  @ FIQ 模式
    msr cpsr_c, #0xd3
    ldr sp, =_stack_start  @ 回到 SVC 模式
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

    @ 安全保底死循环
5:
    wfi
    b 5b
"#);

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart::uart_init();
    println!("Hello from RTOS!");
    gic::gic_init();
    timer::timer_init();
    println!("Timer started, 100ms tick.");
    gic::cpu_enable_interrupts();  // 最后才开 IRQ + FIQ

    let mut last = 0u32;
    loop {
        let t = tick::get_ticks();
        if t.wrapping_sub(last) >= 10 {
            last = t;
            println!("Tick: {}ms", t * tick::TICK_PERIOD_MS);
        }
    }
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