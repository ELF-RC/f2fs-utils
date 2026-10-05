//! SIT (segment info table) 管理器。

use crate::f2fs::consts::F2FS_BLKSIZE;
use crate::f2fs::error::{F2fsError, Result};
use crate::f2fs::write::consts::{SIT_ENTRY_PER_BLOCK_W, SIT_ENTRY_SIZE};
use crate::f2fs::write::types::SitEntry;

#[derive(Debug)]
pub struct SitManager {
    entries: Vec<SitEntry>,
    sit_blkaddr: u32,
    blocks_per_seg: u32,
    main_blkaddr: u32,
}

impl SitManager {
    pub fn new(segment_count: u32, sit_blkaddr: u32, main_blkaddr: u32) -> Self {
        let entries = vec![SitEntry::default(); segment_count as usize];
        Self {
            entries,
            sit_blkaddr,
            blocks_per_seg: crate::f2fs::write::consts::DEFAULT_BLOCKS_PER_SEGMENT,
            main_blkaddr,
        }
    }

    fn segno(&self, blkaddr: u32) -> Result<u32> {
        if blkaddr < self.main_blkaddr {
            return Err(F2fsError::InvalidData(format!(
                "invalid blkaddr: {blkaddr}"
            )));
        }
        Ok((blkaddr - self.main_blkaddr) / self.blocks_per_seg)
    }

    fn blkoff(&self, blkaddr: u32) -> u32 {
        (blkaddr - self.main_blkaddr) % self.blocks_per_seg
    }

    pub fn mark_block_used(&mut self, blkaddr: u32, seg_type: u16) -> Result<()> {
        let segno = self.segno(blkaddr)? as usize;
        if segno >= self.entries.len() {
            return Err(F2fsError::InvalidData(format!(
                "segno out of range: {segno}"
            )));
        }
        let blkoff = self.blkoff(blkaddr) as usize;
        let entry = &mut self.entries[segno];
        entry.mark_block_valid(blkoff);
        entry.set_vblocks(entry.valid_blocks() + 1, seg_type);
        Ok(())
    }

    pub fn set_seg_type(&mut self, segno: u32, seg_type: u16) -> Result<()> {
        if segno as usize >= self.entries.len() {
            return Err(F2fsError::InvalidData(format!(
                "segno out of range: {segno}"
            )));
        }
        let entry = &mut self.entries[segno as usize];
        entry.set_vblocks(entry.valid_blocks(), seg_type);
        Ok(())
    }

    pub fn get_entry(&self, segno: u32) -> Option<&SitEntry> {
        self.entries.get(segno as usize)
    }

    pub fn segment_count(&self) -> u32 {
        self.entries.len() as u32
    }

    pub fn sit_blkaddr(&self) -> u32 {
        self.sit_blkaddr
    }

    fn sit_blocks_needed(&self) -> u32 {
        let per = F2FS_BLKSIZE / SIT_ENTRY_SIZE;
        (self.entries.len() as u32).div_ceil(per as u32)
    }

    /// 序列化 SIT 区域。
    pub fn to_bytes(&self) -> Vec<u8> {
        let per = F2FS_BLKSIZE / SIT_ENTRY_SIZE;
        let blocks = self.sit_blocks_needed() as usize;
        let mut data = vec![0u8; blocks * F2FS_BLKSIZE];
        for (i, entry) in self.entries.iter().enumerate() {
            let block_idx = i / per;
            let entry_idx = i % per;
            let off = block_idx * F2FS_BLKSIZE + entry_idx * SIT_ENTRY_SIZE;
            let bytes = entry.to_bytes();
            data[off..off + SIT_ENTRY_SIZE].copy_from_slice(&bytes);
        }
        let _ = SIT_ENTRY_PER_BLOCK_W;
        data
    }

    /// SIT 版本 bitmap (标记哪些 SIT 块有效, 供 checkpoint 使用)。
    pub fn version_bitmap(&self) -> Vec<u8> {
        let needed = self.sit_blocks_needed();
        let size = (needed as usize).div_ceil(8);
        let mut bm = vec![0u8; size];
        for i in 0..needed {
            bm[i as usize / 8] |= 1 << (i % 8);
        }
        bm
    }
}
