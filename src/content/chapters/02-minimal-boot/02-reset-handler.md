---
title: "汇编入口与 Rust no_std 程序"
description: "编写 reset handler 汇编完成栈初始化和内存初始化，搭建最小 Rust 裸机程序框架"
difficulty: intermediate
estimatedTime: 60
keywords: ["reset handler", "汇编", "no_std", "no_main", "栈初始化", "BSS", "global_asm"]
---

# 本章目标

- 理解寄存器和常用汇编指令的基本含义
- 编写 reset handler 完成三步初始化：设置栈、清零 BSS、复制 .data
- 用 `global_asm!` 将汇编嵌入 Rust 源文件
- 编写 `#[no_std]` 的 Rust 入口函数
- 编译成功并在 QEMU 上启动，无报错，程序进入死循环

## 前置知识

### 已完成的章节

`02-minimal-boot/01-memory-layout.md` 已完成，项目目录下已有 `link.x`、`Cargo.toml`、`.cargo/config.toml`。

### 了解 .data 和 .bss 的区别

知道 .data 存有初始值的全局变量、.bss 存零值变量、Flash 断电不丢失而 RAM 会清空。这在上一篇中已经解释过。

# 汇编基础速查

在写 reset handler 之前，先快速了解会用到的汇编概念。不需要系统学汇编，只要能看懂下面这几样东西就够。

## 寄存器是什么

寄存器是 CPU 内部的"临时变量格"。Cortex-R52 有 16 个通用寄存器，常用的是：

| 寄存器 | 别名 | 用途 |
| --- | --- | --- |
| `r0`–`r3` | — | 通用临时变量，函数前四个参数也放这里 |
| `r13` | `sp` | 栈指针（Stack Pointer），指向当前栈顶位置 |
| `r14` | `lr` | 链接寄存器（Link Register），保存函数返回地址 |
| `r15` | `pc` | 程序计数器（Program Counter），保存下一条指令地址 |

类比：把 CPU 想象成一个工人，寄存器就是他桌上摆的几个便利贴，每次只能记几个数字，用完随时可以覆盖。

## 常用指令速查

| 指令 | 含义 | 例子 |
| --- | --- | --- |
| `ldr r0, =VALUE` | 把立即数或符号地址装入 r0 | `ldr sp, =_stack_start` |
| `mov r0, #4` | 把数字 4 装入 r0 | `mov r2, #0` |
| `str r0, [r1]` | 把 r0 的值写入 r1 指向的内存地址 | `str r2, [r0]` |
| `ldr r0, [r1]` | 从 r1 指向的内存地址读取值到 r0 | `ldr r3, [r2]` |
| `add r0, r0, #4` | r0 = r0 + 4 | 让指针向后移动 4 字节 |
| `cmp r0, r1` | 比较 r0 和 r1，结果影响标志位 | 用于条件跳转 |
| `bhs LABEL` | 如果 r0 ≥ r1（无符号），跳转到 LABEL | 循环退出条件 |
| `b LABEL` | 无条件跳转到 LABEL | 循环回跳 |
| `bl LABEL` | 跳转到 LABEL，同时把返回地址存入 lr | 函数调用 |
| `wfi` | Wait For Interrupt，让 CPU 进入低功耗等待 | 空循环代替忙等 |

## 数字标签与跳转

汇编里用数字做局部标签，`1b` 表示"向前找最近的 `1:` 标签"，`2f` 表示"向后找最近的 `2:` 标签"：

```asm
1:
    ...
    b 1b    @ 跳回上面的 1:，形成循环
    ...
2:          @ 这里是循环结束的位置
    bhs 2f  @ 条件成立时，跳到下面的 2:
    ...
```

# 编写 reset handler

## 步骤零：检测并切换 HYP 模式

> **实践中发现：** Cortex-R52 在 mps3-an536 上以 **HYP 模式（0x1A，Hypervisor mode）** 启动，而不是通常预期的 SVC 模式。

这会带来严重问题：FIQ/IRQ 从 HYP 模式触发时，LR 的计算规则与从 SVC 模式触发时不同，导致 `subs pc, lr, #4` 返回到错误地址（我们的向量表！），程序陷入无限循环。

解决方案：在 reset_body 最开头检测是否处于 HYP 模式，如果是，立刻用 `eret` 切换到 SVC 模式：

```asm
reset_body:
    @ 检测 HYP 模式（mps3-an536 以 HYP 启动）
    mrs r0, cpsr
    and r0, r0, #0x1f      @ 取出 CPSR.M（模式位）
    cmp r0, #0x1a          @ 0x1a = HYP 模式
    bne .Lnormal_init      @ 不是 HYP，跳过切换

    @ 在 HYP 模式：设置 SPSR_hyp = SVC + I+F 禁用，然后 ERET
    mov r0, #0xd3          @ SVC 模式 | I=1 | F=1
    msr spsr_cxsf, r0
    adr r0, .Lnormal_init  @ 切换后的入口地址
    msr elr_hyp, r0
    eret                   @ 切换到 SVC 模式

.Lnormal_init:
```

`eret` 是 AArch32 的"异常返回"指令，它把 ELR_hyp（我们设置的 `.Lnormal_init` 地址）加载进 PC，同时把 SPSR_hyp（我们设置的 SVC 模式）恢复到 CPSR，完成模式切换。

## 步骤一：设置栈指针

CPU 上电时 `sp` 寄存器的值是不确定的。第一件事就是把它指向我们在链接脚本里定义的 `_stack_start`：

```asm
ldr sp, =_stack_start
```

`ldr sp, =_stack_start` 是一条伪指令：汇编器会在代码附近放一个"文字池"（literal pool），把 `_stack_start` 的地址值存进去，然后生成一条普通的内存读取指令，把那个地址值装入 `sp`。

执行完这一行，`sp` 就指向 `0x10080000`（BRAM 末尾），栈可以正常使用了。

## 步骤二：清零 BSS 段

`.bss` 段存放初始值为零的全局变量。C 和 Rust 都保证这些变量在程序启动时是零，但 RAM 上电后内容是随机的，所以我们必须手动清零。

清零范围：从 `_sbss` 到 `_ebss`（这两个符号由链接脚本定义）。

```asm
    ldr r0, =_sbss      @ r0 = 当前写入位置（从 BSS 起始开始）
    ldr r1, =_ebss      @ r1 = BSS 结束位置
    mov r2, #0          @ r2 = 要写入的值（0）

1:
    cmp r0, r1          @ 比较当前位置和结束位置
    bhs 2f              @ 如果 r0 >= r1（写完了），跳出循环
    str r2, [r0]        @ 把 0 写入 r0 指向的地址
    add r0, r0, #4      @ r0 向后移动 4 字节
    b 1b                @ 跳回循环开头

2:                      @ 清零完成
```

## 步骤三：复制 .data 段从 Flash 到 RAM

`.data` 段存有初始值的全局变量。初始值保存在 Flash（`_sidata` 开始），但运行时必须从 RAM（`_sdata` 到 `_edata`）访问。启动时需要把这段内容从 Flash 复制到 RAM。

```asm
    ldr r0, =_sdata     @ r0 = RAM 目标起始地址
    ldr r1, =_edata     @ r1 = RAM 目标结束地址
    ldr r2, =_sidata    @ r2 = Flash 来源起始地址

3:
    cmp r0, r1          @ 复制完了吗？
    bhs 4f              @ 如果 r0 >= r1（复制完），跳出循环
    ldr r3, [r2]        @ 从 Flash（r2 地址）读取 4 字节到 r3
    str r3, [r0]        @ 把 r3 写入 RAM（r0 地址）
    add r0, r0, #4      @ 目标指针后移
    add r2, r2, #4      @ 来源指针后移
    b 3b                @ 跳回循环开头

4:                      @ 复制完成
```

## 步骤四：跳转到 Rust 入口

初始化工作完成，跳转到 Rust 的入口函数 `rust_main`：

```asm
    bl rust_main        @ 调用 rust_main

    @ rust_main 声明了 -> !（永不返回）
    @ 理论上永远不会执行到这里
    @ 但以防万一，加一个安全的死循环
5:
    wfi                 @ 等待中断，CPU 进入低功耗
    b 5b
```

`bl` 和 `b` 的区别：`bl` 会把下一条指令的地址存入 `lr`，方便函数返回；`b` 是纯跳转，没有返回意图。调用函数用 `bl`，普通跳转用 `b`。

# 编写 Rust 入口函数

在 Rust 中，可以用 `global_asm!` 宏把汇编代码直接嵌入源文件。把上面四步的汇编和 Rust 的入口函数合并到 `src/main.rs`：

## 步骤一：完整的 src/main.rs

```rust
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
```

**几个关键点解释：**

`#![no_std]` — 不链接依赖操作系统的 `std` 标准库，改用裸机可用的 `core`。

`#![no_main]` — 告诉编译器不要寻找普通的 `fn main()`，程序入口由我们自己控制（就是 `reset_handler`）。

`#[unsafe(no_mangle)]` — 禁止编译器修改函数名。如果没有这个，Rust 会把 `rust_main` 编译成类似 `_ZN4rtos9rust_mainE` 的乱码名，汇编里的 `bl rust_main` 就找不到它了。

`extern "C"` — 使用 C 语言的调用规范（ABI）。汇编直接调用此函数，必须和汇编约定的调用规范一致。

`-> !` — 函数类型签名中 `!` 表示"永不返回"（Never 类型）。`loop {}` 死循环确保了这一点。如果缺少 `panic_handler`，编译器会报错。

# 验证方法

## 编译验证

```bash
cargo build
```

预期输出：

```text
   Compiling core v0.0.0 (...)
   Compiling rtos v0.1.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in Xs
```

无警告，无错误。

## 符号地址验证

```bash
rust-nm target/armv8r-none-eabihf/debug/rtos | grep -E "reset_handler|rust_main|_stack_start"
```

预期输出：

```text
00000000 T reset_handler
000000XX T rust_main
20200000 A _stack_start
```

- `reset_handler` 在 `0x00000000`——CPU 上电第一条指令就是它 ✓
- `rust_main` 紧随其后在 Flash 里 ✓
- `_stack_start` 在 `0x20200000`（RAM 末尾）✓

## QEMU 启动验证

```bash
qemu-system-arm \
  -machine mps3-an536 \
  -nographic \
  -device loader,file=target/armv8r-none-eabihf/debug/rtos
```

程序会进入 `loop {}`，QEMU 保持运行不退出、不报错。用 **Ctrl+A 然后按 X** 退出 QEMU。

> **注意：** 目前没有任何输出，屏幕一片空白是正常的——我们还没有实现 UART 驱动。下一章会在 `rust_main` 里加上串口输出，届时就能看到程序实际在跑了。

# 练习题

```quiz single
Q: reset handler 中必须在调用 rust_main 之前完成的操作，以下哪一项最关键？
- 初始化 UART 外设，否则无法输出调试信息
+ 设置栈指针（SP），否则 rust_main 中的任何函数调用都会因无效栈而崩溃
- 启用 CPU 缓存，否则程序运行太慢
- 初始化中断控制器，否则 Rust 代码无法运行
E: 函数调用依赖栈来保存返回地址和局部变量。SP 未设置时栈指针指向随机地址，第一次 push/函数调用就会写到非法内存，程序立即崩溃。UART、缓存、中断控制器都可以后续初始化。
```

```quiz single
Q: 为什么清零 BSS 段要由我们自己在 reset handler 里手动完成，而不是 CPU 自动做？
- 因为 Cortex-R52 没有硬件清零功能
+ 因为 RAM 上电后内容随机，C/Rust 语言规范保证零值变量初始为零，但硬件不提供这个保证，必须软件实现
- 因为链接脚本无法自动清零内存
- 因为 BSS 段太大，CPU 清零太慢
E: C 和 Rust 语言标准规定全局零值变量（BSS 段）在程序启动时必须为零。但"程序启动"指的是语言运行时开始前，硬件只负责上电，不保证 RAM 的初始值。所以这个工作必须由启动代码（reset handler）在进入语言运行时前完成。
```

```quiz single
Q: #[unsafe(no_mangle)] 属性的作用是什么？
- 防止编译器优化掉这个函数
+ 禁止编译器对函数名进行名称修饰（mangling），确保汇编代码能通过原始名称找到这个函数
- 让函数可以被中断处理程序调用
- 把函数放到特定的内存段
E: Rust（和 C++）编译器会对函数名进行"名称修饰"，在编译后的符号表里变成包含类型信息的乱码名字。#[unsafe(no_mangle)] 禁止这个行为，让函数名在符号表里保持原样。汇编里 bl rust_main 依赖函数名的原始形式，少了这个属性链接器就找不到 rust_main。
```

```quiz single
Q: global_asm! 宏把汇编放在 .text.reset_handler 节，链接脚本里的哪一行保证了它被放在 0x00000000？
- .text : { *(.text .text.*) } > FLASH
+ KEEP(*(.text.reset_handler)) 在 .text 块的第一行，确保该节被放在 Flash 最开头
- ENTRY(reset_handler) 指令直接把函数放在地址 0
- FLASH : ORIGIN = 0x00000000 规定了所有代码的起始地址
E: KEEP(*(.text.reset_handler)) 在 .text 段的 SECTIONS 块里是第一条规则，链接器按顺序处理，所以 .text.reset_handler 节的内容被放在 .text 段的最前面。.text 段起始地址是 FLASH 的 ORIGIN，也就是 0x00000000。ENTRY() 只是声明 ELF 入口点，不改变代码的实际地址。
```
