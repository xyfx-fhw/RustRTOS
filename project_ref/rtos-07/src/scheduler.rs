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

struct Scheduler {
    slots:   [Option<TaskEntry>; MAX_TASKS],
    count:   usize,     // 已注册的任务数（含 idle）
    current: usize,     // 当前正在运行的任务下标
}

static mut SCHED: Scheduler = Scheduler { ... };
static mut CURRENT_TASK: *mut task::Task = core::ptr::null_mut();

unsafe extern "C" fn task_entry_wrapper() -> ! {
    let entry_fn: fn();
    // r0 = 初始帧里的参数（entry 函数指针）
    core::arch::asm!("mov {0}, r0", out(reg) entry_fn, options(nomem, nostack));
    entry_fn();      // 调用用户函数（fn() 有限任务或 fn()->! 无限循环）
    task_exit();     // 有限任务返回后自动退出
}

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