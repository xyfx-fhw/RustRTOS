#![no_std]
#![no_main]

mod uart;

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(
    ".section .text.reset_handler, \"ax\"",
    ".global reset_handler",
    ".type reset_handler, %function",
    "reset_handler:",
    "ldr sp, =_stack_start",
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
    "bl rust_main",
    "5:",
    "wfi",
    "b 5b",
);

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