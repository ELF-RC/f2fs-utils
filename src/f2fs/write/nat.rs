//! NAT (node address table) 管理器。

use crate::f2fs::consts::{F2FS_BLKSIZE, F2FS_ROOT_INO, NAT_ENTRY_SIZE};
use crate::f2fs::types::{Block, Nid};
use crate::f2fs::write::consts::{
    F2FS_FIRST_INO, F2FS_META_INO, F2FS_NODE_INO, NAT_ENTRY_PER_BLOCK_W,
};
use crate::f2fs::write::types::NatEntryW;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct NatManager {
    entries: BTreeMap<u32, NatEntryW>,
    next_nid: u32,
    #[allow(dead_code)]
    nat_blkaddr: u32,
}

impl NatManager {
    pub fn new(nat_blkaddr: u32) -> Self {
        Self {
            entries: BTreeMap::new(),
            next_nid: F2FS_FIRST_INO,
            nat_blkaddr,
        }
    }

    pub fn alloc_nid(&mut self) -> Nid {
        let nid = self.next_nid;
        self.next_nid += 1;
        Nid(nid)
    }

    pub fn next_free_nid(&self) -> u32 {
        self.next_nid
    }

    pub fn set_entry(&mut self, nid: Nid, block_addr: u32, ino: u32) {
        self.entries.insert(
            nid.0,
            NatEntryW {
                version: 0,
                ino,
                block_addr: Block(block_addr),
            },
        );
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 初始化保留 inode: node_ino / meta_ino (block_addr=1 特殊标记) 与 root_ino。
    pub fn init_reserved_inodes(&mut self, root_blkaddr: u32) {
        // F2FS_NODE_INO(1) / F2FS_META_INO(2) 是内核元数据 inode,
        // 无对应数据块; block_addr 用 NULL_ADDR(0), 否则 fsck
        // f2fs_init_nid_bitmap 校验 "addr is invalid"。
        self.entries.insert(
            F2FS_NODE_INO,
            NatEntryW {
                version: 0,
                ino: F2FS_NODE_INO,
                block_addr: Block(crate::f2fs::consts::NULL_ADDR),
            },
        );
        self.entries.insert(
            F2FS_META_INO,
            NatEntryW {
                version: 0,
                ino: F2FS_META_INO,
                block_addr: Block(crate::f2fs::consts::NULL_ADDR),
            },
        );
        self.entries.insert(
            F2FS_ROOT_INO,
            NatEntryW {
                version: 0,
                ino: F2FS_ROOT_INO,
                block_addr: Block(root_blkaddr),
            },
        );
    }

    fn nat_blocks_needed(&self) -> u32 {
        self.next_nid.div_ceil(NAT_ENTRY_PER_BLOCK_W as u32)
    }

    /// 返回所有已分配的 NAT 条目 (nid, ino, block_addr), 供 compact summary 的 NAT journal 使用。
    pub fn journal_entries(&self) -> Vec<(u32, u32, u32)> {
        self.entries
            .iter()
            .map(|(&nid, e)| (nid, e.ino, e.block_addr.0))
            .collect()
    }

    /// 序列化 NAT 区域为字节数据。
    pub fn to_bytes(&self) -> Vec<u8> {
        let blocks = self.nat_blocks_needed() as usize;
        let mut data = vec![0u8; blocks * F2FS_BLKSIZE];
        for (&nid, entry) in &self.entries {
            let block_idx = nid as usize / NAT_ENTRY_PER_BLOCK_W;
            let entry_idx = nid as usize % NAT_ENTRY_PER_BLOCK_W;
            if block_idx < blocks {
                let off = block_idx * F2FS_BLKSIZE + entry_idx * NAT_ENTRY_SIZE;
                let bytes = entry.to_bytes();
                data[off..off + NAT_ENTRY_SIZE].copy_from_slice(&bytes);
            }
        }
        data
    }
}
