//! 目录项解析: 常规 dentry block 与 inline dentry。

use crate::f2fs::consts::*;
use crate::f2fs::error::{F2fsError, Result};
use crate::f2fs::types::{DirEntry, Inode, Nid};
use crate::f2fs::volume::F2fsVolume;
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read, Seek};

/// 目录块几何 (常规块)。
const BITMAP_OFF: usize = 0;
const DENTRY_OFF: usize = SIZE_OF_DENTRY_BITMAP + SIZE_OF_RESERVED;
const FILENAME_OFF: usize = DENTRY_OFF + NR_DENTRY_IN_BLOCK * SIZE_OF_DIR_ENTRY;

impl<R: Read + Seek + Send> F2fsVolume<R> {
    /// 读取目录下全部条目 (含 inline 目录)。
    pub fn read_dir(&self, inode: &Inode, nid: Nid) -> Result<Vec<DirEntry>> {
        if inode.inline & F2FS_INLINE_DENTRY != 0 {
            let node = self.read_node(nid)?;
            return parse_inline_dentry(&node, inode);
        }

        let blocks = self.read_data_blocks(inode, nid)?;
        let mut entries = Vec::new();
        for blk in &blocks {
            entries.extend(parse_dentry_block(blk)?);
        }
        Ok(entries)
    }
}

fn parse_dentry_block(block: &[u8]) -> Result<Vec<DirEntry>> {
    if block.len() < FILENAME_OFF {
        return Ok(Vec::new());
    }
    let bitmap = &block[BITMAP_OFF..BITMAP_OFF + SIZE_OF_DENTRY_BITMAP];
    let mut entries = Vec::new();
    let mut i = 0;
    while i < NR_DENTRY_IN_BLOCK {
        let byte = i / 8;
        let bit = i % 8;
        if bitmap[byte] & (1 << bit) == 0 {
            i += 1;
            continue;
        }

        let pos = DENTRY_OFF + i * SIZE_OF_DIR_ENTRY;
        if pos + SIZE_OF_DIR_ENTRY > block.len() {
            break;
        }
        let mut cur = Cursor::new(&block[pos..pos + SIZE_OF_DIR_ENTRY]);
        let _hash = cur.read_u32::<LittleEndian>()?;
        let nid = Nid(cur.read_u32::<LittleEndian>()?);
        let name_len = cur.read_u16::<LittleEndian>()? as usize;
        let file_type = cur.read_u8()?;

        if name_len == 0 || name_len > F2FS_NAME_LEN {
            // name_len==0 是续接槽, 跳过一个 slot
            i += 1;
            continue;
        }

        let name_pos = FILENAME_OFF + i * F2FS_SLOT_LEN;
        if name_pos + name_len > block.len() {
            break;
        }
        let name = String::from_utf8_lossy(&block[name_pos..name_pos + name_len]).to_string();
        if name != "." && name != ".." {
            entries.push(DirEntry {
                name,
                nid,
                file_type,
            });
        }
        i += name_len.div_ceil(F2FS_SLOT_LEN).max(1);
    }
    Ok(entries)
}

/// inline 目录: 嵌入 inode 块内, 几何更小。
fn parse_inline_dentry(node: &[u8], inode: &Inode) -> Result<Vec<DirEntry>> {
    let base = inline_dentry_base(inode);
    let bitmap_size = INLINE_DENTRY_BITMAP_SIZE;
    let dentry_off = base + bitmap_size + INLINE_RESERVED_SIZE;
    let filename_off = dentry_off + NR_INLINE_DENTRY * SIZE_OF_DIR_ENTRY;

    if node.len() < filename_off {
        return Err(F2fsError::InvalidData("inline dentry out of bounds".into()));
    }
    let bitmap = &node[base..base + bitmap_size];
    let mut entries = Vec::new();
    let mut i = 0;
    while i < NR_INLINE_DENTRY {
        let byte = i / 8;
        let bit = i % 8;
        if byte >= bitmap.len() || bitmap[byte] & (1 << bit) == 0 {
            i += 1;
            continue;
        }
        let pos = dentry_off + i * SIZE_OF_DIR_ENTRY;
        if pos + SIZE_OF_DIR_ENTRY > node.len() {
            break;
        }
        let mut cur = Cursor::new(&node[pos..pos + SIZE_OF_DIR_ENTRY]);
        let _hash = cur.read_u32::<LittleEndian>()?;
        let nid = Nid(cur.read_u32::<LittleEndian>()?);
        let name_len = cur.read_u16::<LittleEndian>()? as usize;
        let file_type = cur.read_u8()?;
        if name_len == 0 || name_len > F2FS_NAME_LEN {
            i += 1;
            continue;
        }
        let name_pos = filename_off + i * F2FS_SLOT_LEN;
        if name_pos + name_len > node.len() {
            break;
        }
        let name = String::from_utf8_lossy(&node[name_pos..name_pos + name_len]).to_string();
        if name != "." && name != ".." {
            entries.push(DirEntry {
                name,
                nid,
                file_type,
            });
        }
        i += name_len.div_ceil(F2FS_SLOT_LEN).max(1);
    }
    Ok(entries)
}

/// inline dentry 区在 inode 块内的起始偏移。
fn inline_dentry_base(inode: &Inode) -> usize {
    let off = inode.i_addr_offset();
    // inline dentry 复用 i_addr[0] 之后的区域
    off + 4 // 跳过 i_addr[0] (保留作 inline 标记槽)
}
