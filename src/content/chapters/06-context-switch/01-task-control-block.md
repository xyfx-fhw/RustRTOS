---
title: "任务控制块设计"
description: "设计 TCB 数据结构，初始化任务私有栈，为 context switch 做好数据准备"
difficulty: intermediate
estimatedTime: 45
keywords: ["TCB", "任务控制块", "任务栈", "自引用结构", "栈初始化", "伪造上下文"]
---

# 本章目标

- 理解为什么每个任务必须有独立的私有栈
- 设计最小 TCB 结构体，明确 `stack_ptr` 字段的作用
- 理解"自引用结构"问题，以及为什么在裸机上用全局静态数组做任务栈
- 手动构造新任务的初始寄存器帧（伪造"第一次被中断"的现场）

# 前置知识

## 已完成的章节

`06-context-switch/00-index.md` 已阅读，清楚了 TCB 的概念、任务私有栈的必要性，以及 context switch 的三步流程。

## 了解 raw pointer

Rust 的裸指针（`*mut T` / `*const T`）与普通引用不同：不受借用检查器管辖，读写需要在 `unsafe` 块里操作。在裸机手动管理内存时，我们必须使用裸指针——没有运行时来帮我们跟踪生命周期。

## 了解 static mut

`static mut` 是进程生命周期内地址永远不变的全局变量。在单核裸机上，访问它需要 `unsafe`，但只要保证同一时刻只有一段代码访问，就是安全的。

# 为什么每个任务需要独立的栈

在只有一个任务的系统里，程序只有一个调用栈。函数调用时把局部变量、返回地址压栈；函数返回时把它们弹出。整个程序共享一个从高地址向低地址增长的栈空间。

如果多个任务共享同一个栈，会发生什么？

```text
任务 A 正在执行 compute()，栈上：
  [... compute 的局部变量 ...]
  [compute 的返回地址     ]
  ← sp

切换到任务 B，B 调用 display()，push 了自己的栈帧：
  [... compute 的局部变量 ...]   ← 被破坏！
  [display 的局部变量    ]
  [display 的返回地址    ]
  ← sp

切回任务 A，sp 不对，compute 的局部变量全部损坏，立刻崩溃。
```

**结论：每个任务必须有自己的私有栈，互不干扰。** 切换任务时，把 sp 也一起切换到目标任务的私有栈，这样每个任务都活在自己独立的栈空间里。

# TCB 数据结构设计

TCB 中最核心的字段只有一个：**任务被暂停时的栈指针**。

```rust
pub struct Task {
    /// 任务暂停时保存的栈顶指针。
    /// context switch 通过此指针找到任务的寄存器快照。
    pub stack_ptr: *mut u32,
}
```

仅此而已——目前不需要优先级、状态标志等字段（第 07 章调度器再加）。

> **为什么用 `*mut u32` 而不是 `&mut u32`？**
> 裸指针跨越了借用检查器的管辖边界。多个任务的 TCB 会被保存在全局数组里，同时只有一个任务"持有" sp，但借用检查器无法理解这种逻辑。裸机内核的内存管理必须用裸指针，由程序员自行保证正确性。

# 为什么任务栈不放在 TCB 里

你可能会想把栈缓冲区直接放进 TCB，省去额外的全局数组：

```rust
// 看起来很直观，但有问题
pub struct Task {
    pub stack_ptr: *mut u32,
    stack: [u32; 512],  // ← 和 stack_ptr 共存在同一个 struct 里
}
```

问题在于：`stack_ptr` 是一个指向 `stack` 数组内部某个槽的指针。一旦 `Task` 结构体被移动（赋值给另一个变量、放入数组、传给函数），`stack` 数组的地址就变了，但 `stack_ptr` 还指着旧地址——变成了**悬挂指针（dangling pointer）**，访问它会崩溃或产生随机错误。

这在 Rust 中叫做**自引用结构（self-referential struct）**问题，编译器会拒绝让你创建这样的结构（通过移动）。

**解决方案：把任务栈放在独立的全局静态数组里。** `static mut` 的地址在程序运行期间永远不会改变，`stack_ptr` 指向它里面永远有效。

```rust
pub const STACK_SIZE: usize = 512; // 2KB（512 × 4 字节）
pub const MAX_TASKS: usize = 4;

static mut TASK_STACKS: [[u32; STACK_SIZE]; MAX_TASKS] = [[0; STACK_SIZE]; MAX_TASKS];
static mut TASK_COUNT: usize = 0;
```

# 初始化任务栈：伪造第一次"现场"

这是整个 context switch 实现里最容易出错的部分，需要仔细理解。

## 问题：新任务从未运行过

下一章（`02-context-save-restore.md`）的恢复代码会做这件事：

```asm
ldr  sp, [r1]           // 从 TCB 里取出任务的 stack_ptr，切换 sp
pop  {{r0-r12, lr}}     // 从栈上弹出 14 个寄存器
bx   lr                 // 跳到 lr 保存的地址继续执行
```

这段代码假设任务的栈上**已经有一个完整的寄存器帧**（14 个 u32 值），等着被 `pop` 出来。

但新任务从未运行过，它的栈是空的！如果直接 `pop`，会从栈上读到随机数据，然后跳到随机地址——立刻崩溃。

## 解决方案：手动构造初始寄存器帧

创建任务时，我们在它的私有栈上**手动放置一个假的寄存器帧**，就好像这个任务已经运行过一次、被 `push` 暂停过一样。

关键：`pop {r0-r12, lr}` 后接 `bx lr` 会跳到 lr 的值。

所以初始帧必须满足：

- **lr = 任务入口函数的地址**（这样 `bx lr` 就会跳到任务的 `fn` 开始执行）
- r0–r12 = 0（新任务启动时不依赖这些寄存器的初始值，填 0 即可）

## 帧在内存里的布局

`pop {r0-r12, lr}` 按从低地址到高地址的顺序读取（ARM 的 LDMIA/POP 规则：低编号寄存器对应低地址）：

```text
低地址
  sp  → [r0  = 0      ]
        [r1  = 0      ]
        [r2  = 0      ]
        [r3  = 0      ]
        [r4  = 0      ]
        [r5  = 0      ]
        [r6  = 0      ]
        [r7  = 0      ]
        [r8  = 0      ]
        [r9  = 0      ]
        [r10 = 0      ]
        [r11 = 0      ]
        [r12 = 0      ]
        [lr  = entry  ]  ← 入口函数地址
高地址
```

共 14 个槽（14 × 4 = 56 字节）。**sp 指向帧的最低地址（r0 的位置）。**

用数组索引表示（`STACK_SIZE = 512`）：

```text
TASK_STACKS[id][  0 ]  ← 低地址（栈底，永远不会用到）
TASK_STACKS[id][498]  ← r0 的位置（sp 保存在 TCB 里的值）
TASK_STACKS[id][499]  ← r1
...
TASK_STACKS[id][510]  ← r12
TASK_STACKS[id][511]  ← lr = 任务入口函数地址（STACK_SIZE - 1）
```

`sp` 保存的值 = `&TASK_STACKS[id][STACK_SIZE - 14]`（帧的最低地址）。

## 实现 create_task

```rust
pub fn create_task(entry: fn() -> !) -> Task {
    unsafe {
        // 分配一个任务 ID，获取对应的栈空间
        let id = TASK_COUNT;
        TASK_COUNT += 1;

        let stack = &mut TASK_STACKS[id];

        // 在栈顶构造初始寄存器帧（14 个槽）
        // r0-r12 已经是 0（TASK_STACKS 是全局 static，零初始化）
        stack[STACK_SIZE - 1] = entry as u32;  // lr = 入口函数地址

        // sp 指向帧的最低地址（r0 的位置）
        Task {
            stack_ptr: &mut stack[STACK_SIZE - 14] as *mut u32,
        }
    }
}
```

逐行解释：

- `TASK_COUNT += 1`：分配任务 ID，确保每个任务用不同的栈空间
- `stack[STACK_SIZE - 1] = entry as u32`：把入口函数地址转换为 `u32`，写入 lr 的位置（`fn() -> !` 是一个函数指针，转换为 u32 就是它在 Flash 里的地址）
- `&mut stack[STACK_SIZE - 14] as *mut u32`：取 r0 位置的可变裸指针，作为初始 `stack_ptr`

> **注意：** `fn() -> !` 之所以能转成 `u32`，是因为 AArch32 地址本来就是 32 位的。这在 64 位平台上会报错，但在本项目（`armv8r-none-eabihf`）上完全正确。

# 完整的 src/task.rs

```rust
pub const STACK_SIZE: usize = 512; // 2KB（512 × 4 字节）每个任务
pub const MAX_TASKS: usize = 4;

#[repr(C)]
pub struct Task {
    /// 任务暂停时保存的栈顶指针
    pub stack_ptr: *mut u32,
}

static mut TASK_STACKS: [[u32; STACK_SIZE]; MAX_TASKS] = [[0; STACK_SIZE]; MAX_TASKS];
static mut TASK_COUNT: usize = 0;

/// 创建一个新任务，返回初始化好的 TCB
pub fn create_task(entry: fn() -> !) -> Task {
    unsafe {
        let id = TASK_COUNT;
        TASK_COUNT += 1;

        let stack = &mut TASK_STACKS[id];
        // lr = 入口函数地址；r0-r12 已为 0
        stack[STACK_SIZE - 1] = entry as u32;

        Task {
            stack_ptr: &mut stack[STACK_SIZE - 14] as *mut u32,
        }
    }
}
```

目前 `Task` 没有实现 `Send`（因为包含裸指针）。单核裸机不涉及线程，这不是问题。

# 验证方法

本节不涉及可执行验证，创建的 `Task` 需要配合下一节的 context switch 汇编才能运行。

可以先 `cargo build` 确认代码编译通过：

```bash
cargo build
```

如果编译成功，说明 `Task` 结构和 `create_task` 的类型都正确。运行效果在 `02-context-save-restore.md` 完成后验证。

# 练习题

```quiz single
Q: 为什么多个任务不能共享同一个调用栈？
+ 因为任务 B 的函数调用会把栈帧压在任务 A 的数据上方，切回 A 时局部变量和返回地址都被破坏
- 因为 Rust 规定每个线程必须有独立栈
- 因为 ARM 硬件只支持一个 sp 寄存器
- 因为共享栈会导致栈溢出
E: 栈只是一段连续内存。任务 B 运行时的 push/pop 会覆盖任务 A 留在栈上的数据（局部变量、返回地址）。切回 A 时 sp 指向错误的位置，读到的都是 B 写进去的垃圾值，必然崩溃。
```

```quiz single
Q: 为什么不把栈缓冲区 [u32; STACK_SIZE] 直接放在 TCB struct 里面？
- 因为数组太大，放在 struct 里会编译失败
- 因为 Rust 不允许 struct 包含数组字段
+ 因为 stack_ptr 指向 stack 数组内部，一旦 struct 被移动，stack 的地址改变，stack_ptr 就变成悬挂指针
- 因为 static mut 数组比 struct 字段访问更快
E: 这是 Rust 中著名的"自引用结构"问题：struct 内部有一个字段的指针指向另一个字段。struct 被赋值或传递时会被复制到新地址，但指针还指着旧地址，成为悬挂指针。把栈放在 static mut 全局数组里，地址永远不变，避免了这个问题。
```

```quiz single
Q: 新任务初始帧里，lr 必须设为什么值，原因是什么？
- lr 必须设为 0，表示没有返回地址
- lr 必须设为 stack 的起始地址
+ lr 必须设为任务入口函数的地址，因为 context switch 恢复后执行 bx lr，会跳到这个地址开始执行任务
- lr 必须设为 rust_main 的地址
E: context switch 的恢复路径是 pop {r0-r12, lr} 然后 bx lr。对于新任务，这是它第一次"被恢复"，pop 完成后 lr 里装的就是任务的起点。bx lr 跳过去，任务就开始运行了。若 lr = 0，bx lr 会跳到地址 0（向量表），立刻触发 undef_handler。
```

```quiz single
Q: pop {r0-r12, lr} 时，哪个寄存器对应栈上的最低地址（sp 指向的位置）？
- lr，因为它最重要
- r12，因为它编号最高
- 顺序随机，取决于编译器
+ r0，因为 ARM POP 规则：低编号寄存器对应低地址
E: ARM 的 LDM（Load Multiple）指令按照"低编号寄存器对应低地址"的规则读取内存。POP {r0-r12, lr} 从 sp 开始依次读 r0（最低地址）、r1、...、r12、lr（最高地址），然后 sp 增加 14×4=56。所以 create_task 里 sp 保存在 r0 对应的位置，即 &stack[STACK_SIZE - 14]。
```
