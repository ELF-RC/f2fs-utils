//! F2FS 写入侧常量 (构建镜像时使用)。
//!
//! 依据 `include/linux/f2fs_fs.h` 与 f2fs-tools。

// 超级块几何
pub const SUPERBLOCK_SIZE: usize = 3072;
pub const SUPERBLOCK_OFFSET: u64 = 1024;
pub const SB_CHKSUM_OFFSET: usize = 3068;
pub const MAX_VOLUME_NAME: usize = 1024;
pub const VERSION_LEN: usize = 256;
pub const F2FS_VERSION: &[u8] = b"5.15.0";
pub const F2FS_MAJOR_VERSION: u32 = 1;
pub const F2FS_MINOR_VERSION: u32 = 0;

// 基础几何默认值
pub const DEFAULT_SECTOR_SIZE: u32 = 512;
pub const DEFAULT_BLOCKS_PER_SEGMENT: u32 = 512; // 4KiB * 512 = 2MiB
pub const DEFAULT_SEGMENTS_PER_SECTION: u32 = 1;
pub const DEFAULT_SECTIONS_PER_ZONE: u32 = 1;
pub const F2FS_MIN_SEGMENTS: u32 = 9;
pub const F2FS_NUMBER_OF_CHECKPOINT_PACK: u32 = 2;

// NAT / SIT 每块的条目数
pub const NAT_ENTRY_PER_BLOCK_W: usize = 4096 / 9;
pub const SIT_ENTRY_PER_BLOCK_W: usize = 4096 / 74;
pub const SIT_ENTRY_SIZE: usize = 74;
pub const SIT_VBLOCKS_SHIFT: u16 = 10;

// inode 几何
pub const EXTRA_ISIZE: u16 = 36;
pub const DEFAULT_INLINE_XATTR_SIZE: u16 = 50;

// 目录块几何
pub const NR_DENTRY_IN_BLOCK_W: usize = 214;
pub const F2FS_DIR_ENTRY_SIZE: usize = 11;
pub const DENTRY_BITMAP_SIZE: usize = 27;
pub const DENTRY_RESERVED_SIZE: usize = 3;

// inline 目录几何
pub const NR_INLINE_DENTRY_W: usize = 61;
pub const INLINE_DENTRY_BITMAP_SIZE: usize = 8;
pub const INLINE_RESERVED_SIZE: usize = 1;

// inline data 最大容量 (inode 内可嵌入的数据上限, 保守取值兼容 extra_attr)。
pub const MAX_INLINE_DATA_SIZE: usize = 3488;

// summary 块几何
pub const SUMMARY_SIZE: usize = 7;
pub const SUM_FOOTER_SIZE: usize = 5;
pub const SUM_TYPE_NODE: u8 = 1;
pub const SUM_TYPE_DATA: u8 = 0;

// checkpoint 字段
pub const CHECKPOINT_HEADER_SIZE: usize = 192;
pub const CP_CHKSUM_OFFSET: usize = 4092;
pub const CP_UMOUNT_FLAG: u32 = 0x0000_0010;
pub const CP_COMPACT_SUM_FLAG_W: u32 = 0x0000_0004;
pub const CP_NOCRC_FLAG: u32 = 0x0000_0008;
pub const CP_TRIMMED_FLAG: u32 = 0x0000_0200;

// current segment 类型
pub const CURSEG_HOT_DATA: usize = 0;
pub const CURSEG_WARM_DATA: usize = 1;
pub const CURSEG_COLD_DATA: usize = 2;
pub const CURSEG_COLD_NODE: usize = 3;
pub const CURSEG_WARM_NODE: usize = 4;
pub const CURSEG_HOT_NODE: usize = 5;
pub const NR_CURSEG_TYPE: usize = 6;

// inode 内 i_addr 间接 nid 数
pub const NIDS_PER_BLOCK_W: usize = (4096 - 24) / 4;

// 预留 inode 号
pub const F2FS_NODE_INO: u32 = 1;
pub const F2FS_META_INO: u32 = 2;
pub const F2FS_FIRST_INO: u32 = 4;

// superblock 特性标志位
pub const F2FS_FEATURE_ENCRYPT: u32 = 0x0000_0001;
pub const F2FS_FEATURE_BLKZONED: u32 = 0x0000_0002;
pub const F2FS_FEATURE_ATOMIC_WRITE: u32 = 0x0000_0004;
pub const F2FS_FEATURE_EXTRA_ATTR: u32 = 0x0000_0008;
pub const F2FS_FEATURE_PROJECT_QUOTA: u32 = 0x0000_0010;
pub const F2FS_FEATURE_INLINE_XATTR: u32 = 0x0000_0020;
pub const F2FS_FEATURE_INLINE_DATA: u32 = 0x0000_0040;
pub const F2FS_FEATURE_INLINE_DENTRY: u32 = 0x0000_0080;
pub const F2FS_FEATURE_SB_CHKSUM: u32 = 0x0000_0200;
pub const F2FS_FEATURE_CASEFOLD: u32 = 0x0000_1000;
pub const F2FS_FEATURE_COMPRESSION: u32 = 0x0000_2000;

// 压缩算法 (inode i_compress_algorithm)
pub const COMPRESS_LZ4: u8 = 0;
pub const COMPRESS_LZO: u8 = 1;
pub const COMPRESS_ZSTD: u8 = 2;

// inode 压缩字段偏移 (extra_attr 区内, 相对 inode 起始)
// i_compr_blocks 是 __le64 (8 字节)
pub const INODE_OFF_COMPR_BLOCKS: usize = 384;
pub const INODE_OFF_COMPRESS_ALGO: usize = 392;
pub const INODE_OFF_LOG_CLUSTER_SIZE: usize = 393;
pub const INODE_OFF_COMPRESS_FLAG: usize = 394;

// 压缩簇头: clen(4) + chksum(4) + reserved(16) = 24 字节
pub const COMPRESS_HEADER_SIZE: usize = 24;
