// /// 每 tick 100ms（5_000_000 cycles @ 50MHz）
// pub const TICK_PERIOD_MS: u32 = 100;

static mut TICK_COUNT: u32 = 0;

/// 由 FIQ handler 调用，每次定时器中断递增一次
pub fn tick_increment() {
    unsafe { TICK_COUNT += 1; }
}

// /// 读取当前 tick 计数
// ///
// /// 必须用 read_volatile，否则编译器会在主线程紧循环中
// /// 把读取优化为寄存器缓存，永远看不到 FIQ 写入的新值。
// pub fn get_ticks() -> u32 {
//     unsafe { (&raw const TICK_COUNT).read_volatile() }
// }

// /// 等待至少 n 个 tick 后返回（粗粒度延迟，每 tick = 100ms）
// pub fn delay_ticks(n: u32) {
//     let start = get_ticks();
//     // 处理 u32 溢出回绕（运行约 13 年后会溢出）
//     while get_ticks().wrapping_sub(start) < n {}
// }