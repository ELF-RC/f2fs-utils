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
pub use ssa::SsaManager;
pub use superblock::SuperblockBuilder;
pub use types::{F2fsFeatures, FileType, SegType, SuperblockLayout};
