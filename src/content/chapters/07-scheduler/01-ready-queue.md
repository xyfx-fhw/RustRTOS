---
title: "就绪队列与调度器核心"
description: "实现优先级 Round-Robin 调度器：任务状态机、优先级选择、sleep_ticks 真睡眠与空闲任务"
difficulty: intermediate
estimatedTime: 50
keywords: ["就绪队列", "Round-Robin", "优先级", "任务状态", "sleep_ticks", "Sleeping", "空闲任务", "Zombie"]
---

# 本章目标

- 设计 `TaskState` 枚举（Ready / Running / Sleeping / Zombie），理解完整任务生命周期
- 实现优先级就绪队列：`add_task(entry, priority)`，调度器始终选最高优先级 Ready 任务
- 实现 `sleep_ticks(n)`：让任务真正让出 CPU n 个 tick，期间调度其他任务
- 实现自动空闲任务（idle_entry + wfi）：所有任务睡眠时 CPU 不空转
- 理解 `scheduler_tick` 中唤醒检查与立即抢占的顺序关系

## 前置知识

### 已完成的章节

`07-scheduler/00-index.md` 已阅读，理解调度器整体设计。`06-context-switch` 的 `context_switch` 和 `start_first_task` 是本节基础。

### 了解静态数组替代动态分配

裸机无堆分配，所有数据结构必须编译期确定大小。本节用 `[Option<TaskEntry>; MAX_TASKS]` 代替 `Vec`。

# 任务状态机

```text
add_task() → [Ready]
                ↓  scheduler 选中
           [Running] ←────────────────┐
                ↓ sleep_ticks(n)      │ 时间片到期 / yield_now()
           [Sleeping]                 │
                ↓ tick 到期，FIQ 唤醒 ┘
           [Ready]
                ↓ task_exit() 或 fn() 返回
           [Zombie]（永远不再调度）
```

**Ready**：等待被调度器选中。  
**Running**：当前在 CPU 上执行（同一时刻只有一个）。  
**Sleeping**：主动调用 `sleep_ticks(n)` 后进入，等待 `sleep_until` tick 到期才能重新参与调度。  
**Zombie**：任务退出，永不再运行。

> **注意：** `delay_ticks(n)` 是**忙等**（不停轮询 tick），任务仍处于 Running 状态，不会让出 CPU。`sleep_ticks(n)` 才是真正的睡眠——立刻交出 CPU，n tick 后由 FIQ 自动唤醒。

# 数据结构设计

## src/scheduler.rs 结构

```rust
pub const MAX_TASKS:  usize = 8;
pub const TIME_SLICE: u32   = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum TaskState { Ready, Running, Sleeping, Zombie }

struct TaskEntry {
    task:        task::Task,
    state:       TaskState,
    remaining:   u32,        // 本轮剩余时间片 tick 数
    priority:    u8,         // 优先级：0 = 最低（idle），255 = 最高
    sleep_until: u32,        // 仅 Sleeping 时有效：tick 到此值时唤醒
}
```

`sleep_until` 使用 tick 绝对值，支持 wrapping 比较（处理 u32 溢出回绕）。

# 实现步骤

## 步骤一：注册任务 — add_task()

```rust
pub fn add_task(entry: fn(), priority: u8) {
    unsafe {
        let sched = &mut SCHED;
        let id = sched.count;
        // MAX_TASKS - 1：最后一个槽保留给 start() 自动添加的 idle 任务
        assert!(id < MAX_TASKS - 1, "too many tasks");
        sched.count += 1;
        let t = task::create_task_with_arg(
            task_entry_wrapper as *const () as usize,
            entry as *const () as usize,
        );
        sched.slots[id] = Some(TaskEntry {
            task: t,
            state: TaskState::Ready,
            remaining: TIME_SLICE,
            priority,
            sleep_until: 0,
        });
    }
}
```

## 步骤二：任务入口包装器

```rust
unsafe extern "C" fn task_entry_wrapper() -> ! {
    let entry_fn: fn();
    // r0 = 初始帧里的参数（entry 函数指针）
    core::arch::asm!("mov {0}, r0", out(reg) entry_fn, options(nomem, nostack));
    entry_fn();      // 调用用户函数（fn() 有限任务或 fn()->! 无限循环）
    task_exit();     // 有限任务返回后自动退出
}
```

## 步骤三：选最高优先级任务 — select_next()

```rust
fn select_next(sched: &mut Scheduler) -> Option<usize> {
    let n = sched.count;
    let cur = sched.current;

    // 找最高优先级
    let max_prio = (0..n)
        .filter_map(|i| sched.slots[i].as_ref())
        .filter(|e| e.state == TaskState::Ready)
        .map(|e| e.priority)
        .max()?;

    // 在最高优先级中，从 current+1 开始 Round-Robin（避免同优先级饥饿）
    for i in 1..=n {
        let idx = (cur + i) % n;
        if let Some(e) = &sched.slots[idx] {
            if e.state == TaskState::Ready && e.priority == max_prio {
                return Some(idx);
            }
        }
    }
    None
}
```

## 步骤四：sleep_ticks — 真正的睡眠

```rust
pub fn sleep_ticks(n: u32) {
    if n == 0 { return; }
    unsafe {
        let sched = &mut SCHED;
        let cur = sched.current;
        if let Some(e) = &mut sched.slots[cur] {
            e.state = TaskState::Sleeping;
            // wrapping_add 处理 tick 溢出回绕（约 49 天后发生）
            e.sleep_until = tick::get_ticks().wrapping_add(n);
            e.remaining = TIME_SLICE; // 睡醒后重置时间片
        }
        // 立刻切换到下一个 Ready 任务（至少有 idle 任务兜底）
        if let Some(next_idx) = select_next(sched) {
            let curr_ptr = CURRENT_TASK;
            let next = sched.slots[next_idx].as_mut().unwrap();
            next.state = TaskState::Running;
            sched.current = next_idx;
            CURRENT_TASK = &mut next.task as *mut task::Task;
            task::context_switch(curr_ptr, CURRENT_TASK);
            // ← 从这里返回时，本任务已被 scheduler_tick 唤醒并恢复
        }
    }
}
```

`sleep_ticks` 执行后，任务"消失"在 `context_switch` 调用里，等到 n tick 后由 FIQ 唤醒，context_switch 才"返回"，任务从这行继续执行。

## 步骤五：启动调度器 + 自动空闲任务 — start()

```rust
/// 空闲任务：所有任务都在 Sleeping 时自动运行，CPU 进入低功耗等待
unsafe extern "C" fn idle_entry() -> ! {
    loop { core::arch::asm!("wfi"); }
}

pub fn start() -> ! {
    unsafe {
        let sched = &mut SCHED;
        // 在所有用户任务之后，自动添加 idle 任务（priority=0，永远 Ready）
        let idle_id = sched.count;
        sched.count += 1;
        let idle = task::create_task_with_arg(idle_entry as *const () as usize, 0);
        sched.slots[idle_id] = Some(TaskEntry {
            task: idle, state: TaskState::Ready,
            remaining: TIME_SLICE, priority: 0, sleep_until: 0,
        });

        // 从最高优先级任务开始
        if let Some(idx) = select_next(sched) {
            sched.slots[idx].as_mut().unwrap().state = TaskState::Running;
            sched.current = idx;
            CURRENT_TASK = &mut sched.slots[idx].as_mut().unwrap().task;
            task::start_first_task(CURRENT_TASK);
        }
        panic!("no tasks");
    }
}
```

> **注意：** 用户最多可注册 `MAX_TASKS - 1 = 7` 个任务（第 8 个槽留给 idle）。

## 步骤六：scheduler_tick — 唤醒 + 优先级调度

这是调度器的核心，由 FIQ handler 每 tick 调用一次：

```rust
#[unsafe(no_mangle)]
pub extern "C" fn scheduler_tick() {
    unsafe {
        // 1. tick++ + ACK + 清中断 + EOI
        tick::tick_increment();
        gic::gic_ack0();
        timer::TIMER1INTCLR.write_volatile(1);
        gic::gic_eoi0(33);

        let sched = &mut SCHED;
        let cur_tick = tick::get_ticks();

        // ① 唤醒所有到期的睡眠任务（必须在时间片检查之前！）
        //    wrapping_sub < 0x8000_0000 等价于 cur_tick >= sleep_until（含溢出回绕）
        for i in 0..sched.count {
            if let Some(e) = &mut sched.slots[i] {
                if e.state == TaskState::Sleeping
                    && cur_tick.wrapping_sub(e.sleep_until) < 0x8000_0000
                {
                    e.state = TaskState::Ready;
                }
            }
        }

        // ② 递减时间片
        let cur = sched.current;
        let (slice_expired, cur_prio) = {
            let e = sched.slots[cur].as_mut().unwrap();
            if e.remaining > 0 { e.remaining -= 1; }
            (e.remaining == 0, e.priority)
        };

        // ③ 立即抢占检查：有更高优先级任务刚被唤醒？
        let higher_ready = (0..sched.count).any(|i| {
            sched.slots[i].as_ref()
                .map(|e| e.state == TaskState::Ready && e.priority > cur_prio)
                .unwrap_or(false)
        });

        // 既没有到期也没有高优先级唤醒 → 继续当前任务
        if !slice_expired && !higher_ready { return; }

        // ④ 当前任务退回 Ready，时间片到期时重置
        if let Some(e) = &mut sched.slots[cur] {
            if e.state == TaskState::Running { e.state = TaskState::Ready; }
            if slice_expired { e.remaining = TIME_SLICE; }
        }

        // ⑤ 选最高优先级 Ready 任务，更新 CURRENT_TASK
        if let Some(next_idx) = select_next(sched) {
            let next = sched.slots[next_idx].as_mut().unwrap();
            next.state = TaskState::Running;
            sched.current = next_idx;
            CURRENT_TASK = &mut next.task as *mut task::Task;
        }
    }
}
```

**为什么唤醒检查必须在时间片检查之前？**  
如果先做时间片检查，再唤醒：此刻更高优先级任务还是 Sleeping，`higher_ready` 为 false，当前任务继续运行一个 tick。下一个 tick 才能切换。把唤醒提前，可以在同一 tick 内"唤醒 → 立刻抢占"，响应延迟减少 1 个 tick。

## 步骤七：任务退出 — task_exit()

```rust
pub fn task_exit() -> ! {
    unsafe {
        let sched = &mut SCHED;
        if let Some(e) = &mut sched.slots[sched.current] {
            e.state = TaskState::Zombie;
            e.remaining = 0;
        }
        // 找下一个 Ready 任务直接跳入（不保存当前上下文，Zombie 不需要恢复）
        let n = sched.count;
        let start = sched.current + 1;
        for i in 0..n {
            let idx = (start + i) % n;
            if let Some(entry) = &mut sched.slots[idx] {
                if entry.state == TaskState::Ready {
                    entry.state = TaskState::Running;
                    sched.current = idx;
                    CURRENT_TASK = &mut entry.task as *mut task::Task;
                    task::start_first_task(CURRENT_TASK);
                }
            }
        }
        // 理论上不会到这里（至少有 idle 任务）
        loop { core::arch::asm!("wfi"); }
    }
}
```

# 验证方法

完成 `02-preemption.md` 后统一验证。以下是验证周期任务的示例：

```rust
fn task_10ms() {
    loop {
        println!("[10ms] tick={}", tick::get_ticks());
        scheduler::sleep_ticks(10);  // 真正让出 CPU
    }
}

fn task_bg() {
    loop {
        println!("[bg] tick={}", tick::get_ticks());
        scheduler::sleep_ticks(3);
    }
}

// main:
scheduler::add_task(task_bg,   1);
scheduler::add_task(task_10ms, 3);
scheduler::start();
```

预期：10ms 任务精确每 10 tick 运行，期间 bg 任务填补空隙，idle 任务处理全部睡眠情形。

# 练习题

```quiz single
Q: sleep_ticks(10) 和 delay_ticks(10) 有什么本质区别？
+ sleep_ticks 将任务标记为 Sleeping 并立刻调用 context_switch 让出 CPU，10 tick 后由 FIQ 自动唤醒；delay_ticks 是忙等（循环读 get_ticks()），任务始终处于 Running 状态，独占 CPU
- sleep_ticks 更精确，delay_ticks 有误差
- sleep_ticks 需要中断支持，delay_ticks 不需要
- 两者功能相同，只是实现方式不同
E: 忙等和睡眠的核心区别是"是否让出 CPU"。delay_ticks 在等待期间不停执行指令，低优先级任务无法获得 CPU。sleep_ticks 将任务从调度池中"移除"（标记 Sleeping），调度器会选择其他 Ready 任务运行，等到 tick 到期才重新加入竞争。
```

```quiz single
Q: 为什么 start() 要自动添加一个 idle 任务（priority=0）？
+ 当所有用户任务都处于 Sleeping 状态时，select_next 必须能找到至少一个 Ready 任务，否则调度器会 panic；idle 任务永远 Ready，保证系统不会因为"没有可运行任务"而崩溃，同时用 wfi 指令让 CPU 进入低功耗等待
- 为了让调度器有初始任务可以运行
- 因为 ARM 处理器要求始终有任务在运行
- idle 任务负责清理 Zombie 任务的内存
E: 如果所有任务都在 Sleeping 而没有 idle，select_next 返回 None，此时代码路径会走到 panic 或 undefined behavior。idle 任务用 wfi（Wait For Interrupt）指令让处理器挂起直到下一个 FIQ，既保证了调度器的正确性，又不浪费 CPU 电量。
```

```quiz single
Q: scheduler_tick 里，为什么唤醒检查（步骤①）要在时间片递减（步骤②）之前？
+ 把唤醒提前，可以在同一个 tick 内"检测到更高优先级任务醒来 → 立刻抢占"，响应延迟减少 1 tick；若顺序反过来，当 tick 到期时高优先级任务还是 Sleeping，higher_ready 为 false，要多等一个 tick 才能切换
- 因为这是 ARM 的硬件要求
- 为了避免竞态条件
- 唤醒检查比时间片递减更快，所以放前面
E: 调度延迟是实时系统的关键指标。如果一个高优先级任务在 tick=T 到期，最理想是在 tick=T 就立刻得到 CPU。通过先做唤醒检查，再检测 higher_ready，可以在同一个 FIQ 处理周期内完成"唤醒→切换"，最坏响应延迟为 1 tick（100ms），而不是 2 tick。
```
