---
title: "内存布局与链接脚本"
description: "设计 Flash/RAM 内存布局，编写链接脚本控制程序各段的存放位置"
difficulty: intermediate
estimatedTime: 45
keywords: ["链接脚本", "linker script", "内存布局", "Flash", "RAM", "BSS", "data段", "text段"]
---

# 本章目标

- 理解嵌入式系统中 Flash 和 RAM 的作用和区别
- 了解 mps3-an536 的内存地图（各区域的起始地址和大小）
- 编写一份链接脚本，控制程序各部分的存放位置
- 理解链接脚本中各段（`.text`、`.data`、`.bss`）的含义

## 前置知识

### 已完成的章节

`01-environment-setup/01-project-setup.md` 已完成，项目目录下已有 `Cargo.toml` 和 `.cargo/config.toml`，其中 `rustflags = ["-Tlink.x"]` 指定了链接脚本的文件名。

# mps3-an536 的内存地图

| 区域 | 起始地址 | 大小 | 用途 |
| --- | --- | --- | --- |
| Flash（ATCM） | `0x00000000` | 32 KB | 存放程序代码和常量 |
| RAM（BRAM） | `0x10000000` | 512 KB | 存放运行时数据和栈 |

> **注意：** 这里的 `0x00000000` 就是为什么 Cortex-R52 上电后从那个地址开始执行——Flash 就在那里，CPU 从 Flash 里读出第一条指令运行。

# 什么是链接脚本

我们写的 Rust 代码编译后会产生很多"段"（section）：

- **`.text`**：机器指令（代码本体）
- **`.rodata`**：只读数据（字符串常量、`const` 变量等）
- **`.data`**：有初始值的全局变量
- **`.bss`**：初始值为零的全局变量

问题来了：**这些段该放到内存的哪个地址？** 编译器不知道目标硬件的内存布局，它不知道 Flash 在 `0x00000000`，也不知道 RAM 在 `0x20000000`。

链接脚本就是告诉链接器（linker）"请把 `.text` 放到这里，把 `.data` 放到那里"的配置文件。没有它，链接器就不知道该如何排列程序。


# 编写链接脚本

## 步骤一：声明内存区域（MEMORY）

链接脚本的第一部分是 `MEMORY` 块，告诉链接器这块板子有哪些内存、地址在哪、有多大：

```text
MEMORY
{
    FLASH : ORIGIN = 0x00000000, LENGTH = 2M
    RAM   : ORIGIN = 0x20000000, LENGTH = 2M
}
```

每行格式是 `名字 : ORIGIN = 起始地址, LENGTH = 大小`。名字可以自取，后面会用到。

## 步骤二：安排各个段（SECTIONS）

`SECTIONS` 块告诉链接器把哪些段放到哪块内存里：

```text
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
```

这里有几个需要解释的细节：

**`.text` 段** 放在 Flash。`KEEP(*(.text.reset_handler))` 表示强制把名为 `reset_handler` 的函数放在最前面——这样它就落在 `0x00000000`，CPU 上电后第一个执行的就是它。后面的 `*(.text .text.*)` 把其余代码追加进来。

**`.data` 段** 有两个地址。`> RAM AT > FLASH` 是关键：

- `AT > FLASH`：**加载地址（LMA）**，这段数据在 Flash 里存放，烧录时写入 Flash
- `> RAM`：**运行地址（VMA）**，程序运行时从 RAM 里读取

为什么要这样？因为有初始值的全局变量（比如 `static mut X: u32 = 42`）的初始值必须存在 Flash 里（断电不丢失），但运行时 CPU 需要读写它，而 Flash 不支持随意写入，所以必须在启动时把这段数据从 Flash 复制到 RAM。这就是 reset handler 第三步要做的事情。

**`_sidata`、`_sdata`、`_edata`** 是三个符号（相当于全局变量），分别记录：
- `_sidata`：`.data` 在 Flash 里的起始地址（复制的来源）
- `_sdata`：`.data` 在 RAM 里的起始地址（复制的目标）
- `_edata`：`.data` 在 RAM 里的结束地址（知道复制多少字节）

reset handler 的汇编代码会读取这三个地址来完成数据段的复制。

**`.bss` 段** 放在 RAM，标记了 `NOLOAD` 表示链接器不为它分配 Flash 空间——零值变量不需要存在 Flash 里，直接在启动时把 RAM 清零就行。`_sbss` 和 `_ebss` 告诉 reset handler 清零的范围。

**`_stack_start`** 设置在 RAM 的最高地址。栈是从高地址向低地址增长的，所以把栈顶放在 RAM 末尾，向下增长，堆数据从低地址向上，两者相向而行，用满为止。

## 步骤三：完整的 link.x 文件

把上面两步合并，最终文件如下：

```text
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
```

将这个文件保存到项目根目录，命名为 `link.x`。

# 验证方法

链接脚本本身无法独立运行，需要在下一篇文章编写 reset handler 和 Rust 入口后，通过 `cargo build` 一并验证。

本文完成后，项目根目录下应该有 `link.x` 文件：

```bash
ls -l link.x
```

预期输出：

```text
-rw-r--r--  1 ...  link.x
```

# 练习题

```quiz single
Q: 嵌入式系统中 Flash 和 RAM 最核心的区别是什么？
- Flash 比 RAM 速度更快
+ Flash 断电后数据不丢失，RAM 断电后数据全部清空
- Flash 可以随意读写，RAM 只能读不能写
- Flash 用于存运行时数据，RAM 用于存代码
E: Flash 是非易失性存储器（断电不丢失），用来存放代码和常量。RAM 是易失性存储器（断电清空），速度快，用来存放运行时变量和栈。两者各司其职。
```

```quiz single
Q: 链接脚本中 `.data > RAM AT > FLASH` 的含义是什么？
- .data 段既放在 RAM 也放在 Flash，两份拷贝同步更新
- .data 段在 RAM 和 Flash 之间自动切换
+ .data 段存储在 Flash（加载地址），运行时在 RAM（运行地址），启动时需要从 Flash 复制到 RAM
- .data 段只存在 RAM，Flash 里没有备份
E: > RAM 指定运行地址（VMA），AT > FLASH 指定加载地址（LMA）。有初始值的全局变量的初始值保存在 Flash，启动时 reset handler 把它们复制到 RAM，之后程序从 RAM 读写这些变量。
```

```quiz single
Q: .bss 段为什么标记 NOLOAD，不占用 Flash 空间？
- 因为 .bss 段的内容太大，Flash 放不下
+ 因为 .bss 段存放初始值为零的变量，启动时直接清零 RAM 即可，不需要在 Flash 里保存任何数据
- 因为 QEMU 模拟器不支持从 Flash 加载 .bss 段
- 因为 .bss 段的变量在程序运行期间不会被修改
E: 初始值为零意味着 Flash 里不需要存任何内容。启动时只需把对应的 RAM 区域清零，比复制数据更简单，也节省了 Flash 空间。
```

```quiz single
Q: 链接脚本中 _stack_start = ORIGIN(RAM) + LENGTH(RAM) 把栈顶设在 RAM 末尾，原因是什么？
- 因为 RAM 末尾的地址最小，方便计算
- 因为 Cortex-R52 规定栈只能从 RAM 末尾开始
+ 因为 ARM 的栈从高地址向低地址增长，把栈顶放在 RAM 末尾，栈向下增长，与堆/数据区相向而行，空间利用最大化
- 因为链接脚本里地址必须按从小到大顺序排列
E: ARM 架构的栈是满递减栈（Full Descending），每次压栈先减 SP 再写数据，也就是从高地址向低地址增长。把 _stack_start 设在 RAM 最高地址，栈就有最大的向下增长空间，同时和从低地址增长的数据区不会一开始就冲突。
```
