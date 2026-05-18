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

| 指令 | 含义 | 对应的 C 语言概念 |
| --- | --- | --- |
| `ldr r0, =VALUE` | 把较大的立即数或符号地址装入 r0 | `r0 = &VALUE;` （或 `r0 = VALUE;`） |
| `mov r0, #4` | 把较小的数字 4 装入 r0 | `r0 = 4;` |
| `str r0, [r1]` | 把 r0 的值写入 r1 指向的内存地址 | `*r1 = r0;` |
| `ldr r0, [r1]` | 从 r1 指向的内存地址读取值装入 r0 | `r0 = *r1;` |
| `add r0, r0, #4` | r0 = r0 + 4 | `r0 += 4;` |
| `cmp r0, r1` | 比较 r0 和 r1，结果更新状态标志位 | 供后面的 if 判断使用，如比较两者大小 |
| `bhs LABEL` | 如果（上一条比较无符号）≥，跳转到 LABEL | `if (r0 >= r1) goto LABEL;` |
| `b LABEL` | 无条件跳转到 LABEL | `goto LABEL;` |
| `bl LABEL` | 跳转到 LABEL，同时把返回地址存入 lr | `LABEL();` （带返回机制的函数调用） |
| `wfi` | Wait For Interrupt，让 CPU 进入等待 | 休眠指令，如 `SLEEP();` |

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

# 汇编与 Rust 的无缝结合 (src/main.rs)

在开发这类裸机程序时，传统的做法是分开写一个 `.s` 汇编文件和一个 `.c` 源文件然后再进行链接。但在 Rust 中，我们可以直接使用 `global_asm!` 宏，把底层的初始化汇编代码和高层的业务 Rust 代码优雅地放在同一个文件里。

请打开你的 `src/main.rs` 文件清空默认内容。接下来我们要在这个文件里完成两大核心任务：编写汇编启动入口（`reset_handler`）以及编写 Rust 运行入口（`rust_main`）。

## 第一部分：理清汇编层的初始化逻辑

在将代码完整塞进源文件之前，我们需要先了解一下：CPU 上电后，必须通过纯汇编代码完成哪五个底层的环境初始化操作？

### 声明并安放汇编入口点

每一段汇编代码都需要告诉汇编器它应该被放到哪里，以及是否允许外部调用。
在刚开始时，我们要先写下这三行：

```asm
    // 放在 .text.reset_handler 节，链接脚本会把它放到 0x00000000
    .section .text.reset_handler, “ax”
    .global reset_handler
reset_handler:
```
- **`.section .text.reset_handler, “ax”`**：这是一句编译指令（Directive）。它告诉汇编器把接下来的代码放到名叫 `.text.reset_handler` 的特殊段（Section）里。末尾的 `”ax”` 表示这段内存应当分配且具有可执行权限（Allocatable & Executable）。我们在上一节链接脚本里写过，把 `.text.reset_handler` 固定在 `0x00000000` 首地址，这句指令就完美地跟链接脚本配合在了一起。
- **`.global reset_handler`**：将 `reset_handler` 这个标签暴露成全局符号。这相当于在一个 C 文件里写非 `static` 函数，其他文件也能”看到”它。
- **`reset_handler:`**：标志这段代码在这里真正开始。

---

### 检测并切换 CPU 运行模式 (HYP -> SVC)

> **科普：什么是 HYP 和 SVC 模式？**
> 像 Cortex-R52 这种复杂的 ARM 处理器，并非只有一种运行状态。为了安全和权限管理，它划分了多种**特权模式**：
> - **USR (User)**：最低权限，普通第三方应用程序运行在这里。
> - **SVC (Supervisor)**：特权/超级管理员模式，我们的**操作系统内核**（RTOS）默认且通常运行在这个模式，拥有直接管理外设寄存器的全权。
> - **HYP (Hypervisor)**：虚拟化管理模式，权限比 SVC 还高，用于在一颗物理芯片上同时跑多个独立操作系统（虚拟机管理）。
>
> **为什么要做切换？**
> 实践中发现，Cortex-R52 在 mps3-an536 刚上电时，由于它本身支持虚拟化，所以直接以权限最高的 **HYP 模式（0x1A）** 启动。但这会带来严重问题：由于我们的目标是写一个简单的单系统内核（在 SVC 模式下），如果在运行中发生硬件中断（FIQ/IRQ），从 HYP 模式触发中断的 LR（链接寄存器）计算规则，跟从 SVC 模式触发的规则是不一样的。如果不退回到 SVC 模式，以后中断发生后执行返回指令会导致跳到错误地址甚至死机。

解决方案是在启动最开头检测是否处于 HYP 模式，如果是，主动“降级”并立刻用 `eret` 切换到 SVC 模式：

```asm
reset_handler:
    @ 检测 HYP 模式（mps3-an536 以 HYP 启动）
    mrs r0, cpsr
    and r0, r0, #0x1f      @ 取出 CPSR.M（模式位）
    cmp r0, #0x1a          @ 0x1a = HYP 模式
    bne .Lnormal_init      @ 不是 HYP，跳过切换

    @ 在 HYP 模式：设置 SPSR_hyp = SVC + I+F 禁用，然后 ERET
    mov r0, #0xd3          @ SVC 模式（AArch32 EL1）| I=1 | F=1
    msr spsr_cxsf, r0
    adr r0, .Lnormal_init  @ 切换后的入口地址
    msr elr_hyp, r0
    eret                   @ 切换到 SVC 模式

.Lnormal_init:
```

### 设置栈指针

CPU 上电时 `sp` 寄存器的值是不确定的。我们要把它指向前面链接脚本里定义的 `_stack_start`（即 BRAM 的末尾）：

```asm
ldr sp, =_stack_start
```

### 清零 BSS 段

`.bss` 段存放初始值为零的全局变量。硬件上电时 RAM 的内容是随机的，所以我们必须写一个循环来手动清零这段内存（从 `_sbss` 写到 `_ebss`）：

```asm
    ldr r0, =_sbss      @ r0 = 当前写入位置
    ldr r1, =_ebss      @ r1 = 结束位置
    mov r2, #0          @ r2 = 写 0

1:
    cmp r0, r1          @ 是否写完？
    bhs 2f
    str r2, [r0]
    add r0, r0, #4      @ 指针后移 4 字节
    b 1b                @ 循环
2:
```

> **语法小提示：什么是 `1:`, `2:`, `b 1b`, `bhs 2f`？**
> 这是 GNU 汇编器里的**局部数字标签（Local Labels）**语法。
> - `1:` 和 `2:` 定义了代码里的锚点位置。
> - `b 1b` 中的 `b` 代表 **backward（向后/向上）**，意思是“跳转到上方离我最近的 `1` 标签处”，用来实现循环（继续清零）。
> - `bhs 2f` 中的 `f` 代表 **forward（向前/向下）**，意思是“跳转到下方离我最近的 `2` 标签处”，用来跳出循环。
> 这种写法的最大好处是不用给每个小循环费尽心思起局部名字（如 `loop_start`, `loop_end`），让代码极其简洁。

### 复制 .data 段（Flash -> RAM）

`.data` 段存有初始值的全局变量。初始值永久保存在 Flash（`_sidata`），但运行时必须把它们搬到 RAM（`_sdata` 到 `_edata`）里供 CPU 读写：

```asm
    ldr r0, =_sdata     @ 目标起始
    ldr r1, =_edata     @ 目标结束
    ldr r2, =_sidata    @ 来源起始

3:
    cmp r0, r1          @ 是否搬完？
    bhs 4f
    ldr r3, [r2]        @ 读一字
    str r3, [r0]        @ 写一字
    add r0, r0, #4
    add r2, r2, #4
    b 3b
4:
```

### 跳转到 Rust 业务代码

万事俱备，最后把执行权交给真正的 Rust 主函数 `rust_main`，并留一个“进入低功耗”的死循环作为最后的后备防线：

```asm
    bl rust_main        @ 调用 rust_main
5:
    wfi                 @ CPU 睡眠
    b 5b                @ 死循环保底
```

## 第二部分：编写完整的 src/main.rs

现在你已经理解了底层的 5 步初始化干了什么。我们使用 `global_asm!` 把这些汇编字符串逐行嵌入到 Rust 中，顺便将 `rust_main` 写在底下。

把你刚刚清空的 `src/main.rs` 替换为以下全部内容：

```rust
#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(r#"
    // 放在 .text.reset_handler 节，链接脚本会把它放到 0x00000000
    .section .text.reset_handler, "ax"
    .global reset_handler
    reset_handler:

    // 0. 检测 HYP 模式（mps3-an536 以 HYP 模式启动），切换到 SVC
    mrs r0, cpsr
    and r0, r0, #0x1f
    cmp r0, #0x1a
    bne .Lnormal_init
    mov r0, #0xd3
    msr spsr_cxsf, r0      // SVC 模式（AArch32 EL1），禁 IRQ/FIQ
    adr r0, .Lnormal_init
    msr elr_hyp, r0
    eret
    .Lnormal_init:

    // 1. 设置栈指针
    ldr sp, =_stack_start

    // 2. 清零 BSS 段
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

    // 3. 复制 .data 段从 Flash 到 RAM
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

    // 4. 跳转到 Rust 入口
    bl rust_main

    // 安全保底死循环
    5:
    wfi
    b 5b
"#);

/// 程序真正的入口。reset handler 搞定一切脏活累活后就往这儿跳。
#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
```

## 第三部分：关键 Rust 特性解析

在以上的 Rust 代码中，出现了一些系统级编程才会见到的宏与修饰符：

`#![no_std]` — 不链接依赖操作系统的 `std` 标准库，改用裸机可用的 `core`。

`#![no_main]` — 告诉编译器不要寻找普通的 `fn main()`，程序入口由我们自己控制（就是 `reset_handler`）。

`#[unsafe(no_mangle)]` — 禁止编译器修改函数名。如果没有这个，Rust 会把 `rust_main` 编译成类似 `_ZN4rtos9rust_mainE` 的乱码名，汇编里的 `bl rust_main` 就找不到它了。

`extern "C"` — 使用 C 语言的调用规范（ABI）。汇编直接调用此函数，必须和汇编约定的调用规范一致。

`-> !` — 函数类型签名中 `!` 表示"永不返回"（Never 类型）。`loop {}` 死循环确保了这一点。

`#[panic_handler]` — 在普通的 Rust 程序中，遇到 `panic!`（致命错误）时，标准库会负责把错误打印到终端并退出程序。但在 `#![no_std]` 的裸机环境下没有标准库，编译器要求我们**必须亲自接管 panic 的处理逻辑**。目前我们在里面放了一个死循环，只要程序发生崩溃就把 CPU 卡死在这里防止产生更危险的行为。后续我们有了串口驱动后，还会把这里的 `_info` 参数利用起来，将代码文件路径和崩溃报错信息输出到屏幕上以供调试！

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
00000094 T rust_main
10080000 A _stack_start
```

- `reset_handler` 在 `0x00000000`——CPU 上电第一条指令就是它 ✓
- `rust_main` 紧随其后在 Flash 里 ✓
- `_stack_start` 在 `0x10080000`（BRAM 末尾）✓

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
- 启用 CPU 缓存，否则程序运行太慢
+ 设置栈指针（SP），否则 rust_main 中的任何函数调用都会因无效栈而崩溃
- 初始化中断控制器，否则 Rust 代码无法运行
E: 函数调用依赖栈来保存返回地址和局部变量。SP 未设置时栈指针指向随机地址，第一次 push/函数调用就会写到非法内存，程序立即崩溃。UART、缓存、中断控制器都可以后续初始化。
```

```quiz single
Q: 为什么清零 BSS 段要由我们自己在 reset handler 里手动完成，而不是 CPU 自动做？
+ 因为 RAM 上电后内容随机，C/Rust 语言规范保证零值变量初始为零，但硬件不提供这个保证，必须软件实现
- 因为 Cortex-R52 没有硬件清零功能
- 因为 BSS 段太大，CPU 清零太慢
- 因为链接脚本无法自动清零内存
E: C 和 Rust 语言标准规定全局零值变量（BSS 段）在程序启动时必须为零。但"程序启动"指的是语言运行时开始前，硬件只负责上电，不保证 RAM 的初始值。所以这个工作必须由启动代码（reset handler）在进入语言运行时前完成。
```

```quiz single
Q: #[unsafe(no_mangle)] 属性的作用是什么？
- 防止编译器优化掉这个函数
- 把函数放到特定的内存段
- 让函数可以被中断处理程序调用
+ 禁止编译器对函数名进行名称修饰（mangling），确保汇编代码能通过原始名称找到这个函数
E: Rust（和 C++）编译器会对函数名进行"名称修饰"，在编译后的符号表里变成包含类型信息的乱码名字。#[unsafe(no_mangle)] 禁止这个行为，让函数名在符号表里保持原样。汇编里 bl rust_main 依赖函数名的原始形式，少了这个属性链接器就找不到 rust_main。
```

```quiz single
Q: global_asm! 宏把汇编放在 .text.reset_handler 节，链接脚本里的哪一行保证了它被放在 0x00000000？
- .text : { *(.text .text.*) } > FLASH
- ENTRY(reset_handler) 指令直接把函数放在地址 0
- FLASH : ORIGIN = 0x00000000 规定了所有代码的起始地址
+ KEEP(*(.text.reset_handler)) 在 .text 块的第一行，确保该节被放在 Flash 最开头
E: KEEP(*(.text.reset_handler)) 在 .text 段的 SECTIONS 块里是第一条规则，链接器按顺序处理，所以 .text.reset_handler 节的内容被放在 .text 段的最前面。.text 段起始地址是 FLASH 的 ORIGIN，也就是 0x00000000。ENTRY() 只是声明 ELF 入口点，不改变代码的实际地址。
```
