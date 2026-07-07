#![no_std]
#![no_main]

mod uart;

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

    @ 异常处理桩
    .section .text.handlers, "ax"
undef_handler:
    b undef_handler
svc_handler:
    b svc_handler
prefetch_handler:
    b prefetch_handler
data_handler:
    b data_handler
hang:
    wfi
    b hang
irq_handler:
    b irq_handler
fiq_handler:
    b fiq_handler

    @ Reset handler（初始化代码）
    .section .text.reset_handler, "ax"
    .global reset_handler
reset_handler:
    @ 0. 检测 HYP 模式（mps3-an536 以 HYP 模式启动），切换到 SVC
    mrs r0, cpsr
    and r0, r0, #0x1f
    cmp r0, #0x1a
    bne .Lnormal_init
    mov r0, #0xd3
    msr spsr_cxsf, r0
    adr r0, .Lnormal_init
    msr elr_hyp, r0
    eret                    @ 切换到 SVC 模式（AArch32 EL1），跳到 .Lnormal_init
.Lnormal_init:
    @ 1. 设置栈指针
    ldr sp, =_stack_start
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
    println!("Board: mps3-an536  CPU: Cortex-R52");
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("PANIC!");
    loop {}
}