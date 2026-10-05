//! f2fs-utils — F2FS tool for Android.
//!
//! 提供对 F2FS 镜像的读取与分解能力。当前实现:
//! - [`extract`]: 从镜像中分解文件树并生成 Android 的
//!   `file_contexts` / `fs_config`。
//!
//! 布局上分为底层的磁盘格式解析层 [`f2fs`] 与面向 Android 产物的高层
//! 工作流 [`extract`]。

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::doc_markdown)]
// F2FS 解析天然涉及大量定长整数 (块号/大小/偏移) 的类型转换,
// 镜像尺寸不会超过 u32 范围, 放行截断告警。
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
// 常量模块的 glob 导入在 FS 解析中常见且可读, 放行。
#![allow(clippy::wildcard_imports)]
// NAT 影子副本回退逻辑天然出现嵌套条件, 放行折叠建议。
#![allow(clippy::collapsible_if)]
// builder 模式: 先 Default::default() 再逐字段赋值是惯用法, 放行。
#![allow(clippy::field_reassign_with_default)]
// 递归装载等核心逻辑不宜强行拆分, 放行行数限制。
#![allow(clippy::too_many_lines)]

pub mod extract;
pub mod f2fs;
pub mod mkfs;
