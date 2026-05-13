ENTRY(reset_handler)

MEMORY
{
    FLASH : ORIGIN = 0x00000000, LENGTH = 32K
    RAM   : ORIGIN = 0x10000000, LENGTH = 512K
}

SECTIONS
{
    .text :
    {
        KEEP(*(.text.reset_handler))
        *(.text .text.*)
        *(.rodata .rodata.*)
    } > FLASH

    .data :
    {
        _sdata = .;
        *(.data .data.*)
        _edata = .;
    } > RAM AT > FLASH

    _sidata = LOADADDR(.data);

    .bss (NOLOAD) :
    {
        _sbss = .;
        *(.bss .bss.*)
        *(COMMON)
        _ebss = .;
    } > RAM

    _stack_start = ORIGIN(RAM) + LENGTH(RAM);
}