//! F2FS 磁盘结构类型与解析。
//!
//! 所有多字节字段为小端序,布局依据 `include/linux/f2fs_fs.h`。

use crate::f2fs::consts::*;
use crate::f2fs::error::{F2fsError, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

/// 节点 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Nid(pub u32);

/// 物理块号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Block(pub u32);

/// 块地址分类: 区分未分配 / 新分配 / 压缩标记 / 有效块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAddr {
    Null,
    New,
    Compress,
    Valid(Block),
}

impl From<u32> for BlockAddr {
    fn from(addr: u32) -> Self {
        match addr {
            NULL_ADDR => BlockAddr::Null,
            NEW_ADDR => BlockAddr::New,
            COMPRESS_ADDR => BlockAddr::Compress,
            _ => BlockAddr::Valid(Block(addr)),
        }
    }
}

/// 超级块:仅抽取分解所需的字段。
#[derive(Debug, Clone)]
pub struct Superblock {
    pub block_count: u64,
    pub segment_count: u32,
    pub cp_blkaddr: u32,
    pub sit_blkaddr: u32,
    pub nat_blkaddr: u32,
    pub ssa_blkaddr: u32,
    pub main_blkaddr: u32,
    pub log_blocks_per_seg: u32,
    pub segment_count_nat: u32,
}

impl Superblock {
    /// 从一个 4KiB 超级块缓冲解析。
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(buf);
        let magic = cur.read_u32::<LittleEndian>()?;
        if magic != F2FS_MAGIC {
            return Err(F2fsError::InvalidMagic {
                expected: F2FS_MAGIC,
                got: magic,
            });
        }

        cur.set_position(SB_OFF_LOG_BLOCKS_PER_SEG as u64);
        let log_blocks_per_seg = cur.read_u32::<LittleEndian>()?;

        cur.set_position(SB_OFF_BLOCK_COUNT as u64);
        let block_count = cur.read_u64::<LittleEndian>()?;

        cur.set_position(SB_OFF_SEGMENT_COUNT as u64);
        let segment_count = cur.read_u32::<LittleEndian>()?;

        cur.set_position(SB_OFF_SEGMENT_COUNT_NAT as u64);
        let segment_count_nat = cur.read_u32::<LittleEndian>()?;

        cur.set_position(SB_OFF_SEGMENT0_BLKADDR as u64);
        let segment0_blkaddr = cur.read_u32::<LittleEndian>()?;
        let cp_blkaddr = cur.read_u32::<LittleEndian>()?;
        let sit_blkaddr = cur.read_u32::<LittleEndian>()?;
        let nat_blkaddr = cur.read_u32::<LittleEndian>()?;
        let ssa_blkaddr = cur.read_u32::<LittleEndian>()?;
        let main_blkaddr = cur.read_u32::<LittleEndian>()?;

        let _ = segment0_blkaddr; // 解析但未直接使用

        Ok(Self {
            block_count,
            segment_count,
            cp_blkaddr,
            sit_blkaddr,
            nat_blkaddr,
            ssa_blkaddr,
            main_blkaddr,
            log_blocks_per_seg,
            segment_count_nat,
        })
    }

    pub fn blocks_per_seg(&self) -> Result<u32> {
        1u32.checked_shl(self.log_blocks_per_seg)
            .ok_or_else(|| F2fsError::InvalidData("invalid log_blocks_per_seg".into()))
    }
}

/// NAT 条目: nid → 块地址。
#[derive(Debug, Clone, Copy, Default)]
pub struct NatEntry {
    pub block_addr: Block,
}

impl NatEntry {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < NAT_ENTRY_SIZE {
            return Err(F2fsError::InvalidData("NAT entry too short".into()));
        }
        // version(1) + ino(4) + block_addr(4); 仅取 block_addr
        let block_addr = u32::from_le_bytes([data[5], data[6], data[7], data[8]]);
        Ok(Self {
            block_addr: Block(block_addr),
        })
    }
}

/// inode: 仅抽取分解所需的字段。
#[derive(Debug, Clone)]
pub struct Inode {
    pub mode: u16,
    pub inline: u8,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
    pub flags: u32,
    pub xattr_nid: u32,
    pub extra_isize: u16,
}

impl Inode {
    /// 从一个节点块缓冲(完整 4KiB)解析 inode 头部。
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);

        let mode = cur.read_u16::<LittleEndian>()?;

        cur.set_position(3);
        let inline = cur.read_u8()?;

        cur.set_position(4);
        let uid = cur.read_u32::<LittleEndian>()?;
        let gid = cur.read_u32::<LittleEndian>()?;

        cur.set_position(16);
        let size = cur.read_u64::<LittleEndian>()?;
        let blocks = cur.read_u64::<LittleEndian>()?;

        cur.set_position(76);
        let xattr_nid = cur.read_u32::<LittleEndian>()?;

        cur.set_position(116);
        let flags = cur.read_u32::<LittleEndian>()?;

        cur.set_position(360);
        let extra_isize = cur.read_u16::<LittleEndian>()?;

        Ok(Self {
            mode,
            inline,
            uid,
            gid,
            size,
            blocks,
            flags,
            xattr_nid,
            extra_isize,
        })
    }

    pub fn is_dir(&self) -> bool {
        (self.mode & S_IFMT) == S_IFDIR
    }

    pub fn is_reg(&self) -> bool {
        (self.mode & S_IFMT) == S_IFREG
    }

    pub fn is_symlink(&self) -> bool {
        (self.mode & S_IFMT) == S_IFLNK
    }

    pub fn has_extra_attr(&self) -> bool {
        self.inline & F2FS_EXTRA_ATTR != 0
    }

    /// inode 内 inline data 起始偏移 (跳过 i_addr[0] 与 extra_attr 区)。
    pub fn inline_data_offset(&self) -> usize {
        let mut off = INODE_SIZE;
        if self.has_extra_attr() {
            off += self.extra_isize as usize;
        }
        off + 4
    }

    /// i_addr 直接块地址区起始偏移。
    pub fn i_addr_offset(&self) -> usize {
        let mut off = INODE_SIZE;
        if self.has_extra_attr() {
            off += self.extra_isize as usize;
        }
        off
    }

    /// 直接块地址数量 (随 extra_attr / inline_xattr 收缩)。
    pub fn direct_addr_count(&self) -> usize {
        let mut count = DEF_ADDRS_PER_INODE;
        if self.has_extra_attr() {
            count -= self.extra_isize as usize / 4;
        }
        if self.inline & F2FS_INLINE_XATTR != 0 {
            count -= DEFAULT_INLINE_XATTR_ADDRS;
        }
        count
    }
}

/// 目录项 (已解析)。
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub nid: Nid,
    pub file_type: u8,
}

/// xattr 条目 (已解析)。
#[derive(Debug, Clone)]
pub struct XattrEntry {
    pub name_index: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

impl XattrEntry {
    /// 从字节解析单条 xattr,返回 (条目, 对齐后总字节数)。
    pub fn from_bytes(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(F2fsError::InvalidData("xattr entry too short".into()));
        }
        let name_index = data[0];
        let name_len = data[1] as usize;
        let value_size = u16::from_le_bytes([data[2], data[3]]) as usize;

        let name_end = 4usize
            .checked_add(name_len)
            .ok_or_else(|| F2fsError::InvalidData("xattr name length overflow".into()))?;
        if data.len() < name_end {
            return Err(F2fsError::InvalidData("xattr name incomplete".into()));
        }
        let name = data[4..name_end].to_vec();

        let value_end = name_end
            .checked_add(value_size)
            .ok_or_else(|| F2fsError::InvalidData("xattr value length overflow".into()))?;
        if data.len() < value_end {
            return Err(F2fsError::InvalidData("xattr value incomplete".into()));
        }
        let value = data[name_end..value_end].to_vec();

        // 整条按 4 字节对齐
        let total = (value_end + 3) & !3;
        Ok((
            Self {
                name_index,
                name,
                value,
            },
            total,
        ))
    }

    /// 完整属性名 (含前缀)。
    pub fn full_name(&self) -> String {
        let prefix = match self.name_index {
            F2FS_XATTR_INDEX_SECURITY => "security.",
            _ => "",
        };
        format!("{prefix}{}", String::from_utf8_lossy(&self.name))
    }
}
