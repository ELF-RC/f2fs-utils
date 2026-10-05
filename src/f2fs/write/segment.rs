//! Segment 分配器: 管理 6 类 current segment 的块分配。

use crate::f2fs::error::{F2fsError, Result};
use crate::f2fs::write::consts::{DEFAULT_BLOCKS_PER_SEGMENT, NR_CURSEG_TYPE};
use crate::f2fs::write::types::{CursegInfo, SegType};
use std::collections::HashSet;

#[derive(Debug)]
pub struct SegmentAllocator {
    current_segments: [u32; NR_CURSEG_TYPE],
    next_blkoff: [u16; NR_CURSEG_TYPE],
    main_blkaddr: u32,
    blocks_per_seg: u32,
    total_segments: u32,
    allocated_blocks: u64,
    used_segments: HashSet<u32>,
}

impl SegmentAllocator {
    pub fn new(main_blkaddr: u32, total_segments: u32) -> Self {
        let mut alloc = Self {
            current_segments: [0; NR_CURSEG_TYPE],
            next_blkoff: [0; NR_CURSEG_TYPE],
            main_blkaddr,
            blocks_per_seg: DEFAULT_BLOCKS_PER_SEGMENT,
            total_segments,
            allocated_blocks: 0,
            used_segments: HashSet::new(),
        };
        // 各类型分配不同起始 segment (0..6)
        for i in 0..NR_CURSEG_TYPE {
            alloc.current_segments[i] = i as u32;
            alloc.used_segments.insert(i as u32);
        }
        alloc
    }

    pub fn alloc_data_block(&mut self, seg_type: SegType) -> Result<u32> {
        assert!(seg_type.is_data(), "non-data seg_type for data alloc");
        self.alloc_block(seg_type)
    }

    pub fn alloc_node_block(&mut self, seg_type: SegType) -> Result<u32> {
        assert!(seg_type.is_node(), "non-node seg_type for node alloc");
        self.alloc_block(seg_type)
    }

    fn alloc_block(&mut self, seg_type: SegType) -> Result<u32> {
        let idx = seg_type.curseg_index();
        if u32::from(self.next_blkoff[idx]) >= self.blocks_per_seg {
            self.allocate_new_segment(seg_type)?;
        }
        let segno = self.current_segments[idx];
        let blkoff = self.next_blkoff[idx];
        let blkaddr = self.main_blkaddr + segno * self.blocks_per_seg + u32::from(blkoff);
        self.next_blkoff[idx] += 1;
        self.allocated_blocks += 1;
        // 分配恰好填满一段后立即预切到下一段, 保证 get_curseg_info
        // 快照的 next_blkoff < blocks_per_seg (fsck sanity_check_ckpt 要求)
        if u32::from(self.next_blkoff[idx]) >= self.blocks_per_seg {
            self.allocate_new_segment(seg_type)?;
        }
        Ok(blkaddr)
    }

    /// 段满时把当前段标记 used 并推进到下一段。
    /// 在 alloc 入口检查, 保证任意时刻 next_blkoff < blocks_per_seg。
    fn allocate_new_segment(&mut self, seg_type: SegType) -> Result<()> {
        let idx = seg_type.curseg_index();
        // 当前段已用满, 标记为 used (free_segments 计算需要)
        self.used_segments.insert(self.current_segments[idx]);
        let mut next = self.current_segments[idx] + 1;
        loop {
            if next >= self.total_segments {
                return Err(F2fsError::InvalidData("no available segment".into()));
            }
            if !self.used_segments.contains(&next) {
                break;
            }
            next += 1;
        }
        self.current_segments[idx] = next;
        self.next_blkoff[idx] = 0;
        self.used_segments.insert(next);
        Ok(())
    }

    pub fn allocated_blocks(&self) -> u64 {
        self.allocated_blocks
    }

    pub fn free_segments(&self) -> u32 {
        self.total_segments
            .saturating_sub(self.used_segments.len() as u32)
    }

    pub fn get_curseg_info(&self) -> CursegInfo {
        CursegInfo {
            node_segno: [
                self.current_segments[crate::f2fs::write::consts::CURSEG_HOT_NODE],
                self.current_segments[crate::f2fs::write::consts::CURSEG_WARM_NODE],
                self.current_segments[crate::f2fs::write::consts::CURSEG_COLD_NODE],
            ],
            node_blkoff: [
                self.next_blkoff[crate::f2fs::write::consts::CURSEG_HOT_NODE],
                self.next_blkoff[crate::f2fs::write::consts::CURSEG_WARM_NODE],
                self.next_blkoff[crate::f2fs::write::consts::CURSEG_COLD_NODE],
            ],
            data_segno: [
                self.current_segments[crate::f2fs::write::consts::CURSEG_HOT_DATA],
                self.current_segments[crate::f2fs::write::consts::CURSEG_WARM_DATA],
                self.current_segments[crate::f2fs::write::consts::CURSEG_COLD_DATA],
            ],
            data_blkoff: [
                self.next_blkoff[crate::f2fs::write::consts::CURSEG_HOT_DATA],
                self.next_blkoff[crate::f2fs::write::consts::CURSEG_WARM_DATA],
                self.next_blkoff[crate::f2fs::write::consts::CURSEG_COLD_DATA],
            ],
        }
    }
}
