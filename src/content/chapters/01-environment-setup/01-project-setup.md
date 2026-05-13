---
title: "创建项目骨架"
description: "用 cargo new 初始化裸机项目，配置编译目标、build-std 和链接脚本参数"
difficulty: beginner
estimatedTime: 15
keywords: ["cargo new", "Cargo.toml", "config.toml", "build-std", "opt-level", "项目初始化"]
---

# 本章目标

- 用 `cargo new` 创建 Rust 裸机项目
- 配置 `.cargo/config.toml`：指定编译目标和 build-std
- 配置 `Cargo.toml`：设置 opt-level 让 binary 适应 32KB Flash

# 前置知识

## 已完成的章节

`01-environment-setup/00-index.md` 已完成，nightly 工具链和 QEMU 均已安装。

# 步骤一：创建项目

在你想存放代码的目录里执行（项目名自定，本系列文档用 `rtos` 举例）：

```bash
cargo new --bin rtos
cd rtos
```

执行完毕后，目录结构如下：

```text
rtos/
├── src/
│   └── main.rs    ← 先忽略，后面章节会完整替换
└── Cargo.toml
```

> **提示：** `--bin` 表示这是一个可执行程序（不是库）。项目名会成为最终可执行文件的名字，字母数字和连字符都可以，全小写是惯例。

# 步骤二：配置 .cargo/config.toml

在项目根目录新建 `.cargo/config.toml`（注意是隐藏目录 `.cargo`）：

```bash
mkdir .cargo
```

写入以下内容：

```toml
[build]
target = "armv8r-none-eabihf"

[unstable]
build-std = ["core"]
build-std-features = ["compiler-builtins-mem"]

[target.armv8r-none-eabihf]
rustflags = ["-C", "link-arg=-Tlink.x"]
```

**每一行在做什么：**

| 配置项 | 作用 |
| --- | --- |
| `target` | 默认编译到 Cortex-R52（AArch32），不用每次手写 `--target` |
| `build-std = ["core"]` | Tier 3 无预编译库，每次构建时从源码编译 `core` |
| `build-std-features = ["compiler-builtins-mem"]` | 编译内置的 `memcpy`/`memset` 等内存操作函数 |
| `rustflags = ["-Tlink.x"]` | 告诉链接器使用 `link.x` 脚本（下一章会创建） |

添加后目录结构：

```text
rtos/
├── .cargo/
│   └── config.toml    ← 新增
├── src/
│   └── main.rs
└── Cargo.toml
```

# 步骤三：配置 Cargo.toml

打开项目根目录的 `Cargo.toml`，在文件末尾追加两个 profile 配置：

```toml
[profile.dev]
opt-level = "s"

[profile.release]
opt-level = "s"
```

完整的 `Cargo.toml` 应该是这样：

```toml
[package]
name = "rtos"
version = "0.1.0"
edition = "2024"

[dependencies]

[profile.dev]
opt-level = "s"

[profile.release]
opt-level = "s"
```

**为什么 dev 也要 `opt-level = "s"`？**

mps3-an536 的 ATCM（Flash）只有 32 KB。不开优化的 debug binary 通常有几百 KB，直接超出限制导致链接失败。`"s"` 表示"优化代码体积"，是裸机嵌入式开发的标配。

此时完整的目录结构：

```text
rtos/
├── .cargo/
│   └── config.toml
├── src/
│   └── main.rs
└── Cargo.toml        ← 已追加 profile 配置
```

这就是后续所有章节的起点。下一篇文章会在这个目录里新建 `link.x` 链接脚本。

# 验证方法

此时项目还不能编译（缺少链接脚本和正确的 Rust 入口），但可以验证配置文件语法：

```bash
cargo metadata --format-version 1 > /dev/null && echo "Cargo.toml OK"
cat .cargo/config.toml
```

正常时第一条命令无报错，第二条输出刚才写的配置内容。完整的 `cargo build` 验证在 `02-minimal-boot/02-reset-handler.md` 完成后一并进行。

# 练习题

```quiz single
Q: .cargo/config.toml 里 build-std = ["core"] 的作用是什么？
- 下载 armv8r-none-eabihf 的预编译标准库
+ 在构建时从 rust-src 源码现场编译 core 库，因为 Tier 3 目标没有官方预编译版本
- 把 core 库复制到项目目录
- 允许项目使用 std 标准库
E: armv8r-none-eabihf 是 Tier 3，Rust 官方不提供预编译的 core。build-std 指示 Cargo 在每次构建时，读取 rust-src 里的源码，编译出适合这个目标架构的 core 库，再和项目代码链接在一起。
```

```quiz single
Q: 为什么 [profile.dev] 也要设置 opt-level = "s"？
- 因为 "s" 能让 debug 信息更完整
+ 因为 mps3-an536 的 Flash 只有 32KB，不优化的 debug binary 体积过大会导致链接失败
- 因为 Rust 裸机程序必须使用优化才能正常运行
- 为了和 release 配置保持一致
E: 嵌入式芯片的 Flash 容量有限，mps3-an536 只有 32KB。Rust 的 debug 构建默认不做任何优化，生成的 binary 可能几百 KB，远超 Flash 容量。opt-level = "s" 让编译器优化代码体积，是裸机嵌入式的标准做法。
```
