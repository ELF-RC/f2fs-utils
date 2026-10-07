//! SSA (segment summary area) 管理器。

use crate::f2fs::consts::F2FS_BLKSIZE;
use crate::f2fs::error::Result;
use crate::f2fs::write::consts::{
    DEFAULT_BLOCKS_PER_SEGMENT, SUM_FOOTER_SIZE, SUM_TYPE_DATA, SUM_TYPE_NODE, SUMMARY_SIZE,
};
use crate::f2fs::write::types::Summary;

const ENTRIES_IN_SUM: usize = F2FS_BLKSIZE / 8;

#[derive(Debug)]
pub struct SsaManager {
    summaries: Vec<Vec<Summary>>,
    seg_types: Vec<u8>,
    ssa_blkaddr: u32,
    blocks_per_seg: u32,
    main_blkaddr: u32,
}

impl SsaManager {
    pub fn new(segment_count: u32, ssa_blkaddr: u32, main_blkaddr: u32) -> Self {
        let summaries = vec![
            vec![Summary::default(); DEFAULT_BLOCKS_PER_SEGMENT as usize];
            segment_count as usize
        ];
        let seg_types = vec![SUM_TYPE_DATA; segment_count as usize];
        Self {
            summaries,
            seg_types,
            ssa_blkaddr,
            blocks_per_seg: DEFAULT_BLOCKS_PER_SEGMENT,
            main_blkaddr,
        }
    }

    fn segno(&self, blkaddr: u32) -> Option<u32> {
        if blkaddr < self.main_blkaddr {
            return None;
        }
        Some((blkaddr - self.main_blkaddr) / self.blocks_per_seg)
    }

    fn blkoff(&self, blkaddr: u32) -> u32 {
        (blkaddr - self.main_blkaddr) % self.blocks_per_seg
    }

    pub fn set_data_summary(&mut self, blkaddr: u32, nid: u32, ofs_in_node: u16) -> Result<()> {
        if let Some(segno) = self.segno(blkaddr) {
            let blkoff = self.blkoff(blkaddr) as usize;
            if (segno as usize) < self.summaries.len() {
                self.summaries[segno as usize][blkoff] = Summary {
                    nid,
                    version: 0,
                    ofs_in_node,
                };
                self.seg_types[segno as usize] = SUM_TYPE_DATA;
            }
        }
        Ok(())
    }

    pub fn set_node_summary(&mut self, blkaddr: u32, nid: u32) -> Result<()> {
        if let Some(segno) = self.segno(blkaddr) {
            let blkoff = self.blkoff(blkaddr) as usize;
            if (segno as usize) < self.summaries.len() {
                self.summaries[segno as usize][blkoff] = Summary {
                    nid,
                    version: 0,
                    ofs_in_node: 0,
                };
                self.seg_types[segno as usize] = SUM_TYPE_NODE;
            }
        }
        Ok(())
    }

    pub fn get_summary_entry(&self, segno: usize, blkoff: usize) -> Option<&Summary> {
        self.summaries.get(segno).and_then(|s| s.get(blkoff))
    }

    pub fn ssa_blkaddr(&self) -> u32 {
        self.ssa_blkaddr
    }

    /// 构建单个 segment 的 summary 块。
    fn build_summary_block(&self, segno: usize) -> [u8; F2FS_BLKSIZE] {
        let mut buf = [0u8; F2FS_BLKSIZE];
        let entries = &self.summaries[segno];
        for (i, entry) in entries.iter().take(ENTRIES_IN_SUM).enumerate() {
            let off = i * SUMMARY_SIZE;
            buf[off..off + SUMMARY_SIZE].copy_from_slice(&entry.to_bytes());
        }
        // 官方 mkfs 对所有 SSA summary block 只写 footer_type, crc 全零
        // (内核不校验 SSA summary crc)。seg_types 记录每段类型 (DATA/NODE)。
        let footer = F2FS_BLKSIZE - SUM_FOOTER_SIZE;
        buf[footer] = self.seg_types[segno];
        buf
    }

    /// 为 checkpoint pack 构建当前 segment 的 summary 块。
    pub fn build_curseg_summary(&self, segno: usize, is_node: bool) -> [u8; F2FS_BLKSIZE] {
        let mut buf = [0u8; F2FS_BLKSIZE];
        if segno < self.summaries.len() {
            let entries = &self.summaries[segno];
            for (i, entry) in entries.iter().take(ENTRIES_IN_SUM).enumerate() {
                let off = i * SUMMARY_SIZE;
                buf[off..off + SUMMARY_SIZE].copy_from_slice(&entry.to_bytes());
            }
        }
        // 官方 mkfs 对 CP pack summary 块也只写 footer_type, crc 全零
        let footer = F2FS_BLKSIZE - SUM_FOOTER_SIZE;
        buf[footer] = if is_node {
            SUM_TYPE_NODE
        } else {
            SUM_TYPE_DATA
        };
        buf
    }

    /// 序列化 SSA 区域。
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(self.summaries.len() * F2FS_BLKSIZE);
        for segno in 0..self.summaries.len() {
            data.extend_from_slice(&self.build_summary_block(segno));
        }
        data
    }
}
