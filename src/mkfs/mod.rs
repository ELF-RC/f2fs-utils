//! F2FS 镜像写入 (构建) 层。
//!
//! 复刻 AOSP `mkfs.f2fs -g android` + `sload.f2fs` 的一站式镜像构建:
//! - superblock: 超级块布局计算与序列化
//! - checkpoint: checkpoint 双副本构建
//! - nat / sit / ssa: 三张元数据表
//! - segment: 6 类 current-segment 块分配器
//! - inode / dentry: inode 与目录块构建器 (含 TEA 哈希)
//! - config: fs_config / file_contexts 解析
//! - builder: 顶层编排器
//!
//! CRC 算法 (seed = F2FS_MAGIC, 不取反) 见 crc。

pub mod builder;
pub mod checkpoint;
pub mod config;
pub mod consts;
pub mod crc;
pub mod dentry;
pub mod inode;
pub mod nat;
pub mod segment;
pub mod sit;
pub mod sparse;
pub mod ssa;
pub mod superblock;
pub mod types;

pub use builder::{F2fsBuilder, MkfsConfig, build_f2fs_image};
pub use config::{FsConfig, FsConfigEntry, SelinuxContexts, SelinuxEntry};
pub use consts::*;
pub use dentry::{DentryBlockBuilder, DentryInfo, InlineDentryBuilder};
pub use inode::{DirectNodeBuilder, IndirectNodeBuilder, InlineXattrEntry, InodeBuilder};
pub use nat::NatManager;
pub use segment::SegmentAllocator;
pub use sit::SitManager;
pub use sparse::convert_file as convert_to_sparse;
pub use ssa::SsaManager;
pub use superblock::SuperblockBuilder;
pub use types::{F2fsFeatures, FileType, SegType, SuperblockLayout};

// ── mkfs 工作流 ──────────────────────────────────────────

use anyhow::{Result, anyhow};
use std::path::PathBuf;

/// mkfs 配置。
#[allow(clippy::struct_excessive_bools)]
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
    /// 构建完成后转为 Android sparse 镜像。
    pub sparse: bool,
    /// 启用文件压缩 (F2FS_FEATURE_COMPRESSION)。
    pub compression: bool,
    /// 压缩算法: "lz4" (默认)。
    pub compress_algo: String,
    /// log2(压缩簇块数), 默认 2 = 4 块 = 16KB。
    pub cluster_log: u8,
    /// 压缩等级 (LZ4: 3-16, ZSTD: 1-22, 0 = 默认)。
    pub compress_level: u8,
    /// 启用 casefold (大小写不敏感文件名)。
    pub casefold: bool,
}

/// 一站式构建入口。
pub fn mkfs(cfg: MkfsF2fsConfig) -> Result<()> {
    let label = cfg.label.clone().unwrap_or_else(|| cfg.mount_point.clone());
    let mut features = F2fsFeatures::default();
    if cfg.readonly {
        features.bits |= crate::mkfs::consts::F2FS_FEATURE_RO;
    }
    if cfg.compression {
        features.bits |= crate::mkfs::consts::F2FS_FEATURE_COMPRESSION;
    }
    if cfg.casefold {
        features.bits |= crate::mkfs::consts::F2FS_FEATURE_CASEFOLD;
    }

    let compress_algo = match cfg.compress_algo.as_str() {
        "lzo" => crate::mkfs::consts::COMPRESS_LZO,
        "zstd" => crate::mkfs::consts::COMPRESS_ZSTD,
        _ => crate::mkfs::consts::COMPRESS_LZ4,
    };

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
        sparse: cfg.sparse,
        compression: cfg.compression,
        compress_algo,
        cluster_log: cfg.cluster_log,
        compress_level: cfg.compress_level,
        casefold: cfg.casefold,
    };

    build_f2fs_image(inner).map_err(|e| anyhow!(e))
}
