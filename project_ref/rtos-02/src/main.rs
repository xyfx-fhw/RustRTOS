#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(
    // 放在 .text.reset_handler 节，链接脚本会把它放到 0x00000000
    ".section .text.reset_handler, \"ax\"",
    ".global reset_body",
    "reset_body:",

    // 0. 检测 HYP 模式（mps3-an536 以 HYP 模式启动），切换到 SVC
    "mrs r0, cpsr",
    "and r0, r0, #0x1f",
    "cmp r0, #0x1a",
    "bne .Lnormal_init",
    "mov r0, #0xd3",
    "msr spsr_cxsf, r0",
    "adr r0, .Lnormal_init",
    "msr elr_hyp, r0",
    "eret",
    ".Lnormal_init:",

    // 1. 设置栈指针
    "ldr sp, =_stack_start",

    // 2. 清零 BSS 段
    "ldr r0, =_sbss",
    "ldr r1, =_ebss",
    "mov r2, #0",
    "1:",
    "cmp r0, r1",
    "bhs 2f",
    "str r2, [r0]",
    "add r0, r0, #4",
    "b 1b",
    "2:",

    // 3. 复制 .data 段从 Flash 到 RAM
    "ldr r0, =_sdata",
    "ldr r1, =_edata",
    "ldr r2, =_sidata",
    "3:",
    "cmp r0, r1",
    "bhs 4f",
    "ldr r3, [r2]",
    "str r3, [r0]",
    "add r0, r0, #4",
    "add r2, r2, #4",
    "b 3b",
    "4:",

    // 4. 跳转到 Rust 入口
    "bl rust_main",

    // 安全保底死循环
    "5:",
    "wfi",
    "b 5b",
);

/// 程序真正的入口。reset handler 完成初始化后跳转到这里。
#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}