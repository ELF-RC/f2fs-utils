//! F2FS 卷读取后端: 超级块、checkpoint、NAT 查询与块读取。
//!
//! 支持任意 `Read + Seek` 数据源 (普通文件或稀疏镜像虚拟流)。

use crate::f2fs::consts::*;
use crate::f2fs::error::{F2fsError, Result};
use crate::f2fs::types::{Block, Inode, NatEntry, Nid, Superblock};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, RwLock};

/// F2FS 卷句柄。
pub struct F2fsVolume<R: Read + Seek + Send = File> {
    file: Arc<RwLock<R>>,
    pub superblock: Superblock,
    nat_cache: Arc<RwLock<HashMap<Nid, NatEntry>>>,
    nat_journal: HashMap<Nid, NatEntry>,
    /// 每个 NAT 副本包含的 NAT block 数 (用于定位影子副本)。
    nat_blocks_per_copy: u32,
}

impl F2fsVolume<File> {
    /// 从本地文件路径打开。
    pub fn open(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        Self::from_reader(file)
    }
}

impl<R: Read + Seek + Send> F2fsVolume<R> {
    /// 从任意读取后端构造。
    pub fn from_reader(reader: R) -> Result<Self> {
        let file = Arc::new(RwLock::new(reader));

        // 读取超级块 (位于偏移 1024)
        let mut buf = vec![0u8; F2FS_BLKSIZE];
        {
            let mut f = lock_write(&file)?;
            f.seek(SeekFrom::Start(F2FS_SUPER_OFFSET))?;
            f.read_exact(&mut buf)?;
        }
        let superblock = Superblock::from_bytes(&buf)?;
        let blocks_per_seg = superblock.blocks_per_seg()?;
        let nat_blocks_per_copy = (superblock.segment_count_nat / 2).saturating_mul(blocks_per_seg);

        // 选择有效的 checkpoint pack (版本号较大者)
        let cp0_base = superblock.cp_blkaddr;
        let cp1_base = superblock.cp_blkaddr + blocks_per_seg;
        let cp_primary = read_block_raw(&file, Block(cp0_base))?;
        let cp_secondary = read_block_raw(&file, Block(cp1_base))?;
        let cp_primary_ver = read_le_u64(&cp_primary, CP_OFF_CHECKPOINT_VER)?;
        let cp_secondary_ver = read_le_u64(&cp_secondary, CP_OFF_CHECKPOINT_VER)?;
        let (active_cp, active_base) = if cp_secondary_ver > cp_primary_ver {
            (cp_secondary, cp1_base)
        } else {
            (cp_primary, cp0_base)
        };

        // compact summary 块位于 active pack 的 1 + cp_payload 处
        let compact_summary =
            read_block_raw(&file, Block(active_base + 1 + superblock.cp_payload))?;
        let nat_journal = load_nat_journal(&active_cp, &compact_summary)?;

        Ok(Self {
            file,
            superblock,
            nat_cache: Arc::new(RwLock::new(HashMap::new())),
            nat_journal,
            nat_blocks_per_copy,
        })
    }

    /// 读取一个 4KiB 块。
    pub fn read_block(&self, block: Block) -> Result<Vec<u8>> {
        read_block_raw(&self.file, block)
    }

    /// 读取节点块 (由 nid 经 NAT 解析到块地址)。
    pub fn read_node(&self, nid: Nid) -> Result<Vec<u8>> {
        let entry = self.get_nat_entry(nid)?;
        self.read_block(entry.block_addr)
    }

    /// 读取并解析 inode。
    pub fn read_inode(&self, nid: Nid) -> Result<Inode> {
        let node = self.read_node(nid)?;
        Inode::from_bytes(&node)
    }

    /// 块地址是否落在 main 区间内。
    pub fn is_valid_block(&self, block: Block) -> bool {
        block.0 >= self.superblock.main_blkaddr && block.0 < self.superblock.block_count as u32
    }

    fn get_nat_entry(&self, nid: Nid) -> Result<NatEntry> {
        // 1. 命中缓存
        {
            let cache = lock_read(&self.nat_cache)?;
            if let Some(entry) = cache.get(&nid) {
                return Ok(*entry);
            }
        }
        // 2. 命中 NAT journal (checkpoint 内的最新状态)
        if let Some(entry) = self.nat_journal.get(&nid) {
            let entry = *entry;
            {
                let mut cache = lock_write(&self.nat_cache)?;
                cache.insert(nid, entry);
            }
            return Ok(entry);
        }
        // 3. 落到磁盘 NAT 页 (含影子副本回退)
        let nat_block_idx = nid.0 / NAT_ENTRY_PER_BLOCK as u32;
        let entry_idx = (nid.0 % NAT_ENTRY_PER_BLOCK as u32) as usize;
        let mut entry =
            self.read_nat_entry_from_copy(self.superblock.nat_blkaddr, nat_block_idx, entry_idx)?;

        // 主副本无效时回退到影子副本
        if (entry.block_addr.0 == 0 || !self.is_valid_block(entry.block_addr))
            && self.nat_blocks_per_copy > 0
        {
            if let Ok(secondary) = self.read_nat_entry_from_copy(
                self.superblock.nat_blkaddr + self.nat_blocks_per_copy,
                nat_block_idx,
                entry_idx,
            ) {
                if secondary.block_addr.0 != 0 && self.is_valid_block(secondary.block_addr) {
                    entry = secondary;
                }
            }
        }

        {
            let mut cache = lock_write(&self.nat_cache)?;
            cache.insert(nid, entry);
        }
        Ok(entry)
    }

    fn read_nat_entry_from_copy(
        &self,
        nat_base: u32,
        block_idx: u32,
        entry_idx: usize,
    ) -> Result<NatEntry> {
        let data = self.read_block(Block(nat_base + block_idx))?;
        let off = entry_idx * NAT_ENTRY_SIZE;
        let end = off + NAT_ENTRY_SIZE;
        if end > data.len() {
            return Err(F2fsError::InvalidData("NAT entry out of bounds".into()));
        }
        NatEntry::from_bytes(&data[off..end])
    }
}

fn read_block_raw<R: Read + Seek>(file: &Arc<RwLock<R>>, block: Block) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; F2FS_BLKSIZE];
    let offset = u64::from(block.0) * F2FS_BLKSIZE as u64;
    let mut f = lock_write(file)?;
    f.seek(SeekFrom::Start(offset))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_le_u64(data: &[u8], offset: usize) -> Result<u64> {
    if offset + 8 > data.len() {
        return Err(F2fsError::InvalidData("u64 out of bounds".into()));
    }
    Ok(u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

fn read_le_u32(data: &[u8], offset: usize) -> Result<u32> {
    if offset + 4 > data.len() {
        return Err(F2fsError::InvalidData("u32 out of bounds".into()));
    }
    Ok(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// 从 checkpoint 头部 (取 flags) 与 compact summary 块 (取 journal) 加载 NAT journal。
///
/// compact summary 块布局: NAT journal 在前 SUM_JOURNAL_SIZE 字节,
/// 格式为 count(2) + 条目 (每条 nid(4)+version(1)+ino(4)+block_addr(4) = 13)。
fn load_nat_journal(cp_header: &[u8], compact_summary: &[u8]) -> Result<HashMap<Nid, NatEntry>> {
    let mut journal = HashMap::new();

    let flags = read_le_u32(cp_header, CP_OFF_FLAGS)?;
    if flags & CP_COMPACT_SUM_FLAG == 0 {
        return Ok(journal);
    }

    if compact_summary.len() < SUM_JOURNAL_SIZE {
        return Ok(journal);
    }

    let nat_count = u16::from_le_bytes([compact_summary[0], compact_summary[1]]) as usize;
    for i in 0..nat_count {
        let off = 2 + i * NAT_JOURNAL_ENTRY_SIZE;
        if off + NAT_JOURNAL_ENTRY_SIZE > SUM_JOURNAL_SIZE {
            break;
        }
        let nid = read_le_u32(compact_summary, off)?;
        // version(1) @ off+4, ino(4) @ off+5, block_addr(4) @ off+9
        let block_addr = read_le_u32(compact_summary, off + 9)?;
        if block_addr == 0 {
            continue;
        }
        journal.insert(
            Nid(nid),
            NatEntry {
                block_addr: Block(block_addr),
            },
        );
    }
    Ok(journal)
}

fn lock_read<T>(lock: &RwLock<T>) -> Result<std::sync::RwLockReadGuard<'_, T>> {
    lock.read()
        .map_err(|e| F2fsError::InvalidData(format!("lock poisoned: {e}")))
}

fn lock_write<T>(lock: &RwLock<T>) -> Result<std::sync::RwLockWriteGuard<'_, T>> {
    lock.write()
        .map_err(|e| F2fsError::InvalidData(format!("lock poisoned: {e}")))
}
