//! 超级块构建器: 布局计算与序列化。

use crate::f2fs::consts::{F2FS_MAGIC, F2FS_ROOT_INO};
use crate::f2fs::error::{F2fsError, Result};
use crate::f2fs::write::consts::*;
use crate::f2fs::write::crc::crc32;
use crate::f2fs::write::types::{F2fsFeatures, SuperblockLayout};

fn log2_u32(val: u32) -> u32 {
    if val == 0 { 0 } else { val.ilog2() }
}

/// 超级块构建器。
#[derive(Debug)]
pub struct SuperblockBuilder {
    image_size: u64,
    block_size: u32,
    sector_size: u32,
    blocks_per_seg: u32,
    segs_per_sec: u32,
    secs_per_zone: u32,
    features: F2fsFeatures,
    volume_label: String,
    uuid: [u8; 16],
    layout: Option<SuperblockLayout>,
}

impl SuperblockBuilder {
    pub fn new(image_size: u64) -> Self {
        Self {
            image_size,
            block_size: crate::f2fs::consts::F2FS_BLKSIZE as u32,
            sector_size: DEFAULT_SECTOR_SIZE,
            blocks_per_seg: DEFAULT_BLOCKS_PER_SEGMENT,
            segs_per_sec: DEFAULT_SEGMENTS_PER_SECTION,
            secs_per_zone: DEFAULT_SECTIONS_PER_ZONE,
            features: F2fsFeatures::default(),
            volume_label: String::new(),
            uuid: [0u8; 16],
            layout: None,
        }
    }

    pub fn with_features(mut self, features: F2fsFeatures) -> Self {
        self.features = features;
        self
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.volume_label = label.to_string();
        self
    }

    pub fn with_uuid(mut self, uuid: [u8; 16]) -> Self {
        self.uuid = uuid;
        self
    }

    /// 计算全盘布局: 各区段起始块地址与 segment 计数。
    pub fn calculate_layout(&mut self) -> Result<&SuperblockLayout> {
        let log_sectorsize = log2_u32(self.sector_size);
        let log_sectors_per_block = log2_u32(self.block_size / self.sector_size);
        let log_blocksize = log_sectorsize + log_sectors_per_block;
        let log_blks_per_seg = log2_u32(self.blocks_per_seg);

        let blk_size_bytes = 1u64 << log_blocksize;
        let segment_size_bytes = blk_size_bytes * u64::from(self.blocks_per_seg);
        let zone_size_bytes = blk_size_bytes
            * u64::from(self.secs_per_zone)
            * u64::from(self.segs_per_sec)
            * u64::from(self.blocks_per_seg);

        // 总块数
        let total_sectors = self.image_size / u64::from(self.sector_size);
        let block_count = total_sectors >> log_sectors_per_block;

        // segment0 起始: zone 对齐
        let zone_align_start_offset = (2 * crate::f2fs::consts::F2FS_BLKSIZE as u64)
            .div_ceil(zone_size_bytes)
            * zone_size_bytes;
        let segment0_blkaddr = (zone_align_start_offset / blk_size_bytes) as u32;
        let cp_blkaddr = segment0_blkaddr;

        let usable_bytes = self.image_size.checked_sub(zone_align_start_offset).ok_or_else(|| {
            F2fsError::InvalidData(format!(
                "image too small: {0} bytes, requires more than {zone_align_start_offset} bytes for zone alignment",
                self.image_size
            ))
        })?;

        let total_segments =
            (usable_bytes / segment_size_bytes) as u32 / self.segs_per_sec * self.segs_per_sec;

        if total_segments < F2FS_MIN_SEGMENTS {
            return Err(F2fsError::InvalidData(format!(
                "image too small: requires at least {F2FS_MIN_SEGMENTS} segments, only {total_segments} available"
            )));
        }

        let segment_count_ckpt = F2FS_NUMBER_OF_CHECKPOINT_PACK;
        let sit_blkaddr = segment0_blkaddr + segment_count_ckpt * self.blocks_per_seg;

        // SIT segment 数 (双份)
        let blocks_for_sit = total_segments.div_ceil(SIT_ENTRY_PER_BLOCK_W as u32);
        let sit_segments = blocks_for_sit.div_ceil(self.blocks_per_seg);
        let segment_count_sit = sit_segments * 2;

        // NAT segment 数 (双份)
        let nat_blkaddr = sit_blkaddr + segment_count_sit * self.blocks_per_seg;
        let total_valid_after_sit =
            (total_segments - segment_count_ckpt - segment_count_sit) * self.blocks_per_seg;
        let blocks_for_nat = total_valid_after_sit.div_ceil(NAT_ENTRY_PER_BLOCK_W as u32);
        let nat_segments = blocks_for_nat.div_ceil(self.blocks_per_seg);
        let segment_count_nat = nat_segments * 2;

        // SSA segment 数
        let ssa_blkaddr = nat_blkaddr + segment_count_nat * self.blocks_per_seg;
        let total_valid_after_nat =
            (total_segments - segment_count_ckpt - segment_count_sit - segment_count_nat)
                * self.blocks_per_seg;
        let blocks_for_ssa = total_valid_after_nat / self.blocks_per_seg + 1;
        let mut segment_count_ssa = blocks_for_ssa.div_ceil(self.blocks_per_seg);

        // 元数据 segment 总数对齐到 zone 边界
        let total_meta =
            segment_count_ckpt + segment_count_sit + segment_count_nat + segment_count_ssa;
        let diff = total_meta % self.segs_per_sec;
        if diff != 0 {
            segment_count_ssa += self.segs_per_sec - diff;
        }
        let total_meta =
            segment_count_ckpt + segment_count_sit + segment_count_nat + segment_count_ssa;

        let main_blkaddr = segment0_blkaddr + total_meta * self.blocks_per_seg;

        let total_zones = total_segments / self.segs_per_sec - total_meta / self.segs_per_sec;
        let section_count = total_zones * self.secs_per_zone;
        let segment_count_main = section_count * self.segs_per_sec;

        // cp_payload: 当 SIT bitmap 放不进 checkpoint 时需要的额外块
        let sit_bitmap_size = (segment_count_sit / 2) << log_blks_per_seg;
        let max_sit_bitmap =
            sit_bitmap_size.min((CP_CHKSUM_OFFSET - CHECKPOINT_HEADER_SIZE) as u32);
        let cp_payload = if max_sit_bitmap > (CP_CHKSUM_OFFSET - CHECKPOINT_HEADER_SIZE) as u32 {
            max_sit_bitmap.div_ceil(crate::f2fs::consts::F2FS_BLKSIZE as u32)
        } else {
            0
        };

        self.layout = Some(SuperblockLayout {
            segment0_blkaddr,
            cp_blkaddr,
            sit_blkaddr,
            nat_blkaddr,
            ssa_blkaddr,
            main_blkaddr,
            segment_count: total_segments,
            segment_count_ckpt,
            segment_count_sit,
            segment_count_nat,
            segment_count_ssa,
            segment_count_main,
            section_count,
            block_count,
            cp_payload,
        });
        self.layout
            .as_ref()
            .ok_or_else(|| F2fsError::InvalidData("layout calculation failed".into()))
    }

    pub fn layout(&self) -> Option<&SuperblockLayout> {
        self.layout.as_ref()
    }

    /// 序列化超级块为 3072 字节缓冲。
    pub fn build(&self) -> Result<[u8; SUPERBLOCK_SIZE]> {
        let layout = self
            .layout
            .as_ref()
            .ok_or_else(|| F2fsError::InvalidData("layout not calculated".into()))?;

        let mut buf = [0u8; SUPERBLOCK_SIZE];
        let log_sectorsize = log2_u32(self.sector_size);
        let log_sectors_per_block = log2_u32(self.block_size / self.sector_size);
        let log_blocksize = log_sectorsize + log_sectors_per_block;
        let log_blks_per_seg = log2_u32(self.blocks_per_seg);

        buf[..4].copy_from_slice(&F2FS_MAGIC.to_le_bytes());
        buf[4..6].copy_from_slice(&F2FS_MAJOR_VERSION.to_le_bytes());
        buf[6..8].copy_from_slice(&F2FS_MINOR_VERSION.to_le_bytes());
        buf[8..12].copy_from_slice(&log_sectorsize.to_le_bytes());
        buf[12..16].copy_from_slice(&log_sectors_per_block.to_le_bytes());
        buf[16..20].copy_from_slice(&log_blocksize.to_le_bytes());
        buf[20..24].copy_from_slice(&log_blks_per_seg.to_le_bytes());
        buf[24..28].copy_from_slice(&self.segs_per_sec.to_le_bytes());
        buf[28..32].copy_from_slice(&self.secs_per_zone.to_le_bytes());

        let checksum_offset = if self.features.sb_chksum() {
            SB_CHKSUM_OFFSET as u32
        } else {
            0
        };
        buf[32..36].copy_from_slice(&checksum_offset.to_le_bytes());

        buf[36..44].copy_from_slice(&layout.block_count.to_le_bytes());
        buf[44..48].copy_from_slice(&layout.section_count.to_le_bytes());
        buf[48..52].copy_from_slice(&layout.segment_count.to_le_bytes());
        buf[52..56].copy_from_slice(&layout.segment_count_ckpt.to_le_bytes());
        buf[56..60].copy_from_slice(&layout.segment_count_sit.to_le_bytes());
        buf[60..64].copy_from_slice(&layout.segment_count_nat.to_le_bytes());
        buf[64..68].copy_from_slice(&layout.segment_count_ssa.to_le_bytes());
        buf[68..72].copy_from_slice(&layout.segment_count_main.to_le_bytes());
        buf[72..76].copy_from_slice(&layout.segment0_blkaddr.to_le_bytes());
        buf[76..80].copy_from_slice(&layout.cp_blkaddr.to_le_bytes());
        buf[80..84].copy_from_slice(&layout.sit_blkaddr.to_le_bytes());
        buf[84..88].copy_from_slice(&layout.nat_blkaddr.to_le_bytes());
        buf[88..92].copy_from_slice(&layout.ssa_blkaddr.to_le_bytes());
        buf[92..96].copy_from_slice(&layout.main_blkaddr.to_le_bytes());
        buf[96..100].copy_from_slice(&F2FS_ROOT_INO.to_le_bytes());
        buf[100..104].copy_from_slice(&F2FS_NODE_INO.to_le_bytes());
        buf[104..108].copy_from_slice(&F2FS_META_INO.to_le_bytes());
        buf[108..124].copy_from_slice(&self.uuid);

        // 卷标 (UTF-16, 偏移 124, 共 MAX_VOLUME_NAME 字节)
        let vol: Vec<u16> = self.volume_label.encode_utf16().collect();
        for (i, &ch) in vol.iter().take(MAX_VOLUME_NAME / 2).enumerate() {
            let off = 124 + i * 2;
            buf[off..off + 2].copy_from_slice(&ch.to_le_bytes());
        }

        buf[1664..1668].copy_from_slice(&layout.cp_payload.to_le_bytes());

        let vlen = F2FS_VERSION.len().min(VERSION_LEN);
        buf[1668..1668 + vlen].copy_from_slice(&F2FS_VERSION[..vlen]);
        buf[1924..1924 + vlen].copy_from_slice(&F2FS_VERSION[..vlen]);

        buf[2180..2184].copy_from_slice(&self.features.to_bits().to_le_bytes());

        if self.features.sb_chksum() {
            let crc = crc32(&buf[..SB_CHKSUM_OFFSET]);
            buf[SB_CHKSUM_OFFSET..SB_CHKSUM_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
        }

        Ok(buf)
    }
}
