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