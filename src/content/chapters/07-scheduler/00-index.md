---
title: "调度器设计概述"
description: "理解 RTOS 调度器的整体设计：从协作式到真抢占，以及 FIQ 中断触发任务切换的完整方案"
difficulty: intermediate
estimatedTime: 20
keywords: ["调度器", "抢占", "时间片", "Round-Robin", "SRSDB", "RFEIA", "context frame"]
---

# 本章目标

- 理解协作式调度与抢占式调度的本质区别
- 了解本章实现的调度器全貌：Round-Robin 时间片 + FIQ 真抢占
- 掌握 16 字 context frame 的设计，理解为什么需要这个格式
- 了解 ARM 两条专用指令 `SRSDB` / `RFEIA` 如何优雅地解决 FIQ 抢占难题

## 本章结构

| 小节 | 内容 |
| --- | --- |
| `01-ready-queue.md` | 调度器数据结构、Round-Robin 算法、任务状态、任务退出 |
| `02-preemption.md` | 16 字 context frame、FIQ handler 改写、与调度器联动 |

# 前置知识

## 已完成的章节

`06-context-switch` 已完成，`Task` 结构体、`create_task`、协作式 `context_switch` 均可用，两个任务可以手动轮换执行。

## 了解 ARM 异常模式

AArch32 有多种处理器模式，每种模式有**独立的 SP 和 LR**（banked 寄存器）：

| 模式 | 编号 | 触发条件 | 额外 banked 的寄存器 |
| --- | --- | --- | --- |
| SVC（Supervisor） | 0x13 | SVC 指令 | sp, lr |
| IRQ | 0x12 | 普通中断 | sp, lr |
| FIQ（Fast IRQ） | 0x11 | 快速中断 | **r8-r14**（共 7 个！） |
| SYS（System） | 0x1F | 编程切换 | 与 User 共享 |

FIQ 额外 bank 了 r8-r12，这是它速度快（handler 不需要保存这些寄存器）的原因，同时也是从 FIQ 模式访问被中断任务的 r8-r12 变得困难的原因。

# 协作式 vs 抢占式

## 协作式调度（第 06 章的做法）

任务主动调用 `context_switch()` 让出 CPU：

```text
Task A: ... do work ... → context_switch() → Task B 运行
Task B: ... do work ... → context_switch() → Task A 运行
```

**优点**：实现简单，任务完全控制切换时机。  
**缺点**：如果某个任务陷入死循环或长时间计算，其他任务永远得不到执行。

## 抢占式调度（本章实现）

FIQ 定时中断**强制**打断当前任务，切换到下一个：

```text
Task A 运行 → FIQ 触发 → 保存 A 的完整现场 → 调度器选 B → 恢复 B 的现场 → B 继续运行
Task B 运行 → FIQ 触发 → 保存 B 的完整现场 → 调度器选 C → 恢复 C 的现场 → C 继续运行
```

**优点**：任务无需主动让出，即使某个任务死循环也不影响其他任务。  
**缺点**：必须正确保存/恢复所有寄存器，包括条件标志（CPSR），稍有出错就会崩溃。

# 16 字 Context Frame 设计

第 06 章的 14 字 frame（`push {r0-r12, lr}`）用于协作式切换已经够用，但对真抢占**不够**：

| 缺失的内容 | 为什么必须保存 |
| --- | --- |
| **CPSR 条件标志** | 任务被中断时可能正处于一个 `cmp` 之后还没执行 `beq` 的位置，条件标志丢失会跳到错误地址 |
| **LR_svc（任务自己的 lr）** | 任务在某个函数调用链中间被中断，lr 里是函数的返回地址，不保存就无法正确返回 |

本章统一使用 **16 字 frame（64 字节）**，协作式和抢占式共用同一格式：

```text
sp+0:  r0        ← 低地址
sp+4:  r1
sp+8:  r2
sp+12: r3
sp+16: r4
sp+20: r5
sp+24: r6
sp+28: r7
sp+32: r8
sp+36: r9
sp+40: r10
sp+44: r11
sp+48: r12
sp+52: lr_svc    ← 任务的 lr 寄存器（函数返回地址）
sp+56: resume_pc ← 任务恢复执行的 PC（协作式 = 函数返回地址，抢占式 = 被中断的 PC）
sp+60: cpsr      ← 任务的 CPSR（含条件标志）
```

恢复路径对两种情况完全相同：

```asm
pop  {r0-r12, lr}   ; 恢复 r0-r12 和 lr_svc
add  sp, sp, #4     ; 跳过 lr_svc 的 4 字节... 不对，见下
```

实际用 ARM 专用指令优雅处理，详见 02 节。

# SRSDB 与 RFEIA：ARM 的 OS 利器

这两条指令是 ARM 架构专门为操作系统上下文保存设计的：

**`SRSDB SP!, #mode`**（Store Return State, Decrement Before）  
把**当前模式**的 LR 和 SPSR 存到**指定模式**的栈上，再更新该模式的 SP。

```asm
; FIQ 模式里：
SUB   LR, LR, #4          ; LR_fiq = 被中断的 PC
SRSDB SP!, #0x13           ; 把 {LR_fiq, SPSR_fiq} 存入 SVC 模式的栈
                           ; → 任务的 SVC 栈顶出现了 [resume_pc, cpsr]
```

一条指令完成了"把 FIQ 模式里的中断现场信息转移到任务 SVC 栈"的操作，绕开了 FIQ r8-r12 banking 的问题。

**`RFEIA SP!`**（Return From Exception, Increment After）  
从栈上加载 PC 和 CPSR，完成异常返回——恢复路径的最后一步。

```asm
RFEIA SP!    ; PC = [sp], CPSR = [sp+4], sp += 8，跳到 PC，同时恢复 CPSR
```

> **FreeRTOS 做什么？** FreeRTOS ARM_CRx 也用了完全相同的 `SRSDB`/`RFEIA` 技术，但它用 IRQ（不用 FIQ）做 tick。由于 IRQ 只 bank r13/r14（不 bank r8-r12），它甚至不需要我们这里的 SRSDB 技巧——但 RFEIA 同样是它恢复路径的核心。

# 调度器整体结构

```text
┌─────────────────────────────────────────────┐
│               FIQ handler                   │
│  SRSDB + CPS + PUSH → save context          │
│  BL scheduler_tick() → tick++, 唤醒, 调度   │
│  POP + RFEIA          → restore context     │
└────────────────┬────────────────────────────┘
                 │ BL
┌────────────────▼────────────────────────────┐
│            scheduler_tick()                  │
│  ① 唤醒 sleep_until 到期的 Sleeping 任务     │
│  ② 递减当前任务时间片                        │
│  ③ 检测是否有更高优先级任务刚被唤醒          │
│  ④ 若需切换：更新 CURRENT_TASK              │
└─────────────────────────────────────────────┘

任务状态机：
  add_task()       → Ready
  scheduler 选中   → Running
  sleep_ticks(n)   → Sleeping（真正让出 CPU，n tick 后自动唤醒 → Ready）
  time slice ends  → Ready（重新入队，保留优先级）
  task_exit()      → Zombie（永不再调度）
  all sleeping     → idle 任务（wfi 低功耗等待）
```

| 小节 | 内容 |
| --- | --- |
| `01-ready-queue.md` | 优先级就绪队列、`sleep_ticks`、空闲任务、完整 scheduler_tick |
| `02-preemption.md` | SRSDB/RFEIA 实现、16 字 context frame、`add_task(entry, priority)` |

# 验证方法

完成 01 和 02 小节后，运行：

```bash
cargo build
qemu-system-arm \
  -machine mps3-an536 \
  -nographic \
  -device loader,file=target/armv8r-none-eabihf/debug/rtos
```

预期输出（三个任务被 FIQ 抢占，轮流运行）：

```text
Hello from RTOS!
[Task 0] count=0
[Task 1] count=0
[Task 2] count=0
[Task 0] count=1
[Task 1] count=1
```

# 练习题

```quiz single
Q: 协作式调度和抢占式调度的根本区别是什么？
- 协作式更快，抢占式更慢
+ 协作式由任务主动让出 CPU，抢占式由硬件中断强制切换，任务无需配合
- 协作式只支持两个任务，抢占式支持更多
- 协作式不能用于嵌入式系统
E: 协作式调度的"协"字指任务之间相互配合、主动让出。一旦某个任务出现 bug 陷入死循环，整个系统就卡死。抢占式调度依靠定时中断强制打断任意任务，即使某个任务死循环，定时器仍然会触发，调度器仍然能切换到其他任务。
```

```quiz single
Q: 为什么 14 字 frame（第 06 章的 push {r0-r12, lr}）对真抢占不够？
+ 缺少 CPSR 条件标志和任务自己的 lr_svc，任务在任意点被中断时这两个值都可能正在使用中
- 因为 14 字 frame 只适用于协作式切换
- 因为 push {r0-r12, lr} 在 FIQ 模式下会保存错误的 r8-r12
- 因为 14 字 frame 不够放下所有寄存器
E: 在协作式切换中，任务主动调用 context_switch()，CPSR 条件标志已经"消耗完"了（没有未执行的条件分支依赖它），lr 也等于函数的返回地址（这正是我们需要保存的）。而在抢占式切换中，任务可能在任何指令之间被中断，此时 CPSR 可能正在等待一个条件判断，lr 也可能是函数调用链中的某个中间地址，两者都必须精确保存和恢复。
```

```quiz single
Q: SRSDB SP!, #0x13 在 FIQ 模式下执行，它在哪里存储什么内容？
- 在 FIQ 模式的栈上存储 r0-r7 的值
+ 在 SVC 模式（0x13）的栈上存储 LR_fiq（= 被中断的 PC）和 SPSR_fiq（= 被中断的 CPSR）
- 在 SVC 模式的栈上存储 FIQ 模式的 r8-r12
- 在 FIQ 模式的栈上存储 SVC 模式的 sp 和 lr
E: SRS = Store Return State，存储"返回状态"（即异常返回所需的 LR 和 SPSR）。DB = Decrement Before（先递减 SP 再存储）。#0x13 = SVC 模式。所以这条指令把当前模式（FIQ）的 LR 和 SPSR 存到 SVC 模式的栈上，并更新 SVC 模式的 SP。这正好把"被中断的 PC"和"被中断的 CPSR"从 FIQ 的 banked 寄存器里"搬"到了任务的 SVC 栈上。
```
