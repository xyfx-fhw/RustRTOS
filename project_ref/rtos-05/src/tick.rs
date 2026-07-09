pub const TICK_PERIOD_MS: u32 = 100;

static mut TICK_COUNT: u32 = 0;

pub fn tick_increment() {
    unsafe { TICK_COUNT += 1; }
}

pub fn get_ticks() -> u32 {
    unsafe { (&raw const TICK_COUNT).read_volatile() }
}

#[allow(dead_code)]
pub fn delay_ticks(n: u32) {
    let start = get_ticks();
    while get_ticks().wrapping_sub(start) < n {}
}

#[allow(dead_code)]
pub fn delay_ms(ms: u32) {
    delay_ticks(ms / TICK_PERIOD_MS);
}