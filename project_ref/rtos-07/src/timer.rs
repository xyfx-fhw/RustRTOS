const TIMER1_BASE: usize = 0xe0101000;
const TIMER1LOAD:    *mut u32 = (TIMER1_BASE + 0x000) as *mut u32;
const TIMER1CONTROL: *mut u32 = (TIMER1_BASE + 0x008) as *mut u32;
const TIMER1INTCLR:  *mut u32 = (TIMER1_BASE + 0x00C) as *mut u32;

pub fn timer_init() {
    unsafe {
        // 50 MHz 时钟，100 ms 周期 = 50_000_000 × 0.1 = 5_000_000
        TIMER1LOAD.write_volatile(5_000_000);
        // 0xE8 = TimerEn(1) | TimerMode(1) | IntEnable(1) | TimerSize=32bit(1)
        TIMER1CONTROL.write_volatile(0xE8);
    }
}

pub fn timer_clear_interrupt() {
    unsafe { TIMER1INTCLR.write_volatile(1); }
}