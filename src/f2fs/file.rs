//! 文件数据读取: inline data、直接/间接/二级间接块链、LZ4 压缩簇。

use crate::f2fs::consts::{
    DEF_ADDRS_PER_BLOCK, DEF_ADDRS_PER_INODE, F2FS_BLKSIZE, F2FS_INLINE_DATA, NIDS_PER_INODE,
    NODE_FOOTER_SIZE,
};
use crate::f2fs::error::{F2fsError, Result};
use crate::f2fs::types::{BlockAddr, Inode, Nid};
use crate::f2fs::volume::F2fsVolume;
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read, Seek};

/// 压缩簇默认 block 数。
const CLUSTER_SIZE: usize = 4;

impl<R: Read + Seek + Send> F2fsVolume<R> {
    /// 读取文件全部数据 (按 inode.size 截断)。
    pub fn read_file_data(&self, inode: &Inode, nid: Nid) -> Result<Vec<u8>> {
        if inode.size == 0 {
            return Ok(Vec::new());
        }

        // inline data: 直接从 inode 块内取
        if inode.inline & F2FS_INLINE_DATA != 0 {
            let node = self.read_node(nid)?;
            let off = inode.inline_data_offset();
            let end = node.len().saturating_sub(NODE_FOOTER_SIZE);
            if off >= end {
                return Ok(Vec::new());
            }
            let len = (inode.size as usize).min(end - off);
            return Ok(node[off..off + len].to_vec());
        }

        let blocks = self.read_data_blocks(inode, nid)?;
        let mut data = Vec::with_capacity((inode.size as usize).min(8 * 1024 * 1024));
        let mut remaining = inode.size;
        for blk in blocks {
            if remaining == 0 {
                break;
            }
            if (blk.len() as u64) <= remaining {
                data.extend_from_slice(&blk);
                remaining -= blk.len() as u64;
            } else {
                data.extend_from_slice(&blk[..remaining as usize]);
                remaining = 0;
            }
        }
        Ok(data)
    }

    /// 读取符号链接目标 (inline 或常规文件数据, 去 NUL)。
    pub fn read_symlink_target(&self, inode: &Inode, nid: Nid) -> Result<String> {
        let data = self.read_file_data(inode, nid)?;
        let len = (inode.size as usize).min(data.len());
        let trimmed = &data[..len];
        let target = if !trimmed.is_empty() && *trimmed.last().unwrap() == 0 {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };
        Ok(String::from_utf8_lossy(target).into_owned())
    }

    /// 读取文件全部数据块 (已解压、已填充空洞)。
    pub fn read_data_blocks(&self, inode: &Inode, nid: Nid) -> Result<Vec<Vec<u8>>> {
        let node = self.read_node(nid)?;
        let direct = read_u32_array(&node, inode.i_addr_offset(), inode.direct_addr_count());

        let expected = inode.size.div_ceil(F2FS_BLKSIZE as u64);
        let total = inode.blocks.min(expected);

        let mut blocks = Vec::new();
        let mut read = 0u64;
        let mut i = 0;

        // 直接块
        while i < direct.len() && read < total {
            match BlockAddr::from(direct[i]) {
                BlockAddr::Compress => {
                    let cluster = self.read_cluster_blocks(&direct[i..]);
                    let decompressed = Self::decompress_cluster(&cluster)?;
                    for blk in decompressed {
                        if read < total {
                            blocks.push(blk);
                            read += 1;
                        }
                    }
                    i += CLUSTER_SIZE;
                }
                BlockAddr::Valid(b) if self.is_valid_block(b) => {
                    blocks.push(self.read_block(b)?);
                    read += 1;
                    i += 1;
                }
                // Null / New / 无效块: 以全零 block 填充
                _ => {
                    blocks.push(zero_block());
                    read += 1;
                    i += 1;
                }
            }
        }

        // 间接/二级间接块
        if read < total {
            let indirect = self.read_indirect_blocks(&node, inode, total - read)?;
            blocks.extend(indirect);
        }
        Ok(blocks)
    }

    fn read_indirect_blocks(&self, node: &[u8], inode: &Inode, count: u64) -> Result<Vec<Vec<u8>>> {
        let off = inode.i_addr_offset() + DEF_ADDRS_PER_INODE * 4;
        let i_nid = read_u32_array(node, off, NIDS_PER_INODE);
        let mut blocks = Vec::new();
        let mut read = 0u64;

        // i_nid[0..2]: direct node
        for &n in &i_nid[0..2] {
            if read >= count || n == 0 {
                continue;
            }
            let got = self.read_direct_node(Nid(n), count - read)?;
            read += got.len() as u64;
            blocks.extend(got);
        }
        // i_nid[2..4]: indirect node
        for &n in &i_nid[2..4] {
            if read >= count || n == 0 {
                continue;
            }
            let got = self.read_indirect_node(Nid(n), count - read)?;
            read += got.len() as u64;
            blocks.extend(got);
        }
        Ok(blocks)
    }

    fn read_direct_node(&self, nid: Nid, count: u64) -> Result<Vec<Vec<u8>>> {
        let node = self.read_node(nid)?;
        let addrs = read_u32_array(&node, 0, DEF_ADDRS_PER_BLOCK);
        let mut blocks = Vec::new();
        let mut i = 0;
        while i < addrs.len() && (blocks.len() as u64) < count {
            match BlockAddr::from(addrs[i]) {
                BlockAddr::Compress => {
                    let cluster = self.read_cluster_blocks(&addrs[i..]);
                    let decompressed = Self::decompress_cluster(&cluster)?;
                    for blk in decompressed {
                        if (blocks.len() as u64) < count {
                            blocks.push(blk);
                        }
                    }
                    i += CLUSTER_SIZE;
                }
                BlockAddr::Valid(b) if self.is_valid_block(b) => {
                    blocks.push(self.read_block(b)?);
                    i += 1;
                }
                // Null / New / 无效块: 以全零 block 填充
                _ => {
                    blocks.push(zero_block());
                    i += 1;
                }
            }
        }
        Ok(blocks)
    }

    fn read_indirect_node(&self, nid: Nid, count: u64) -> Result<Vec<Vec<u8>>> {
        let node = self.read_node(nid)?;
        let nids = read_u32_array(&node, 0, DEF_ADDRS_PER_BLOCK);
        let mut blocks = Vec::new();
        for &n in &nids {
            if (blocks.len() as u64) >= count || n == 0 {
                continue;
            }
            let got = self.read_direct_node(Nid(n), count - blocks.len() as u64)?;
            blocks.extend(got);
        }
        Ok(blocks)
    }

    /// 解压一个压缩簇。
    ///
    /// F2FS 压缩格式: 首个 block 前 24 字节为头 (clen(4)+chksum(4)+reserved(16)),
    /// 其后是 clen 字节压缩载荷。
    fn decompress_cluster(compressed_blocks: &[Vec<u8>]) -> Result<Vec<Vec<u8>>> {
        let out = vec![zero_block(); CLUSTER_SIZE];
        if compressed_blocks.is_empty() {
            return Ok(out);
        }

        let mut payload = Vec::new();
        for blk in compressed_blocks {
            payload.extend_from_slice(blk);
        }
        if payload.len() < 24 {
            return Ok(out);
        }

        let clen = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
        if clen == 0 || 24 + clen > payload.len() {
            return Ok(out);
        }

        let compressed = &payload[24..24 + clen];
        let decompressed_size = CLUSTER_SIZE * F2FS_BLKSIZE;

        // 当前仅实现 LZ4 (F2FS 默认算法)
        let decompressed = match lz4_flex::decompress(compressed, decompressed_size) {
            Ok(d) => d,
            Err(e) => return Err(F2fsError::Decompression(e.to_string())),
        };

        let mut blocks = Vec::with_capacity(CLUSTER_SIZE);
        for i in 0..CLUSTER_SIZE {
            let start = i * F2FS_BLKSIZE;
            let end = start + F2FS_BLKSIZE;
            if end <= decompressed.len() {
                blocks.push(decompressed[start..end].to_vec());
            } else if start < decompressed.len() {
                let mut b = decompressed[start..].to_vec();
                b.resize(F2FS_BLKSIZE, 0);
                blocks.push(b);
            } else {
                blocks.push(zero_block());
            }
        }
        Ok(blocks)
    }

    /// 读取压缩簇内的实际数据 block (跳过 Compress/Null 标记)。
    fn read_cluster_blocks(&self, addrs: &[u32]) -> Vec<Vec<u8>> {
        let take = CLUSTER_SIZE.min(addrs.len());
        let mut blocks = Vec::with_capacity(take);
        for &addr in &addrs[..take] {
            match BlockAddr::from(addr) {
                BlockAddr::Valid(b) if self.is_valid_block(b) => {
                    if let Ok(blk) = self.read_block(b) {
                        blocks.push(blk);
                    }
                }
                _ => {}
            }
        }
        blocks
    }
}

fn zero_block() -> Vec<u8> {
    vec![0u8; F2FS_BLKSIZE]
}

/// 从偏移读取 count 个小端 u32 (越界即止)。
fn read_u32_array(data: &[u8], offset: usize, count: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(count);
    let mut cur = Cursor::new(&data[offset..]);
    for _ in 0..count {
        match cur.read_u32::<LittleEndian>() {
            Ok(v) => out.push(v),
            Err(_) => break,
        }
    }
    out
}
