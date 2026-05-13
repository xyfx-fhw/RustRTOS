---
title: "串口输出与调试宏"
description: "实现 UART 驱动和 print!/println! 宏，让裸机程序能够输出调试信息"
difficulty: intermediate
estimatedTime: 50
keywords: ["UART", "串口", "print!", "println!", "PL011", "CMSDK", "寄存器", "调试输出"]
---

# 本章目标

- 理解 UART 串口通信的基本原理
- 了解 mps3-an536 上 UART 的寄存器结构
- 实现一个最小的 UART 驱动，能发送单个字符
- 实现 `print!` / `println!` 宏，支持格式化输出
- 在 QEMU 上看到程序输出的文字

## 前置知识

### 已完成的章节

`02-minimal-boot` 章节全部完成，`cargo build` 编译无误，QEMU 能正常启动程序。

### 知道什么是指针

理解指针是一个内存地址，向某个地址写入数据就是向该地址对应的硬件寄存器发命令。

# UART 是什么

UART（Universal Asynchronous Receiver/Transmitter，通用异步收发器）是嵌入式开发中最基础的通信接口。在没有屏幕的开发板上，程序的调试信息几乎都通过 UART 发出来，再由连接到开发板的电脑读取显示。

类比：UART 就像古代的电报机——你把想说的话一个字一个字地发出去，对面收到后打印出来。不需要握手，不需要协商，最基础的串行通信。

在 QEMU 中，模拟的 UART 输出直接显示在你的终端里，不需要真实的串口线。这正是 QEMU 调试的便利之处。

# mps3-an536 的 UART

mps3-an536 使用 ARM CMSDK APB UART，这是 ARM Cortex-M 系统设计套件中的标准 UART 外设。相比 PL011 这类复杂的 UART，CMSDK APB UART 寄存器极少，非常适合学习。

UART0 的基址是 `0xe7c00000`（CPU0 专用 UART，连接到 QEMU 的第一个串口后端）。从这个地址开始依次排列以下寄存器：

| 偏移 | 名称 | 作用 |
| --- | --- | --- |
| `+0x00` | DATA | 数据寄存器。写入一个字节，UART 就发出这个字节 |
| `+0x04` | STATE | 状态寄存器。bit 1 = 发送缓冲区满（1 表示满，需等待） |
| `+0x08` | CTRL | 控制寄存器。bit 0 = 发送使能（写 1 开启发送） |

> **提示：** QEMU 模拟的 UART 通常不需要配置波特率，速率由模拟器控制。但 CTRL 寄存器的发送使能位仍然需要置 1，否则向 DATA 写入的字节不会被发送。

# 发送单个字符

## 步骤一：定义寄存器地址

在 `src/uart.rs` 中定义 UART 寄存器地址和基础操作（也可以直接写在 `main.rs` 里，本章为清晰起见单独建文件）：

```rust
const UART0_BASE: usize = 0xe7c00000;

const UART0_DATA:  *mut   u32 = (UART0_BASE + 0x00) as *mut   u32;
const UART0_STATE: *const u32 = (UART0_BASE + 0x04) as *const u32;
const UART0_CTRL:  *mut   u32 = (UART0_BASE + 0x08) as *mut   u32;
```

这里用 `*mut u32` 和 `*const u32` 区分"可写寄存器"和"只读寄存器"，虽然不是硬性要求，但能帮助读者理解哪些寄存器只能读、哪些可以写。

> **注意：** 裸指针操作必须放在 `unsafe` 块里。这是 Rust 提醒你：你在绕过内存安全检查，直接操作硬件地址，后果自负。

## 步骤二：实现 uart_init 和 uart_putc

```rust
/// 初始化 UART：开启发送使能
pub fn uart_init() {
    unsafe {
        // CTRL bit 0 = TX enable（发送使能）
        UART0_CTRL.write_volatile(0x1);
    }
}

/// 发送单个字节
pub fn uart_putc(byte: u8) {
    unsafe {
        // 等待发送缓冲区不满（STATE bit 1 = TXBF，1 表示满）
        while (UART0_STATE.read_volatile() & 0x2) != 0 {}
        // 写入数据寄存器，UART 开始发送
        UART0_DATA.write_volatile(byte as u32);
    }
}
```

`write_volatile` 和 `read_volatile` 是 Rust 的"不可优化读写"，告诉编译器不要把这些操作优化掉——对于硬件寄存器，每次读写都有意义，不能被跳过或重排。如果用普通的指针赋值，编译器可能会认为"这段内存没被读过，写了也没用"，直接把整个操作删掉。

## 步骤三：发送字符串

有了 `uart_putc`，发送字符串只需遍历每个字节：

```rust
/// 发送字符串
pub fn uart_puts(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}
```

`s.bytes()` 把字符串的每个字节（UTF-8 编码）依次取出来。对于纯 ASCII 文本，每个字符对应一个字节。

# 实现 print! 宏

`uart_puts` 只能发送固定字符串。实际调试时需要打印变量值，比如 `print!("count = {}", count)`。这就需要格式化输出。

Rust 的 `core::fmt` 模块提供了格式化的底层机制，我们只需要告诉它"格式化完了把字节输出到哪里"。

## 步骤一：实现 Write trait

`core::fmt::Write` 是 Rust 用于格式化输出的 trait。只要我们为自己的类型实现 `write_str` 方法，格式化系统就能用 UART 发送格式化后的字符串：

```rust
use core::fmt::{self, Write};

/// UART 写入器，用于对接 core::fmt 的格式化系统
struct UartWriter;

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        uart_puts(s);
        Ok(())
    }
}
```

这个 `UartWriter` 是个空结构体，只是一个"把字节送进 UART"的接口。`core::fmt` 负责把 `"count = {}"` 加上参数格式化成完整字符串，然后调用我们的 `write_str` 把结果传给 UART。

## 步骤二：实现 print! 和 println! 宏

```rust
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let mut w = $crate::uart::UartWriter;
        core::fmt::write(&mut w, core::format_args!($($arg)*)).ok();
    }};
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {{
        $crate::print!($($arg)*);
        $crate::print!("\n");
    }};
}
```

`format_args!($($arg)*)` 会在编译时把格式字符串和参数打包成一个 `fmt::Arguments` 对象，再由 `core::fmt::write` 调用我们的 `write_str` 输出。整个过程不需要堆内存分配，完全在栈上完成。

## 步骤三：完整的 src/uart.rs

```rust
use core::fmt::{self, Write};

const UART0_BASE: usize = 0xe7c00000;

const UART0_DATA:  *mut   u32 = (UART0_BASE + 0x00) as *mut   u32;
const UART0_STATE: *const u32 = (UART0_BASE + 0x04) as *const u32;
const UART0_CTRL:  *mut   u32 = (UART0_BASE + 0x08) as *mut   u32;

pub fn uart_init() {
    unsafe {
        UART0_CTRL.write_volatile(0x1);
    }
}

pub fn uart_putc(byte: u8) {
    unsafe {
        while (UART0_STATE.read_volatile() & 0x2) != 0 {}
        UART0_DATA.write_volatile(byte as u32);
    }
}

pub fn uart_puts(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}

pub struct UartWriter;

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        uart_puts(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let mut w = $crate::uart::UartWriter;
        core::fmt::write(&mut w, core::format_args!($($arg)*)).ok();
    }};
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {{
        $crate::print!($($arg)*);
        $crate::print!("\n");
    }};
}
```

## 步骤四：更新 src/main.rs

将 UART 模块引入，并在 `rust_main` 里输出：

```rust
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
```

# 验证方法

编译并用 QEMU 运行，观察串口输出：

```bash
cargo build
qemu-system-arm \
  -machine mps3-an536 \
  -nographic \
  -device loader,file=target/armv8r-none-eabihf/debug/rtos
```

预期输出：

```text
Hello from RTOS!
Board: mps3-an536  CPU: Cortex-R52
```

之后 QEMU 进入 `loop {}`，屏幕无新输出。按 **Ctrl+A 然后 X** 退出。

> **注意：** 如果没有任何输出，可能是 UART 基地址不对。检查 QEMU 文档或用 `-d guest_errors` 标志查看访问了哪些外设地址：

```bash
qemu-system-arm -machine mps3-an536 -nographic -device loader,file=... -d guest_errors 2>&1 | head
```

# 练习题

```quiz single
Q: 为什么向硬件寄存器读写必须使用 write_volatile / read_volatile，而不是普通指针赋值？
- 因为 volatile 访问速度更快
- 因为硬件寄存器地址超出了正常内存范围
+ 因为编译器可能把"看似无用"的内存读写优化掉，volatile 告诉编译器每次操作都有实际意义不能省略
- 因为 Rust 的借用检查器不允许普通指针访问固定地址
E: 编译器的优化器会分析数据流，发现"写入了一个值但没有读取"时可能直接删掉写入操作。对于普通内存这是合理的，但对于硬件寄存器，写入操作本身就是向硬件发命令，不能被删除。volatile 读写绕过这类优化，保证每条读写指令都实际执行。
```

```quiz single
Q: uart_putc 函数里 while (STATE.read_volatile() & 0x2) != 0 {} 这个循环的目的是什么？
- 等待接收缓冲区有新数据
+ 等待发送缓冲区空出来，防止新字节在上一个字节还没发完时覆盖掉它
- 检测 UART 是否初始化完成
- 等待对方发回确认信号
E: CMSDK APB UART 的 STATE 寄存器 bit 1（TXBF）为 1 时表示发送缓冲区已满，不能写入新数据。while 循环持续检查这个位，直到为 0（缓冲区有空间）才继续写入，防止数据丢失。
```

```quiz single
Q: 为什么实现 print! 宏要先实现 core::fmt::Write trait，而不是直接调用 uart_putc？
- 因为 uart_putc 太慢，Write trait 会自动缓冲数据
+ 因为 core::fmt 的格式化系统（如 {} 占位符）需要一个实现了 Write 的类型来接收格式化后的字符串，通过 trait 可以复用整个格式化基础设施
- 因为 Write trait 会自动处理 UTF-8 编码
- 因为直接调用 uart_putc 无法在 no_std 环境使用
E: core::fmt::write() 函数接受一个 &mut dyn Write 参数，负责把格式字符串和参数组合成最终字符串，每拼出一段就调用 write_str 输出。我们只要实现 write_str（转调 uart_puts），就能借用整个格式化系统，自动支持 {}、{:x}、{:#?} 等所有格式符号。
```
