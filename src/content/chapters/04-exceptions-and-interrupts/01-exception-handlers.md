---
title: "ARM 异常处理机制与 handler 实现"
description: "理解 ARM 处理器模式和 banked 寄存器，实现能正确保存现场并返回的异常处理函数"
difficulty: intermediate
estimatedTime: 60
keywords: ["处理器模式", "banked 寄存器", "SPSR", "异常返回", "context save", "undef", "data abort"]
---

# 本章目标

- 理解 ARM AArch32 的 7 种处理器模式以及 banked 寄存器的作用
- 掌握异常发生时 CPU 自动保存了什么、没保存什么
- 为每种异常编写汇编包装函数，正确保存/恢复现场
- 理解不同异常类型的返回指令差异（LR 偏移不同）
- 实现能打印错误信息的 fault handler，替换之前的死循环桩

## 前置知识

### 已完成的章节

`04-exceptions-and-interrupts/00-index.md` 已完成，向量表已就位，8 个异常入口已注册。

### 了解 push/pop 汇编

知道 `push {r0-r3}` 是把寄存器压入栈，`pop {r0-r3}` 是弹出。

# ARM 的 7 种处理器模式

ARM AArch32 定义了 7 种处理器模式，不同模式下 CPU 有不同的权限和不同的寄存器视图：

| 模式 | 缩写 | 触发方式 | 用途 |
| --- | --- | --- | --- |
| User | USR | 应用代码运行时 | 普通用户代码，权限最低 |
| System | SYS | 手动切换 | 和 User 共享寄存器，但有特权 |
| Supervisor | SVC | 执行 `svc` 指令 | 系统调用入口 |
| IRQ | IRQ | 外部普通中断 | 处理 IRQ |
| FIQ | FIQ | 外部快速中断 | 处理 FIQ |
| Abort | ABT | 访问非法内存 | 处理 Prefetch/Data Abort |
| Undefined | UND | 执行未定义指令 | 处理 Undef |

类比：把处理器模式想象成一个公司的不同部门——每个部门有自己的内线电话（banked 寄存器），接到紧急电话时自动切换到对应部门接听，不影响其他部门正在进行的工作。

## Banked 寄存器

"banked"（独立的）寄存器的含义是：同一个名字的寄存器，在不同模式下是**物理上不同的存储单元**。

IRQ、FIQ、SVC、ABT、UND 这几种异常模式各自都有独立的 `sp`（SP_irq、SP_abt 等）和 `lr`（LR_irq、LR_abt 等）。这样的好处是：发生异常时，CPU 切换到异常模式，立即就有一个干净的 SP 和 LR 可用，不会破坏被打断代码的栈和返回地址。

> **注意：** `r0`–`r12` 没有 banked 版本（FIQ 例外，它有独立的 `r8`–`r12`）。这意味着异常处理函数**必须手动保存** `r0`–`r12`，否则会覆盖被中断代码的数据。

# 异常发生时 CPU 自动做了什么

以 IRQ 为例，当外部中断触发时，CPU **自动完成**以下动作（不需要软件干预）：

1. `SPSR_irq ← CPSR`（把当前程序状态保存到 IRQ 模式的 SPSR）
2. `LR_irq ← 下一条应执行指令的地址 + 4`（记录返回地址，带 +4 偏移）
3. 切换到 IRQ 模式（CPSR 中的模式位改变，SP 和 LR 自动切换到 SP_irq、LR_irq）
4. 禁用 IRQ（防止嵌套中断，CPSR.I 置 1）
5. 跳转到向量表 `0x00000018`（执行我们的 `b irq_handler` 指令）

CPU **没有**自动保存的东西：`r0`–`r12`，这些必须由我们的处理函数手动保存。

# 如何写一个正确的 handler

以 IRQ handler 为例，框架如下：

```asm
irq_handler:
    push {r0-r12, lr}        @ 保存 r0-r12 + LR_irq（含返回地址）
    bl   rust_irq_handler    @ 调用 Rust 实现的中断处理逻辑
    pop  {r0-r12, lr}        @ 恢复寄存器
    subs pc, lr, #4          @ 从 IRQ 返回：PC = LR - 4，同时恢复 CPSR
```

**为什么不需要在 handler 开头初始化 SP？** 每种异常模式都有独立的 banked SP，上电时确实是未初始化的。本章将在步骤三更新 `reset_handler`，在进入 `rust_main` 之前为所有异常模式批量设置好 SP——这比在每个 handler 里各自初始化更安全（因为连 handler 入口的第一条指令都要用到 SP）。

最后一行 `subs pc, lr, #4` 是 AArch32 从异常返回的标准写法：
- 把 `LR - 4` 写入 PC（跳回被中断的代码）
- `s` 后缀表示同时把 SPSR_irq 恢复回 CPSR（恢复被打断时的处理器状态）

没有这个 `s` 后缀，CPSR 不会恢复，CPU 会留在 IRQ 模式继续运行，结果一片混乱。

# 不同异常类型的返回指令

不同异常进入时，LR 的偏移不同，返回指令也不同：

| 异常类型 | LR 的含义 | 返回指令 | 说明 |
| --- | --- | --- | --- |
| IRQ / FIQ | 被打断指令 + 4 | `subs pc, lr, #4` | 返回被打断的那条指令 |
| Data Abort | 出错指令 + 8 | `subs pc, lr, #8` | 返回出错的那条指令重试 |
| Prefetch Abort | 出错指令 + 4 | `subs pc, lr, #4` | 返回出错的取指地址 |
| Undefined | 出错指令下一条 | `movs pc, lr` | LR 已经是正确返回地址 |
| SVC | SVC 指令下一条 | `movs pc, lr` | LR 已经是正确返回地址 |

> **提示：** 记不住偏移？只需记一条规则：**LR 始终指向"如果什么都没发生，接下来要执行的地址"**。IRQ 时 CPU 流水线已经取了下一条指令，LR 多记录了 4 字节，所以要减 4。其他异常类推。

# 实现 fault handler

## 步骤一：汇编包装函数

为每种异常写一个汇编包装器，保存现场后调用 Rust 函数：

```rust
global_asm!(r#"
    // 异常处理函数（各模式 SP 已由 reset_handler 批量初始化，handler 无需再设置）
    .section .text.handlers, "ax"

    undef_handler:
    push {{r0-r12, lr}}
    bl rust_undef_handler      // 不会返回（-> !）
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
    sub lr, lr, #8             // Data Abort LR 偏移 8
    push {{r0-r12, lr}}
    bl rust_data_handler
    pop {{r0-r12, lr}}
    movs pc, lr

    hang:
    wfi
    b hang

    irq_handler:               // 将在 02-gic-setup.md 中替换为真实实现
    push {{r0-r12, lr}}
    bl rust_irq_handler
    pop {{r0-r12, lr}}
    subs pc, lr, #4

    fiq_handler:
    push {{r0-r12, lr}}
    bl rust_fiq_handler
    pop {{r0-r12, lr}}
    subs pc, lr, #4
"#);
```

## 步骤二：Rust 处理函数

```rust
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
pub extern "C" fn rust_irq_handler() {
    // 将在 02-gic-setup.md 中实现
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_fiq_handler() {
    // 暂不使用
}
```

## 步骤三：完善 Reset Handler（填补系统状态与影子栈）

在最终拼装代码前，你会在完整的 `reset_handler` 里看到两段关键汇编代码。第一段 **HYP 模式降级**从第 2 章延续而来；第二段**各异常模式 SP 批量初始化**是本章新引入的内容——正是因为在 `reset_handler` 里统一完成了所有模式的 SP 设置，单个 handler 才不再需要各自执行 `ldr sp`。

### 1. HYP 模式降级（必须切到 SVC）

```rust
    // mps3-an536 以 HYP 模式启动，需切换到 SVC 模式才能使用普通向量表
    mrs r0, cpsr
    and r0, r0, #0x1f
    cmp r0, #0x1a          // 0x1a = HYP 模式
    bne .Lnormal_init
    mov r0, #0xd3          // SVC 模式（AArch32 EL1），禁 IRQ/FIQ
    msr spsr_cxsf, r0
    adr r0, .Lnormal_init
    msr elr_hyp, r0
    eret                   // 切换到 SVC 模式，跳到 .Lnormal_init
    .Lnormal_init:
```

**mps3-an536** 模拟器上电时，CPU 默认处于**虚拟化特权（HYP，AArch32 EL2）模式**。在 HYP 模式下，中断向量表受另一个独立寄存器（HVBAR）控制，与我们的 `0x00000000` 无关。若不切换到普通的内核特权模式（SVC），系统根本不会去看我们写在 0 地址的向量表，随后的任何外设中断也永远无法触发我们的处理代码。

所以必须在清空内存之前，利用 `eret`（异常返回）指令，硬生生把 CPU 从 HYP 模式“退回”到 SVC 模式，并跳向 `.Lnormal_init` 标号，从而真正取回普通系统的控制权。

### 2. 寄存器影子系统与各模式栈指针（SP）初始化

```rust
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
```

在 ARM 中，**每进一种异常模式，CPU 都会切出一套专属于该模式的“影子分身（Banked Registers）”——其中包括独立的栈指针 SP**。如果你只在 SVC 模式下设定了 SP，一旦触发 Data Abort 硬件将 CPU 切入 Abort 模式，它使用的专属影子 SP 就会是一片未经设定的内存垃圾！此时你在处理函数里哪怕只要执行一次 `push` ，就会立刻因为内存崩溃（踩到非法地址）引发绝望的连环死机！

所以，在进入内核（`rust_main`）执行前，我们必须通过操作控制寄存器（`CPSR_c`）反复横跳到各个即将使用到的异常模式（Undefined: `0xdb`、Abort: `0xd7`、IRQ: `0xd2` 等），并为它们逐一将 `sp` 设为 `_stack_start`（在本小项目中，为了省事，所有的异常目前共享同一个大工作栈），全部安顿好以后最后再切回 SVC（`0xd3`）。

## 步骤四：完整的 src/main.rs

```rust
#![no_std]
#![no_main]

mod uart;

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

    fiq_handler:
    push {{r0-r12, lr}}
    bl rust_fiq_handler
    pop {{r0-r12, lr}}
    subs pc, lr, #4

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

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart::uart_init();
    println!("Hello from RTOS!");
    println!("Board: mps3-an536  CPU: Cortex-R52");

    // 取消下面任意一行注释，触发对应的异常处理器：
    // unsafe { core::arch::asm!("udf #0"); }   // → FAULT: Undefined Instruction
    // unsafe { core::arch::asm!("svc #0"); }   // → FAULT: SVC (not implemented)

    loop {}
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
pub extern "C" fn rust_fiq_handler() {}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("PANIC!");
    loop {}
}
```

# 验证方法

## 正常启动验证

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

## 验证未定义指令异常

在 `rust_main` 里取消注释 `udf #0` 那行，然后重新编译运行：

```rust
unsafe { core::arch::asm!("udf #0"); }   // 取消注释这行
```

`udf`（Undefined instruction）是 ARM 提供的标准"故意触发未定义指令异常"的指令，执行后 CPU 立即跳入 `undef_handler`。

预期输出：

```text
Hello from RTOS!
Board: mps3-an536  CPU: Cortex-R52
FAULT: Undefined Instruction
```

## 验证 SVC 异常

在 `rust_main` 里取消注释 `svc #0` 那行：

```rust
unsafe { core::arch::asm!("svc #0"); }   // 取消注释这行
```

`svc #0` 主动发起一次 Supervisor Call，CPU 跳入 `svc_handler`。

预期输出：

```text
Hello from RTOS!
Board: mps3-an536  CPU: Cortex-R52
FAULT: SVC (not implemented)
```

> **注意：** 验证完毕后记得把这两行重新注释掉，后续章节的代码以正常启动为基础。

# 练习题

```quiz single
Q: ARM AArch32 中，IRQ 异常发生时 CPU 自动保存了哪些内容？
- r0-r12、CPSR、PC（全部寄存器）
- 只保存了 PC
+ 把 CPSR 保存到 SPSR_irq，把返回地址保存到 LR_irq，然后切换到 IRQ 模式
- 什么都不保存，全靠软件
E: CPU 自动做的只有三件事：保存 CPSR → SPSR_irq、计算返回地址存入 LR_irq、切换处理器模式。r0-r12 没有被保存，处理函数必须手动 push {r0-r12} 才能保护被打断代码的数据。
```

```quiz single
Q: 为什么 IRQ handler 返回时要用 subs pc, lr, #4 而不是 subs pc, lr, #0？
+ 因为 IRQ 时 LR 存的是"被打断指令的下一条 + 4"，减 4 才能正确返回到被打断处继续执行
- 因为 ARM 指令都是 4 字节，需要对齐
- 因为 SPSR 需要额外 4 个周期才能生效
- 因为中断处理本身消耗了 4 字节的栈空间
E: ARM 流水线导致 LR_irq = 实际返回地址 + 4。subs pc, lr, #4 减去这个偏移，同时 s 后缀触发 SPSR_irq → CPSR 的恢复，一条指令完成返回和状态恢复两件事。
```

```quiz single
Q: Data Abort 的返回指令是 subs pc, lr, #8，而不是 #4，原因是什么？
- 因为 Data Abort 处理函数比 IRQ 多占用 4 字节栈空间
+ 因为 Data Abort 时 LR 指向出错指令 + 8，减 8 才能回到出错的那条指令重试
- 因为 Data Abort 需要额外一个时钟周期处理，导致偏移加倍
- 因为 Data Abort 和 IRQ 使用不同的 LR 寄存器
E: 不同异常类型发生时，CPU 保存到 LR 的偏移不同，是 ARM 架构规范的一部分。Data Abort 时 LR = 出错指令 + 8，减 8 可以返回到触发错误的指令，让软件有机会修复映射后重试（在有 MMU/MPU 的系统中）。
```

```quiz single
Q: 异常 handler 的 push {{r0-r12, lr}} 中为什么要把 lr 也 push 进去？
- 因为 ARM 规定所有寄存器都必须保存
- 因为 lr 是 r14，必须和其他寄存器一起保存才能对齐
+ 因为 lr 保存的是返回地址，push 之后 bl 调用 Rust 函数会覆盖 lr，必须提前保存才能在返回时还原
- 因为 Rust 函数会修改 lr 寄存器
E: bl 指令会把返回地址写入 lr，覆盖异常进入时 CPU 存的原始返回地址。如果不在 bl 之前把 lr push 到栈上，异常处理结束后就无从得知应该返回到哪里。pop {{r0-r12, lr}} 恢复 lr，subs pc, lr, #4 才能跳回正确的地址。
```
