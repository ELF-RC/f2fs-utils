//! F2FS 磁盘格式常量。
//!
//! 依据 Linux 内核 `include/linux/f2fs_fs.h`。

// 魔数与基础几何
pub const F2FS_MAGIC: u32 = 0xF2F5_2010;
pub const F2FS_SUPER_OFFSET: u64 = 1024;
pub const F2FS_BLKSIZE: usize = 4096;
pub const F2FS_NAME_LEN: usize = 255;
pub const F2FS_SLOT_LEN: usize = 8;

// 超级块内字段偏移 (相对超级块起始)
pub const SB_OFF_LOG_BLOCKS_PER_SEG: usize = 20;
pub const SB_OFF_BLOCK_COUNT: usize = 36;
pub const SB_OFF_SEGMENT_COUNT: usize = 48;
pub const SB_OFF_SEGMENT0_BLKADDR: usize = 72;
pub const SB_OFF_CP_BLKADDR: usize = 76;
pub const SB_OFF_SIT_BLKADDR: usize = 80;
pub const SB_OFF_NAT_BLKADDR: usize = 84;
pub const SB_OFF_SSA_BLKADDR: usize = 88;
pub const SB_OFF_MAIN_BLKADDR: usize = 92;
pub const SB_OFF_SEGMENT_COUNT_NAT: usize = 60;
pub const SB_OFF_CP_PAYLOAD: usize = 1664;

// 块地址哨兵
pub const NULL_ADDR: u32 = 0;
pub const NEW_ADDR: u32 = 0xFFFF_FFFF;
pub const COMPRESS_ADDR: u32 = 0xFFFF_FFFE;

// 预留 inode 号
pub const F2FS_ROOT_INO: u32 = 3;

// inode / node 几何
pub const INODE_SIZE: usize = 360;
pub const NODE_FOOTER_SIZE: usize = 24;
pub const DEF_ADDRS_PER_INODE: usize = (F2FS_BLKSIZE - INODE_SIZE - 20 - NODE_FOOTER_SIZE) / 4;
pub const DEF_ADDRS_PER_BLOCK: usize = (F2FS_BLKSIZE - NODE_FOOTER_SIZE) / 4;
pub const NIDS_PER_INODE: usize = 5;
pub const DEFAULT_INLINE_XATTR_ADDRS: usize = 50;

// inode 内 i_inline 标志
pub const F2FS_INLINE_XATTR: u8 = 0x01;
pub const F2FS_INLINE_DATA: u8 = 0x02;
pub const F2FS_INLINE_DENTRY: u8 = 0x04;
pub const F2FS_DATA_EXIST: u8 = 0x08;
pub const F2FS_EXTRA_ATTR: u8 = 0x20;

// 目录块几何 (常规块)
pub const NR_DENTRY_IN_BLOCK: usize = 214;
pub const SIZE_OF_DENTRY_BITMAP: usize = 27;
pub const SIZE_OF_RESERVED: usize = 3;
pub const SIZE_OF_DIR_ENTRY: usize = 11;

// inline 目录几何
pub const NR_INLINE_DENTRY: usize = 61;
pub const INLINE_DENTRY_BITMAP_SIZE: usize = 8;
pub const INLINE_RESERVED_SIZE: usize = 1;

// 文件类型 (dentry file_type 字段)
pub const F2FS_FT_REG_FILE: u8 = 1;
pub const F2FS_FT_DIR: u8 = 2;
pub const F2FS_FT_SYMLINK: u8 = 7;

// NAT 条目
pub const NAT_ENTRY_SIZE: usize = 9;
pub const NAT_ENTRY_PER_BLOCK: usize = F2FS_BLKSIZE / NAT_ENTRY_SIZE;

// checkpoint 标志
pub const CP_COMPACT_SUM_FLAG: u32 = 0x0000_0004;

// checkpoint 头部字段偏移
pub const CP_OFF_CHECKPOINT_VER: usize = 0;
pub const CP_OFF_FLAGS: usize = 132;
pub const CP_OFF_CP_PACK_TOTAL_BLOCK_COUNT: usize = 136;
pub const CP_OFF_CP_PACK_START_SUM: usize = 140;

// summary journal (compact summary 块内的 NAT journal 区)
pub const SUM_FOOTER_SIZE: usize = 5;
pub const SUM_ENTRIES_SIZE: usize = 7 * 512;
pub const SUM_JOURNAL_SIZE: usize = F2FS_BLKSIZE - SUM_FOOTER_SIZE - SUM_ENTRIES_SIZE;
pub const NAT_JOURNAL_ENTRY_SIZE: usize = 13; // nid(4) + nat_entry(9)

// xattr 索引
pub const F2FS_XATTR_INDEX_SECURITY: u8 = 6;

// inode 模式位
pub const S_IFMT: u16 = 0o170_000;
pub const S_IFSOCK: u16 = 0o140_000;
pub const S_IFLNK: u16 = 0o120_000;
pub const S_IFREG: u16 = 0o100_000;
pub const S_IFBLK: u16 = 0o060_000;
pub const S_IFDIR: u16 = 0o040_000;
pub const S_IFCHR: u16 = 0o020_000;
pub const S_IFIFO: u16 = 0o010_000;
