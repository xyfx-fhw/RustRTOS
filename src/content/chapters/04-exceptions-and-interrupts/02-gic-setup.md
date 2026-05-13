---
title: "GIC 中断控制器与定时器中断"
description: "配置 GICv3 中断控制器，启动 CMSDK DualTimer，实现第一个真正响应硬件的 IRQ handler"
difficulty: advanced
estimatedTime: 75
keywords: ["GIC", "GICv3", "GICD", "ICC", "定时器", "DualTimer", "IRQ handler", "INTID"]
---

# 本章目标

- 理解 GICv3 的三个组件（Distributor、Redistributor、CPU Interface）及其职责
- 配置 GICD 使能 Timer 中断，配置 ICC 系统寄存器使能 CPU 端中断接收
- 配置 CMSDK DualTimer 产生周期性定时中断
- 实现完整的 IRQ handler：应答 → 处理 → 清除 → EOI
- 在 QEMU 上看到定时器每秒输出一次 tick 计数

## 前置知识

### 已完成的章节

`04-exceptions-and-interrupts/01-exception-handlers.md` 已完成，`irq_handler` 汇编包装器和 `rust_irq_handler` 占位函数均已就位。

### 理解内存映射寄存器

知道向特定地址写入数值等于向对应硬件发命令，如第 03 章 UART 驱动中的做法。

# GICv3 的结构

GIC（Generic Interrupt Controller，通用中断控制器）是 ARM 系统中负责收集、分发、路由中断信号的独立硬件模块。可以把它想象成公司的前台——各部门（外设）有事找前台，前台判断找哪个员工（CPU），员工处理完了告诉前台"搞定了"（EOI）。

GICv3 分为三个层次：

**Distributor（GICD）** — 整个系统共享一个。
负责管理所有 SPI（Shared Peripheral Interrupt，共享外设中断）。把它理解为公司前台：接收所有部门的来电，决定这个电话该转给哪个 CPU 核。
- 内存映射寄存器，基址 `0xf0000000`

**Redistributor（GICR）** — 每个 CPU 核各有一个。
负责管理只属于该核的 PPI（Private Peripheral Interrupt）和 SGI（Software Generated Interrupt）。
- 内存映射寄存器，基址 `0xf0100000`

**CPU Interface（ICC_*）** — 每个 CPU 核通过系统寄存器访问。
CPU 读取"是哪个中断触发了我"（IAR），处理完后通知 GIC"我搞定了"（EOI）。
- 通过 AArch32 的 `MRC`/`MCR` 指令访问，**不是**内存映射

三者的协作流程：

```text
外设发出中断信号
     ↓
Distributor（GICD）判断路由给哪个 CPU
     ↓
Redistributor（GICR）转发给对应 CPU 的 IRQ 输入
     ↓
CPU 执行向量表 0x00000018 的 b irq_handler
     ↓
irq_handler 读 ICC_IAR1 获取 INTID
     ↓
处理中断，清除外设的中断标志
     ↓
写 ICC_EOIR1 通知 GIC 结束
```

# 中断编号（INTID）

GICv3 用 INTID 统一编号所有中断：

| 范围 | 类型 | 说明 |
| --- | --- | --- |
| 0–15 | SGI | 软件触发，用于 CPU 间通信 |
| 16–31 | PPI | 每个 CPU 核私有 |
| 32–1019 | SPI | 共享外设中断，来自外部硬件 |

mps3-an536 上 CMSDK DualTimer 的中断连接（来自 QEMU 源码 `mps3r.c`）：

| 定时器 | GIC SPI 编号 | INTID |
| --- | --- | --- |
| Timer 1 | SPI 1 | 33 |
| Timer 2 | SPI 2 | 34 |
| 合并输出 | SPI 3 | 35 |

本章使用 **Timer 1**，INTID = **33**。

# 配置 GIC

> **实践中发现：** mps3-an536 上 INTID 33 默认在 **Group 0（Secure）**，会作为 **FIQ** 而非 IRQ 投递。这是因为 Cortex-R52 以 HYP 模式启动，Group 1 NS 的访问受限。本章选择配合 FIQ 路径（使用 `fiq_handler` 和 `rust_fiq_handler`），而非 IRQ 路径。

## 本章新增文件结构

```text
src/
├── gic.rs      ← 新建：GIC 初始化 + 应答/EOI 辅助函数
├── timer.rs    ← 新建：定时器初始化
└── main.rs     ← 修改：添加模块声明、TICK_COUNT、rust_fiq_handler
```

## 新建 src/gic.rs

将 GIC 所有寄存器常量和初始化函数集中在这一个文件里：

```rust
const GICD_BASE: usize = 0xf0000000;
const GICD_CTLR:       *mut u32 = (GICD_BASE + 0x000) as *mut u32;
const GICD_ISENABLER1: *mut u32 = (GICD_BASE + 0x104) as *mut u32;
const GICD_IPRIORITYR: *mut u8  = (GICD_BASE + 0x400) as *mut u8;
const GICD_ITARGETSR:  *mut u8  = (GICD_BASE + 0x800) as *mut u8;

const GICR_BASE: usize = 0xf0100000;
const GICR_WAKER: *mut u32 = (GICR_BASE + 0x014) as *mut u32;

pub fn gic_init() {
    unsafe {
        // 1. 唤醒 Redistributor：清除 ProcessorSleep（bit 1），等待 ChildrenAsleep（bit 2）清零
        let waker = GICR_WAKER.read_volatile();
        GICR_WAKER.write_volatile(waker & !0x2);
        while GICR_WAKER.read_volatile() & 0x4 != 0 {}

        // 2. 使能 Distributor Group 1（bit 0 = EnableGrp1）
        GICD_CTLR.write_volatile(0x1);

        // 3. INTID 33：设置优先级、路由到 CPU 0、使能
        GICD_IPRIORITYR.add(33).write_volatile(0xA0);
        GICD_ITARGETSR.add(33).write_volatile(0x01);
        GICD_ISENABLER1.write_volatile(1 << (33 % 32));

        // 4. CPU Interface：优先级掩码 + Group 0 使能
        // ICC_PMR = 0xFF：允许所有优先级（默认 0 会屏蔽一切）
        core::arch::asm!("mcr p15, 0, {0}, c4, c6, 0", in(reg) 0xFFu32);
        // ICC_IGRPEN0 = 1：使能 CPU 侧 Group 0 中断投递（默认 0 不投递）
        core::arch::asm!("mcr p15, 0, {0}, c12, c12, 6", in(reg) 1u32);
    }
}

/// 读取 Group 0 IAR（返回 INTID），同时把中断标记为 Active
pub fn gic_ack0() -> u32 {
    let intid: u32;
    unsafe {
        core::arch::asm!("mrc p15, 0, {0}, c12, c8, 0", out(reg) intid);
    }
    intid
}

/// 写 Group 0 EOIR，通知 GIC 中断处理完成
pub fn gic_eoi0(intid: u32) {
    unsafe {
        core::arch::asm!("mcr p15, 0, {0}, c12, c8, 1", in(reg) intid);
    }
}
```

**步骤说明：**

| 步骤 | 作用 |
| --- | --- |
| 唤醒 GICR_WAKER | Redistributor 上电睡眠，必须先醒来才能接收中断 |
| GICD_CTLR = 0x1 | 使能 Distributor Group 0（bit 0 = EnableGrp0，INTID 33 在 Group 0） |
| GICD_IPRIORITYR[33] | 设置 INTID 33 的优先级（数值越小优先级越高） |
| GICD_ITARGETSR[33] | 路由到 CPU 0（GICv2 兼容模式，bit N = CPU N） |
| GICD_ISENABLER1 bit 1 | 使能 INTID 33（bit 位置 = 33 % 32 = 1） |
| ICC_PMR = 0xFF | 优先级掩码：允许所有优先级的中断通过（默认值 0 屏蔽一切） |
| ICC_IGRPEN0 = 1 | CPU Interface 侧 Group 0 使能（默认 0，不配置则 FIQ 永远不到达 CPU） |

> **注意（QEMU Cortex-R52 上的 ICC_* 访问）：**
>
> - **ICC_SRE**（c12, c12, 5）在 QEMU Cortex-R52 上写入会触发未定义指令异常——QEMU 默认已将其永久置 1（系统寄存器接口始终开启），不需要也不允许软件再设置。
> - **ICC_PMR**（c4, c6, 0）和 **ICC_IGRPEN0**（c12, c12, 6）**必须手动配置**：QEMU 的默认值均为 0，ICC_PMR=0 会屏蔽所有中断（优先级条件 `prio < 0` 永不成立），ICC_IGRPEN0=0 则关闭 CPU Interface 侧的 Group 0 投递。两者缺一不可，否则 FIQ 永远无法到达 CPU。

## 新建 src/timer.rs

```rust
pub const TIMER1INTCLR: *mut u32 = (0xe0101000usize + 0x00C) as *mut u32;

pub fn timer_init() {
    unsafe {
        let load    = (0xe0101000usize + 0x000) as *mut u32;
        let control = (0xe0101000usize + 0x008) as *mut u32;

        // 50 MHz 时钟，100 ms 周期 = 50_000_000 × 0.1 = 5_000_000
        load.write_volatile(5_000_000);
        // 0xE8 = TimerEn | TimerMode | IntEnable | TimerSize（32 位）
        control.write_volatile(0xE8);
    }
}
```

## 修改 src/main.rs

在文件顶部添加两个模块声明：

```rust
mod gic;
mod timer;
```

在 `rust_main` 里调用初始化，最后才开中断：

```rust
#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart::uart_init();
    println!("Hello from RTOS!");

    gic::gic_init();
    timer::timer_init();

    println!("Timer started. Waiting for FIQ...");
    unsafe { core::arch::asm!("cpsie if"); }  // 最后才开 IRQ + FIQ

    loop {}
}
```

> **注意：** `cpsie if` 必须在 GIC 和 Timer **都初始化完毕后**才执行。过早打开中断，可能在 FIQ handler 里读到未初始化的变量。

添加 `TICK_COUNT` 全局变量和完整的 `rust_fiq_handler`（替换原来的空函数）：

```rust
static mut TICK_COUNT: u32 = 0;

#[unsafe(no_mangle)]
pub extern "C" fn rust_fiq_handler() {
    let intid = gic::gic_ack0();  // 应答，把中断标为 Active

    if intid == 33 {
        unsafe {
            // 清除 Timer 1 中断标志（必须在 EOI 之前）
            timer::TIMER1INTCLR.write_volatile(1);

            TICK_COUNT += 1;
            if TICK_COUNT % 10 == 0 {
                println!("Tick: {}", TICK_COUNT as u32);
            }
        }
    }

    gic::gic_eoi0(intid);  // 通知 GIC 处理完毕
}
```

`rust_irq_handler` 保持上一章的**空函数**不变——本章 Timer 中断走 FIQ 路径，IRQ handler 在这章不会被触发。

# 验证方法

```bash
cargo build
qemu-system-arm \
  -machine mps3-an536 \
  -nographic \
  -device loader,file=target/armv8r-none-eabihf/debug/rtos
```

预期输出（每约 1 秒一行）：

```text
Hello from RTOS!
Initializing GIC and Timer...
Timer started. Waiting for interrupts...
Tick: 10
Tick: 20
Tick: 30
```

按 **Ctrl+A 然后 X** 退出。

> **注意：** 如果只看到前三行但没有 Tick 输出，用以下命令确认 FIQ 是否被触发：
>
> ```bash
> qemu-system-arm -machine mps3-an536 -nographic \
>   -device loader,file=target/armv8r-none-eabihf/debug/rtos \
>   -d in_asm 2>&1 | grep "fiq_handler" | head -5
> ```

# 练习题

```quiz single
Q: GICv3 中，Distributor（GICD）和 CPU Interface（ICC_*）的主要区别是什么？
- GICD 负责高优先级中断，ICC 负责低优先级中断
+ GICD 是全局共享的内存映射寄存器，管理 SPI 的路由和使能；ICC 是每个 CPU 核私有的系统寄存器，负责中断应答和结束通知
- GICD 在 Flash 里，ICC 在 RAM 里
- GICD 处理外部中断，ICC 处理软件中断
E: Distributor 是整个系统共享的一块硬件（内存映射），管理所有 SPI 的全局配置（使能、优先级、路由目标）。CPU Interface 是每个核私有的接口，通过系统寄存器（MRC/MCR）访问，提供应答（IAR）和结束通知（EOIR）功能。
```

```quiz single
Q: FIQ handler 中，为什么必须先读 ICC_IAR0（应答），再清除外设中断标志，最后写 ICC_EOIR0？
- 只是惯例，顺序不影响结果
+ 先读 IAR 让 GIC 把该中断标为"处理中"防止重复分发；清外设标志须在 EOI 前，否则 GIC 完成 EOI 后立即收到新中断；最后 EOI 恢复为 Inactive 状态
- 因为 IAR 和 EOIR 是同一个寄存器，必须成对使用
- 因为编译器会对读写重排，必须按此顺序防止重排
E: 读 IAR 把中断标记为 Active，防止 GIC 重复分发。在 EOI 之前清除外设标志，否则 GIC 完成 EOI 后外设立刻再次拉高中断线，形成中断风暴。最后 EOI 把状态从 Active 恢复为 Inactive，允许下一次触发。
```

```quiz single
Q: GICD_ISENABLER[1] 寄存器（地址 GICD_BASE + 0x104）的 bit 1 对应哪个 INTID？
- INTID 1
- INTID 32
+ INTID 33
- INTID 64
E: GICD_ISENABLER 数组中，ISENABLER[n] 的 bit m 对应 INTID (32n + m)。ISENABLER[1] 的 bit 1 = INTID (32×1 + 1) = 33。这正好是 mps3-an536 上 Timer 1 的 INTID。
```

```quiz single
Q: 为什么 cpsie if（同时开 IRQ 和 FIQ）要放在 GIC 和 Timer 都初始化完之后？
- 因为 cpsie 指令本身依赖 GIC 才能执行
+ 防止在外设配置未完成时收到中断，handler 会访问未初始化的硬件和变量，导致不可预期的行为
- 因为 ARM 架构规定 cpsie 必须是 rust_main 的最后一条指令
- 因为 Timer 在 cpsie 之前硬件上不产生中断信号
E: 如果先开中断，定时器配置未完成时可能就触发了 FIQ，fiq_handler 会访问未初始化的全局变量和寄存器，结果随机出错。先完成全部硬件初始化，最后打开中断接收，是嵌入式初始化的标准做法。
```
