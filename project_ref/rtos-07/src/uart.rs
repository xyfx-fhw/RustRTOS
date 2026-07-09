use core::fmt::{self, Write};

const UART0_BASE: usize = 0xe7c00000;

const UART0_DATA:  *mut   u32 = (UART0_BASE + 0x00) as *mut   u32;
const UART0_STATE: *const u32 = (UART0_BASE + 0x04) as *const u32;
const UART0_CTRL:  *mut   u32 = (UART0_BASE + 0x08) as *mut   u32;

pub fn uart_init() {
    unsafe {
        UART0_CTRL.write_volatile(0b01);
    }
}

pub fn uart_putc(byte: u8) {
    unsafe {
        while (UART0_STATE.read_volatile() & 0b10) != 0 {}
        UART0_DATA.write_volatile(byte as u32);
    }
}

pub fn uart_puts(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}

pub struct UartWriter;

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        uart_puts(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let mut w = $crate::uart::UartWriter;
        core::fmt::write(&mut w, core::format_args!($($arg)*)).ok();
    }};
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {{
        $crate::print!($($arg)*);
        $crate::print!("\n");
    }};
}