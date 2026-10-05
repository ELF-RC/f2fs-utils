//! F2FS 写入侧类型: 供各构建器使用的结构定义。

use crate::f2fs::types::Block;
use crate::f2fs::write::consts::*;

/// segment 分配语义类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegType {
    HotData,
    WarmData,
    ColdData,
    ColdNode,
    WarmNode,
    HotNode,
}

impl SegType {
    /// 是否为数据 segment。
    pub fn is_data(self) -> bool {
        matches!(self, Self::HotData | Self::WarmData | Self::ColdData)
    }

    /// 是否为 node segment。
    pub fn is_node(self) -> bool {
        matches!(self, Self::ColdNode | Self::WarmNode | Self::HotNode)
    }

    /// 映射到 NR_CURSEG_TYPE 数组下标。
    pub fn curseg_index(self) -> usize {
        match self {
            Self::HotData => CURSEG_HOT_DATA,
            Self::WarmData => CURSEG_WARM_DATA,
            Self::ColdData => CURSEG_COLD_DATA,
            Self::ColdNode => CURSEG_COLD_NODE,
            Self::WarmNode => CURSEG_WARM_NODE,
            Self::HotNode => CURSEG_HOT_NODE,
        }
    }
}

/// 文件类型 (目录项 file_type 字段)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Unknown,
    RegFile,
    Dir,
    ChrDev,
    BlkDev,
    Fifo,
    Sock,
    Symlink,
}

impl FileType {
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::RegFile => 1,
            Self::Dir => 2,
            Self::ChrDev => 3,
            Self::BlkDev => 4,
            Self::Fifo => 5,
            Self::Sock => 6,
            Self::Symlink => 7,
        }
    }

    pub fn from_mode(mode: u16) -> Self {
        match mode & 0o170_000 {
            0o040_000 => Self::Dir,
            0o100_000 => Self::RegFile,
            0o120_000 => Self::Symlink,
            0o020_000 => Self::ChrDev,
            0o060_000 => Self::BlkDev,
            0o010_000 => Self::Fifo,
            0o140_000 => Self::Sock,
            _ => Self::Unknown,
        }
    }
}

/// SIT 条目: 每个 segment 一条, 记录有效块位图与类型。
#[derive(Debug, Clone)]
pub struct SitEntry {
    pub valid_map: [u8; 64],
    pub mtime: u64,
    pub vblocks: u16, // 低 10 位为 valid block count, 高位为 seg type
}

impl Default for SitEntry {
    fn default() -> Self {
        Self {
            valid_map: [0u8; 64],
            mtime: 0,
            vblocks: 0,
        }
    }
}

impl SitEntry {
    pub fn valid_blocks(&self) -> u16 {
        self.vblocks & 0x03FF
    }

    pub fn seg_type(&self) -> u16 {
        self.vblocks >> SIT_VBLOCKS_SHIFT
    }

    pub fn set_vblocks(&mut self, valid: u16, seg_type: u16) {
        self.vblocks = (valid & 0x03FF) | (seg_type << SIT_VBLOCKS_SHIFT);
    }

    pub fn mark_block_valid(&mut self, blkoff: usize) {
        if blkoff < 512 {
            let byte = blkoff / 8;
            let bit = blkoff % 8;
            self.valid_map[byte] |= 1 << bit;
        }
    }

    pub fn to_bytes(&self) -> [u8; SIT_ENTRY_SIZE] {
        let mut buf = [0u8; SIT_ENTRY_SIZE];
        // 内核 struct f2fs_sit_entry 布局: vblocks@0 + valid_map@2 + mtime@66
        buf[..2].copy_from_slice(&self.vblocks.to_le_bytes());
        buf[2..66].copy_from_slice(&self.valid_map);
        buf[66..74].copy_from_slice(&self.mtime.to_le_bytes());
        buf
    }
}

/// SSA summary 条目: 记录一个数据/node 块的归属。
#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub nid: u32,
    pub version: u8,
    pub ofs_in_node: u16,
}

impl Summary {
    pub fn to_bytes(&self) -> [u8; SUMMARY_SIZE] {
        let mut buf = [0u8; SUMMARY_SIZE];
        buf[..4].copy_from_slice(&self.nid.to_le_bytes());
        buf[4] = self.version;
        buf[5..7].copy_from_slice(&self.ofs_in_node.to_le_bytes());
        buf
    }
}

/// node 块尾部 (最后 24 字节)。
#[derive(Debug, Clone, Copy)]
pub struct NodeFooter {
    pub nid: u32,
    pub ino: u32,
    pub flag: u8,
    pub cp_ver: u64,
    pub next_blkaddr: u32,
}

impl NodeFooter {
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        buf[..4].copy_from_slice(&self.nid.to_le_bytes());
        buf[4..8].copy_from_slice(&self.ino.to_le_bytes());
        buf[8] = self.flag;
        buf[9..17].copy_from_slice(&self.cp_ver.to_le_bytes());
        buf[17..21].copy_from_slice(&self.next_blkaddr.to_le_bytes());
        // [21..24] 为 padding
        buf
    }
}

/// 可写 NAT 条目 (写入侧)。
#[derive(Debug, Clone, Copy)]
pub struct NatEntryW {
    pub version: u8,
    pub ino: u32,
    pub block_addr: Block,
}

impl NatEntryW {
    pub fn to_bytes(&self) -> [u8; 9] {
        let mut buf = [0u8; 9];
        buf[0] = self.version;
        buf[1..5].copy_from_slice(&self.ino.to_le_bytes());
        buf[5..9].copy_from_slice(&self.block_addr.0.to_le_bytes());
        buf
    }
}

/// 目录项原始结构 (dentry block 内每条 11 字节)。
#[derive(Debug, Clone, Copy)]
pub struct DirEntryRaw {
    pub hash_code: u32,
    pub ino: u32,
    pub name_len: u16,
    pub file_type: u8,
}

impl DirEntryRaw {
    pub fn to_bytes(&self) -> [u8; F2FS_DIR_ENTRY_SIZE] {
        let mut buf = [0u8; F2FS_DIR_ENTRY_SIZE];
        buf[..4].copy_from_slice(&self.hash_code.to_le_bytes());
        buf[4..8].copy_from_slice(&self.ino.to_le_bytes());
        buf[8] = (self.name_len & 0xFF) as u8;
        buf[9] = (self.name_len >> 8) as u8;
        buf[10] = self.file_type;
        buf
    }
}

/// 超级块特性标志集。
#[derive(Debug, Clone, Copy)]
pub struct F2fsFeatures {
    pub bits: u32,
}

impl Default for F2fsFeatures {
    fn default() -> Self {
        // AOSP mkfs.f2fs -g android 默认特性 (sb feature 位)。
        // inline_data / inline_xattr / inline_dentry 是 inode i_inline 标志,
        // 不是 sb feature, 由 InodeBuilder 按需置位。
        Self {
            bits: F2FS_FEATURE_ENCRYPT | F2FS_FEATURE_EXTRA_ATTR | F2FS_FEATURE_SB_CHKSUM,
        }
    }
}

impl F2fsFeatures {
    pub fn to_bits(self) -> u32 {
        self.bits
    }

    pub fn sb_chksum(self) -> bool {
        self.bits & F2FS_FEATURE_SB_CHKSUM != 0
    }
}

/// 当前 segment 信息快照 (供 checkpoint 使用)。
#[derive(Debug, Clone)]
pub struct CursegInfo {
    pub node_segno: [u32; 3], // hot, warm, cold node
    pub node_blkoff: [u16; 3],
    pub data_segno: [u32; 3], // hot, warm, cold data
    pub data_blkoff: [u16; 3],
}

/// 超级块布局计算结果。
#[derive(Debug, Clone)]
pub struct SuperblockLayout {
    pub segment0_blkaddr: u32,
    pub cp_blkaddr: u32,
    pub sit_blkaddr: u32,
    pub nat_blkaddr: u32,
    pub ssa_blkaddr: u32,
    pub main_blkaddr: u32,
    pub segment_count: u32,
    pub segment_count_ckpt: u32,
    pub segment_count_sit: u32,
    pub segment_count_nat: u32,
    pub segment_count_ssa: u32,
    pub segment_count_main: u32,
    pub section_count: u32,
    pub block_count: u64,
    pub cp_payload: u32,
}
