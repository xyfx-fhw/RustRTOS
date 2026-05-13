# 从零构建 Rust 简易操作系统（基于 QEMU R52） — 项目说明

## 语言要求

- 所有 AI 对话回复使用**中文**
- 所有代码注释使用**中文**
- 所有文档（README、设计文档、计划文档等）使用**中文**
- 变量名、函数名、文件名使用英文（代码标识符保持英文惯例）

## 项目概述

一个基于 **Astro** 框架构建的 Rust 简易 RTOS 教程网站，教你在 QEMU Cortex-R52 平台上从裸机启动到任务调度，一步步用 Rust 构建一个简易操作系统。

核心功能：
- Markdown 内容驱动，支持嵌入式代码块演示
- 可编辑代码练习（Monaco Editor）
- 章节测验（选择题 + 编码题）
- 交互式图解组件（SVG + JavaScript）
- 学习进度追踪（localStorage）
- 学习证书生成与 PDF 导出

## 技术栈

| 项目 | 选型 |
|------|------|
| 前端框架 | Astro |
| 内容格式 | Markdown + 自定义 remark 插件 |
| 代码编辑器 | Monaco Editor |
| 代码执行 | Rust Playground API |
| PDF 生成 | jsPDF |
| 进度存储 | localStorage |

## 设计文档

详细需求见 [docs/superpowers/specs/2026-04-23-rust-tutorial-design.md](docs/superpowers/specs/2026-04-23-rust-tutorial-design.md)
