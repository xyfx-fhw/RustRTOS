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