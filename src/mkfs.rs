//! F2FS 镜像一站式打包工作流。
//!
//! 复刻 AOSP `mkfs.f2fs -g android` + `sload.f2fs -f -C -s -t -T` 的一站式流程:
//! 从源目录树 + fs_config + file_contexts 直接产出可挂载的 F2FS 镜像。

use crate::f2fs::write::{F2fsFeatures, MkfsConfig, build_f2fs_image};
use anyhow::{Result, anyhow};
use std::path::PathBuf;

/// mkfs 配置。
pub struct MkfsF2fsConfig {
    /// 源目录 (要装载的文件树根)。
    pub source_dir: PathBuf,
    /// 输出镜像路径。
    pub output_path: PathBuf,
    /// 镜像大小 (字节)。
    pub image_size: u64,
    /// 挂载点 (如 "/system", "/product")。
    pub mount_point: String,
    /// 卷标 (默认 = 挂载点)。
    pub label: Option<String>,
    /// fs_config 文件路径。
    pub fs_config: Option<PathBuf>,
    /// file_contexts 文件路径。
    pub file_contexts: Option<PathBuf>,
    /// 固定时间戳 (AOSP 用 2009-01-01 UTC = 1230768000)。
    pub timestamp: Option<u64>,
    /// 只读镜像 (设为 true 时启用 RO 特性)。
    pub readonly: bool,
}

/// 一站式构建入口。
pub fn mkfs(cfg: MkfsF2fsConfig) -> Result<()> {
    let label = cfg.label.clone().unwrap_or_else(|| cfg.mount_point.clone());
    let mut features = F2fsFeatures::default();
    if cfg.readonly {
        // RO 特性: 简化为追加 RO bit (复刻 -O ro)
        features.bits |= 0x4000;
    }

    let inner = MkfsConfig {
        source_dir: cfg.source_dir,
        output_path: cfg.output_path,
        image_size: cfg.image_size,
        mount_point: cfg.mount_point,
        label,
        fs_config: cfg.fs_config,
        file_contexts: cfg.file_contexts,
        timestamp: cfg.timestamp,
        features: Some(features),
    };

    build_f2fs_image(inner).map_err(|e| anyhow!(e))
}
