//! F2FS 镜像构建编排器: 整合 superblock/NAT/SIT/SSA/segment,
//! 递归装载源目录树, 生成完整可挂载的 F2FS 镜像。
//!
//! 复刻 AOSP `mkfs.f2fs -g android` + `sload.f2fs -f -C -s -t` 的一站式流程:
//! 1. 计算布局、初始化元数据管理器
//! 2. 创建根目录 inode
//! 3. 递归装载源目录 (目录/文件/符号链接), 应用 fs_config 与 SELinux 上下文
//! 4. 写入 NAT / SIT / SSA 区域
//! 5. 写入双副本 checkpoint (含 compact summary)
//! 6. 写入超级块

use crate::f2fs::consts::{COMPRESS_ADDR, F2FS_BLKSIZE, F2FS_ROOT_INO, NULL_ADDR};
use crate::f2fs::types::Nid;
use crate::f2fs::write::checkpoint::{CheckpointBuilder, NAT_JOURNAL_ENTRY_SIZE, SUM_JOURNAL_SIZE};
use crate::f2fs::write::config::{FsConfig, SelinuxContexts};
use crate::f2fs::write::consts::{
    COMPRESS_HEADER_SIZE, CP_CHKSUM_OFFSET, CP_COMPACT_SUM_FLAG_W, CP_NAT_BITS_FLAG,
    CP_UMOUNT_FLAG, DEFAULT_BLOCKS_PER_SEGMENT, MAX_INLINE_DATA_SIZE, NR_CURSEG_TYPE,
};
use crate::f2fs::write::dentry::{DentryBlockBuilder, DentryInfo};
use crate::f2fs::write::inode::InodeBuilder;
use crate::f2fs::write::nat::NatManager;
use crate::f2fs::write::segment::SegmentAllocator;
use crate::f2fs::write::sit::SitManager;
use crate::f2fs::write::ssa::SsaManager;
use crate::f2fs::write::superblock::SuperblockBuilder;
use crate::f2fs::write::types::FileType;
use crate::f2fs::write::types::{CursegInfo, F2fsFeatures, SegType, SuperblockLayout};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 构建配置。
pub struct MkfsConfig {
    /// 源目录 (要装载的文件树根)。
    pub source_dir: PathBuf,
    /// 输出镜像路径。
    pub output_path: PathBuf,
    /// 镜像大小 (字节)。
    pub image_size: u64,
    /// 挂载点 (如 "/system", 用于 fs_config / file_contexts 查询的前缀)。
    pub mount_point: String,
    /// 卷标。
    pub label: String,
    /// fs_config 文件路径 (可选)。
    pub fs_config: Option<PathBuf>,
    /// file_contexts 文件路径 (可选)。
    pub file_contexts: Option<PathBuf>,
    /// 固定时间戳 (可选, AOSP 用 2009-01-01)。
    pub timestamp: Option<u64>,
    /// 特性标志 (None 用默认)。
    pub features: Option<F2fsFeatures>,
    /// 构建完成后转为 Android sparse 镜像。
    pub sparse: bool,
    /// 启用文件压缩 (配合 features 的 COMPRESSION 位)。
    pub compression: bool,
    /// 压缩算法: COMPRESS_LZ4 等。
    pub compress_algo: u8,
    /// log2(压缩簇块数), 默认 2 = 4 块 = 16KB。
    pub cluster_log: u8,
}

/// 待写入的目录 (延迟写 inode: 子项装载完毕后才知道 links/size)。
struct PendingDir {
    nid: u32,
    pino: u32,
    name: Vec<u8>,
    fs_path: String,
    dentries: Vec<DentryInfo>,
    data_blkaddr: u32,
    is_root: bool,
    /// 目录自身的修改时间 (秒), 用于重写 inode 时还原时间戳。
    mtime_secs: u64,
    mtime_nsecs: u32,
    /// 目录深度 (根=0), 写入 inode.i_current_depth。
    depth: u32,
}

/// F2FS 镜像构建器。
pub struct F2fsBuilder {
    cfg: MkfsConfig,
    writer: File,
    superblock_builder: SuperblockBuilder,
    layout: SuperblockLayout,
    nat: NatManager,
    sit: SitManager,
    ssa: SsaManager,
    segalloc: SegmentAllocator,
    cp_ver: u64,
    /// 镜像构建时间 (默认当前, 或 -T 指定值); 用于根 inode 与 checkpoint。
    mkfs_time: u64,
    /// 固定时间戳 (仅当用户指定 -T 时); 文件 inode 全部使用该值。
    /// None 表示用源文件实际 mtime。
    fixed_time: Option<u64>,
    fs_config: Option<FsConfig>,
    selinux: Option<SelinuxContexts>,
    /// nid → 已写入的 inode 块地址 (用于后续更新 inode)。
    inode_blocks: HashMap<u32, u32>,
    /// 待写目录栈 (深度优先)。
    pending_dirs: Vec<PendingDir>,
    /// 有效块计数。
    valid_block_count: u64,
    /// 有效 node 计数。
    valid_node_count: u32,
    /// 有效 inode 计数。
    valid_inode_count: u32,
    /// 是否启用文件压缩。
    compression: bool,
    /// 压缩算法 (COMPRESS_LZ4 等)。
    compress_algo: u8,
    /// log2(压缩簇块数)。
    cluster_log: u8,
}

impl F2fsBuilder {
    pub fn new(cfg: MkfsConfig) -> Result<Self> {
        let file = File::create(&cfg.output_path)?;
        // -T 同时控制 mkfs_time 与文件时间戳 (AOSP build_image.py 行为)
        let fixed_time = cfg.timestamp;
        let mkfs_time = cfg.timestamp.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        });

        let features = cfg.features.unwrap_or_default();
        let mut sb_builder = SuperblockBuilder::new(cfg.image_size)
            .with_features(features)
            .with_label(&cfg.label);
        let layout = sb_builder.calculate_layout()?.clone();

        let nat = NatManager::new(layout.nat_blkaddr);
        let sit = SitManager::new(
            layout.segment_count_main,
            layout.sit_blkaddr,
            layout.main_blkaddr,
        );
        let ssa = SsaManager::new(
            layout.segment_count_main,
            layout.ssa_blkaddr,
            layout.main_blkaddr,
        );
        let segalloc = SegmentAllocator::new(layout.main_blkaddr, layout.segment_count_main);

        let fs_config = cfg
            .fs_config
            .as_ref()
            .and_then(|p| FsConfig::from_file(p).ok());
        let selinux = cfg
            .file_contexts
            .as_ref()
            .and_then(|p| SelinuxContexts::from_file(p).ok());

        let compression = cfg.compression;
        let compress_algo = cfg.compress_algo;
        let cluster_log = cfg.cluster_log;

        Ok(Self {
            cfg,
            writer: file,
            superblock_builder: sb_builder,
            layout,
            nat,
            sit,
            ssa,
            segalloc,
            cp_ver: 1,
            mkfs_time,
            fixed_time,
            fs_config,
            selinux,
            inode_blocks: HashMap::new(),
            pending_dirs: Vec::new(),
            valid_block_count: 0,
            valid_node_count: 0,
            valid_inode_count: 0,
            compression,
            compress_algo,
            cluster_log,
        })
    }

    /// 构建镜像。
    pub fn build(&mut self) -> Result<()> {
        // 预分配镜像文件大小
        self.writer.set_len(self.cfg.image_size)?;

        // 1. 创建根目录
        self.create_root_dir()?;

        // 2. 递归装载源目录
        if self.cfg.source_dir.exists() {
            self.load_directory(
                &self.cfg.source_dir.clone(),
                F2FS_ROOT_INO,
                &self.cfg.mount_point.clone(),
                0,
            )?;
        }

        // 3. 写入所有待写目录的 inode (此时已知 links / dentries)
        self.flush_pending_dirs()?;

        // 4. 写入元数据区域
        self.write_nat_area()?;
        self.finalize_curseg_sit_types();
        self.write_sit_area()?;
        self.write_ssa_area()?;

        // 5. 写入 checkpoint (双副本)
        self.write_checkpoint()?;

        // 6. 写入超级块
        self.write_superblock()?;

        self.writer.flush()?;
        Ok(())
    }

    /// 创建根目录: 分配 inode 块 + 默认 dentry block (含 . 和 ..)。
    fn create_root_dir(&mut self) -> Result<()> {
        // 根 inode 的 node 块: 从 HOT_NODE segment 分配
        let root_blkaddr = self.segalloc.alloc_node_block(SegType::HotNode)?;
        self.nat.init_reserved_inodes(root_blkaddr);
        self.mark_node_block(root_blkaddr, F2FS_ROOT_INO, SegType::HotNode);

        // 根目录的 dentry block: 从 HOT_DATA segment 分配
        let dentry_blkaddr = self.segalloc.alloc_data_block(SegType::HotData)?;
        self.mark_data_block(dentry_blkaddr, F2FS_ROOT_INO, 0, SegType::HotData);

        // 写入根 inode (mode 040755, uid/gid 0); 根目录时间 = 构建时间
        let root_inode = InodeBuilder::new_dir(0o755, 0, 0)
            .with_links(2)
            .with_size(F2FS_BLKSIZE as u64)
            .with_blocks(2)
            .with_timestamp(self.mkfs_time)
            .with_pino(F2FS_ROOT_INO)
            .with_depth(0)
            .with_name(b"/")
            .with_addrs(vec![dentry_blkaddr]);
        // 应用 SELinux 上下文 (若提供)
        let root_inode = if let Some(ref mut selinux) = self.selinux {
            if let Some(ctx) = selinux.lookup(&self.cfg.mount_point) {
                root_inode.with_selinux_context(&ctx)
            } else {
                root_inode
            }
        } else {
            root_inode
        };
        let root_buf = root_inode.build(F2FS_ROOT_INO, F2FS_ROOT_INO, self.cp_ver);
        self.write_block_at(root_blkaddr, &root_buf)?;
        self.inode_blocks.insert(F2FS_ROOT_INO, root_blkaddr);

        self.valid_node_count += 1;
        self.valid_inode_count += 1;
        self.valid_block_count += 2;

        // 推入根目录到待写栈 (根的 dentry block 将在装载时填充)
        self.pending_dirs.push(PendingDir {
            nid: F2FS_ROOT_INO,
            pino: F2FS_ROOT_INO,
            name: Vec::new(),
            fs_path: self.cfg.mount_point.clone(),
            dentries: Vec::new(),
            data_blkaddr: dentry_blkaddr,
            is_root: true,
            mtime_secs: self.mkfs_time,
            mtime_nsecs: 0,
            depth: 0,
        });

        Ok(())
    }

    /// 递归装载目录: 为每个子项创建 inode, 收集 dentry, 推入待写目录栈。
    fn load_directory(
        &mut self,
        dir_path: &Path,
        parent_nid: u32,
        fs_path: &str,
        depth: u32,
    ) -> Result<()> {
        let entries: Vec<_> = fs::read_dir(dir_path)?.collect::<std::result::Result<_, _>>()?;
        // 排序保证可复现
        let mut entries = entries;
        entries.sort_by_key(std::fs::DirEntry::file_name);

        for entry in entries {
            let name_os = entry.file_name();
            let name = name_os.to_string_lossy();
            let entry_path = entry.path();
            let child_fs_path = if fs_path == "/" {
                format!("/{name}")
            } else {
                format!("{fs_path}/{name}")
            };

            let meta = entry
                .metadata()
                .with_context(|| format!("failed to stat {}", entry_path.display()))?;
            // 时间戳: -T 指定时全部用固定值, 否则读源文件 mtime
            let mtime = self.file_timestamp(&meta);

            if meta.is_dir() {
                let nid = self.nat.alloc_nid().0;
                let (uid, gid, mode) = self.fs_config_attrs(&child_fs_path, true);
                let node_blkaddr = self.segalloc.alloc_node_block(SegType::HotNode)?;
                self.nat.set_entry(Nid(nid), node_blkaddr, nid);
                self.mark_node_block(node_blkaddr, nid, SegType::HotNode);

                // 目录的 dentry block
                let dentry_blkaddr = self.segalloc.alloc_data_block(SegType::HotData)?;
                self.mark_data_block(dentry_blkaddr, nid, 0, SegType::HotData);

                // 先写一个占位 inode (links/size 待子项装载后更新)
                let inode = InodeBuilder::new_dir((mode & 0o7777) as u16, uid, gid)
                    .with_links(2)
                    .with_size(F2FS_BLKSIZE as u64)
                    .with_blocks(2)
                    .with_timestamp_nsecs(mtime.0, mtime.1)
                    .with_pino(parent_nid)
                    .with_depth(depth + 1)
                    .with_name(name.as_bytes())
                    .with_addrs(vec![dentry_blkaddr]);
                let inode = self.apply_selinux(inode, &child_fs_path);
                let buf = inode.build(nid, nid, self.cp_ver);
                self.write_block_at(node_blkaddr, &buf)?;
                self.inode_blocks.insert(nid, node_blkaddr);

                self.valid_node_count += 1;
                self.valid_inode_count += 1;
                self.valid_block_count += 2;

                // 添加到父目录的 dentry 列表
                self.add_dentry_to_current(parent_nid, &name, nid, FileType::Dir);

                // 推入待写目录栈
                self.pending_dirs.push(PendingDir {
                    nid,
                    pino: parent_nid,
                    name: name.as_bytes().to_vec(),
                    fs_path: child_fs_path.clone(),
                    dentries: Vec::new(),
                    data_blkaddr: dentry_blkaddr,
                    is_root: false,
                    mtime_secs: mtime.0,
                    mtime_nsecs: mtime.1,
                    depth: depth + 1,
                });

                // 递归
                self.load_directory(&entry_path, nid, &child_fs_path, depth + 1)?;
            } else if meta.is_file() {
                let nid = self.nat.alloc_nid().0;
                let (uid, gid, mode) = self.fs_config_attrs(&child_fs_path, false);
                let file_size = meta.len();
                let data = fs::read(&entry_path)
                    .with_context(|| format!("failed to read {}", entry_path.display()))?;

                let node_blkaddr = self.segalloc.alloc_node_block(SegType::WarmNode)?;
                self.nat.set_entry(Nid(nid), node_blkaddr, nid);
                self.mark_node_block(node_blkaddr, nid, SegType::WarmNode);
                self.valid_node_count += 1;
                self.valid_inode_count += 1;
                self.valid_block_count += 1;

                // 小文件内联 (不占独立数据块); 大文件分配数据块
                let inode = if data.len() <= MAX_INLINE_DATA_SIZE {
                    InodeBuilder::new_file((mode & 0o7777) as u16, uid, gid)
                        .with_links(1)
                        .with_size(file_size)
                        .with_blocks(1)
                        .with_timestamp_nsecs(mtime.0, mtime.1)
                        .with_pino(parent_nid)
                        .with_name(name.as_bytes())
                        .with_inline_data(data)
                } else if self.compression {
                    let (data_addrs, compr_blocks) = self.write_file_data_compressed(&data, nid)?;
                    let blocks = u64::from(compr_blocks) + 1;
                    InodeBuilder::new_file((mode & 0o7777) as u16, uid, gid)
                        .with_links(1)
                        .with_size(file_size)
                        .with_blocks(blocks)
                        .with_timestamp_nsecs(mtime.0, mtime.1)
                        .with_pino(parent_nid)
                        .with_name(name.as_bytes())
                        .with_addrs(data_addrs)
                        .with_compression(self.compress_algo, self.cluster_log, compr_blocks)
                } else {
                    let data_addrs = self.write_file_data(&data, nid)?;
                    let blocks = data_addrs.len() as u64 + 1;
                    InodeBuilder::new_file((mode & 0o7777) as u16, uid, gid)
                        .with_links(1)
                        .with_size(file_size)
                        .with_blocks(blocks)
                        .with_timestamp_nsecs(mtime.0, mtime.1)
                        .with_pino(parent_nid)
                        .with_name(name.as_bytes())
                        .with_addrs(data_addrs)
                };
                let inode = self.apply_selinux(inode, &child_fs_path);
                let buf = inode.build(nid, nid, self.cp_ver);
                self.write_block_at(node_blkaddr, &buf)?;

                self.add_dentry_to_current(parent_nid, &name, nid, FileType::RegFile);
            } else if meta.file_type().is_symlink() {
                let nid = self.nat.alloc_nid().0;
                let (uid, gid, _) = self.fs_config_attrs(&child_fs_path, false);
                let target = fs::read_link(&entry_path)
                    .with_context(|| format!("failed to readlink {}", entry_path.display()))?;
                let target_str = target.to_string_lossy();

                let node_blkaddr = self.segalloc.alloc_node_block(SegType::WarmNode)?;
                self.nat.set_entry(Nid(nid), node_blkaddr, nid);
                self.mark_node_block(node_blkaddr, nid, SegType::WarmNode);

                let inode = InodeBuilder::new_symlink(uid, gid)
                    .with_links(1)
                    .with_timestamp_nsecs(mtime.0, mtime.1)
                    .with_pino(parent_nid)
                    .with_name(name.as_bytes())
                    .with_symlink_target(&target_str);
                let inode = self.apply_selinux(inode, &child_fs_path);
                let buf = inode.build(nid, nid, self.cp_ver);
                self.write_block_at(node_blkaddr, &buf)?;

                self.valid_node_count += 1;
                self.valid_inode_count += 1;
                self.valid_block_count += 1;
                self.inode_blocks.insert(nid, node_blkaddr);

                self.add_dentry_to_current(parent_nid, &name, nid, FileType::Symlink);
            }
            // 其它类型 (设备/FIFO/套接字) 暂不处理
        }
        Ok(())
    }

    /// 写入文件数据块, 返回块地址列表。
    fn write_file_data(&mut self, data: &[u8], nid: u32) -> Result<Vec<u32>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let mut addrs = Vec::new();
        for chunk in data.chunks(F2FS_BLKSIZE) {
            let blkaddr = self.segalloc.alloc_data_block(SegType::WarmData)?;
            let mut block = vec![0u8; F2FS_BLKSIZE];
            block[..chunk.len()].copy_from_slice(chunk);
            self.write_block_at(blkaddr, &block)?;
            self.mark_data_block(blkaddr, nid, addrs.len() as u16, SegType::WarmData);
            addrs.push(blkaddr);
            self.valid_block_count += 1;
        }
        Ok(addrs)
    }

    /// 写入压缩文件数据块, 返回 (addr 数组, 物理块计数)。
    ///
    /// F2FS 压缩格式 (与读取侧 file.rs::decompress_cluster 对称):
    /// - 簇 = 2^cluster_log 个逻辑块 (默认 4 块 = 16KB)
    /// - LZ4 压缩, 24 字节头 (clen(4)+chksum(4)+reserved(16)) + clen 字节载荷
    /// - 载荷切成 N 个 4KB 物理块
    /// - addr 数组: [COMPRESS_ADDR, phys0..physN, 0...] 填充至簇块数
    /// - 压缩后 >= 原始大小则存为普通块 (不压缩)
    fn write_file_data_compressed(&mut self, data: &[u8], nid: u32) -> Result<(Vec<u32>, u32)> {
        let cluster_blks = 1usize << self.cluster_log;
        let cluster_size = cluster_blks * F2FS_BLKSIZE;
        let mut addrs = Vec::new();
        let mut compr_blocks: u32 = 0;

        for cluster_data in data.chunks(cluster_size) {
            // 不足整簇: 补零到簇大小再压缩
            let mut padded = cluster_data.to_vec();
            padded.resize(cluster_size, 0);

            let compressed = lz4_flex::compress(&padded);
            let header_and_payload_len = COMPRESS_HEADER_SIZE + compressed.len();
            let phys_blks = header_and_payload_len.div_ceil(F2FS_BLKSIZE);

            // 压缩未获益 (物理块数 >= 逻辑块数): 存为普通块
            if phys_blks >= cluster_blks {
                for chunk in padded.chunks(F2FS_BLKSIZE).take(cluster_blks) {
                    let blkaddr = self.segalloc.alloc_data_block(SegType::WarmData)?;
                    self.write_block_at(blkaddr, chunk)?;
                    self.mark_data_block(blkaddr, nid, addrs.len() as u16, SegType::WarmData);
                    addrs.push(blkaddr);
                    self.valid_block_count += 1;
                }
                continue;
            }

            // 压缩获益: 构造 24B 头 + 载荷, 切成 phys_blks 个物理块
            let clen = compressed.len() as u32;
            let chksum = crate::f2fs::write::crc::crc32(&padded);
            let mut payload = Vec::with_capacity(header_and_payload_len);
            payload.extend_from_slice(&clen.to_le_bytes());
            payload.extend_from_slice(&chksum.to_le_bytes());
            payload.extend_from_slice(&[0u8; 16]); // reserved
            payload.extend_from_slice(&compressed);

            let mut phys_addrs = Vec::with_capacity(phys_blks);
            for i in 0..phys_blks {
                let blkaddr = self.segalloc.alloc_data_block(SegType::WarmData)?;
                let mut block = vec![0u8; F2FS_BLKSIZE];
                let start = i * F2FS_BLKSIZE;
                let end = (start + F2FS_BLKSIZE).min(payload.len());
                block[..end - start].copy_from_slice(&payload[start..end]);
                self.write_block_at(blkaddr, &block)?;
                self.mark_data_block(blkaddr, nid, addrs.len() as u16, SegType::WarmData);
                phys_addrs.push(blkaddr);
                self.valid_block_count += 1;
                compr_blocks += 1;
            }

            // addr 数组: [COMPRESS_ADDR, phys0..physN, 0 填充] 共 cluster_blks 槽
            addrs.push(COMPRESS_ADDR);
            for &pa in &phys_addrs {
                addrs.push(pa);
            }
            while addrs.len() % cluster_blks != 0 {
                addrs.push(NULL_ADDR);
            }
        }
        Ok((addrs, compr_blocks))
    }

    /// 向当前 (最近推入的) 目录的 dentry 列表追加一条。
    fn add_dentry_to_current(&mut self, parent_nid: u32, name: &str, ino: u32, ftype: FileType) {
        // 找到栈中属于 parent_nid 的目录 (深度优先, 栈顶最可能)
        for dir in self.pending_dirs.iter_mut().rev() {
            if dir.nid == parent_nid {
                dir.dentries
                    .push(DentryInfo::new(name.as_bytes(), ino, ftype));
                return;
            }
        }
    }

    /// flush 所有待写目录: 重写 dentry block (含子项条目) 与更新 inode links。
    fn flush_pending_dirs(&mut self) -> Result<()> {
        let pending = std::mem::take(&mut self.pending_dirs);
        for dir in pending {
            // 构建 dentry block: . .. + 子项
            let mut dentry_builder = DentryBlockBuilder::new();
            dentry_builder.add_entry(DentryInfo::new(b".", dir.nid, FileType::Dir));
            dentry_builder.add_entry(DentryInfo::new(b"..", dir.pino, FileType::Dir));
            for de in &dir.dentries {
                dentry_builder.add_entry(de.clone());
            }
            let dentry_block = dentry_builder.build();
            self.write_block_at(dir.data_blkaddr, &dentry_block)?;

            // 更新 inode: links = 2 + 子目录数, size = 4096 (单 dentry block)
            let subdir_count = dir
                .dentries
                .iter()
                .filter(|d| d.file_type == FileType::Dir)
                .count() as u32;
            let links = 2 + subdir_count;
            let (uid, gid, mode) = self.fs_config_attrs(&dir.fs_path, true);
            let mut inode = InodeBuilder::new_dir((mode & 0o7777) as u16, uid, gid)
                .with_links(links)
                .with_size(F2FS_BLKSIZE as u64)
                .with_blocks(2)
                .with_timestamp_nsecs(dir.mtime_secs, dir.mtime_nsecs)
                .with_pino(dir.pino)
                .with_depth(dir.depth)
                .with_name(if dir.is_root { b"/" } else { &dir.name })
                .with_addrs(vec![dir.data_blkaddr]);
            inode = self.apply_selinux(inode, &dir.fs_path);
            let buf = inode.build(dir.nid, dir.nid, self.cp_ver);
            if let Some(&blkaddr) = self.inode_blocks.get(&dir.nid) {
                self.write_block_at(blkaddr, &buf)?;
            } else if !dir.is_root {
                // 非根目录的 inode 块在 load_directory 时已写入, 这里重写更新
                let _ = buf;
            }
        }
        Ok(())
    }

    /// 标记 node 块: 更新 SIT / SSA。seg_type 决定 SIT vblocks 高位的 curseg 类型。
    fn mark_node_block(&mut self, blkaddr: u32, nid: u32, seg_type: SegType) {
        let _ = self
            .sit
            .mark_block_used(blkaddr, seg_type.curseg_index() as u16);
        let _ = self.ssa.set_node_summary(blkaddr, nid);
    }

    /// 标记 data 块: 更新 SIT / SSA。
    fn mark_data_block(&mut self, blkaddr: u32, nid: u32, ofs_in_node: u16, seg_type: SegType) {
        let _ = self
            .sit
            .mark_block_used(blkaddr, seg_type.curseg_index() as u16);
        let _ = self.ssa.set_data_summary(blkaddr, nid, ofs_in_node);
    }

    /// 为 6 个 curseg 当前段显式设 SIT type。某些 curseg 段可能未实际分配块
    /// (如 COLD_DATA 段未被使用), 但 CP 仍指向它, fsck 要求 SIT type 与
    /// curseg 类型一致, 否则报 "Incorrect curseg type(SIT) [0]"。
    fn finalize_curseg_sit_types(&mut self) {
        let ci = self.segalloc.get_curseg_info();
        let pairs = [
            (
                ci.node_segno[0],
                crate::f2fs::write::consts::CURSEG_HOT_NODE as u16,
            ),
            (
                ci.node_segno[1],
                crate::f2fs::write::consts::CURSEG_WARM_NODE as u16,
            ),
            (
                ci.node_segno[2],
                crate::f2fs::write::consts::CURSEG_COLD_NODE as u16,
            ),
            (
                ci.data_segno[0],
                crate::f2fs::write::consts::CURSEG_HOT_DATA as u16,
            ),
            (
                ci.data_segno[1],
                crate::f2fs::write::consts::CURSEG_WARM_DATA as u16,
            ),
            (
                ci.data_segno[2],
                crate::f2fs::write::consts::CURSEG_COLD_DATA as u16,
            ),
        ];
        for &(segno, seg_type) in &pairs {
            let _ = self.sit.set_seg_type(segno, seg_type);
        }
    }

    /// 查询 fs_config 属性。
    fn fs_config_attrs(&self, fs_path: &str, is_dir: bool) -> (u32, u32, u32) {
        if let Some(ref cfg) = self.fs_config {
            cfg.get_attrs(fs_path, is_dir)
        } else {
            let mode = if is_dir { 0o755 } else { 0o644 };
            (0, 0, mode)
        }
    }

    /// 取一条目目的时间戳: -T 指定时全部用固定值, 否则读源文件 mtime (含纳秒)。
    fn file_timestamp(&self, meta: &std::fs::Metadata) -> (u64, u32) {
        if let Some(fixed) = self.fixed_time {
            return (fixed, 0);
        }
        match meta.modified() {
            Ok(time) => {
                let dur = time
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                (dur.as_secs(), dur.subsec_nanos())
            }
            // 取不到 mtime 时退回构建时间
            Err(_) => (self.mkfs_time, 0),
        }
    }

    /// 应用 SELinux 上下文 (若提供且命中)。
    fn apply_selinux(&mut self, inode: InodeBuilder, fs_path: &str) -> InodeBuilder {
        if let Some(ref mut selinux) = self.selinux {
            if let Some(ctx) = selinux.lookup(fs_path) {
                return inode.with_selinux_context(&ctx);
            }
        }
        inode
    }

    /// 在指定块地址写入 4KiB 数据。
    fn write_block_at(&mut self, blkaddr: u32, data: &[u8]) -> Result<()> {
        let offset = u64::from(blkaddr) * F2FS_BLKSIZE as u64;
        self.writer.seek(SeekFrom::Start(offset))?;
        self.writer.write_all(data)?;
        Ok(())
    }

    fn write_nat_area(&mut self) -> Result<()> {
        let data = self.nat.to_bytes();
        let offset = u64::from(self.layout.nat_blkaddr) * F2FS_BLKSIZE as u64;
        self.writer.seek(SeekFrom::Start(offset))?;
        self.writer.write_all(&data)?;
        // 双副本: 第二份写到 nat_blkaddr + (segment_count_nat/2) * blocks_per_seg
        let half = self.layout.segment_count_nat / 2 * DEFAULT_BLOCKS_PER_SEGMENT;
        self.writer.seek(SeekFrom::Start(
            offset + u64::from(half) * F2FS_BLKSIZE as u64,
        ))?;
        self.writer.write_all(&data)?;
        Ok(())
    }

    fn write_sit_area(&mut self) -> Result<()> {
        let data = self.sit.to_bytes();
        let offset = u64::from(self.layout.sit_blkaddr) * F2FS_BLKSIZE as u64;
        self.writer.seek(SeekFrom::Start(offset))?;
        self.writer.write_all(&data)?;
        let half = self.layout.segment_count_sit / 2 * DEFAULT_BLOCKS_PER_SEGMENT;
        self.writer.seek(SeekFrom::Start(
            offset + u64::from(half) * F2FS_BLKSIZE as u64,
        ))?;
        self.writer.write_all(&data)?;
        Ok(())
    }

    fn write_ssa_area(&mut self) -> Result<()> {
        let data = self.ssa.to_bytes();
        let offset = u64::from(self.layout.ssa_blkaddr) * F2FS_BLKSIZE as u64;
        self.writer.seek(SeekFrom::Start(offset))?;
        self.writer.write_all(&data)?;
        Ok(())
    }

    /// 写入双副本 checkpoint (含 compact summary)。
    ///
    /// F2FS 约定: pack0 ver=奇数(活跃), pack1 ver=偶数(旧); 内核选版本号大者。
    /// 新建镜像写 pack0=1, pack1=0。
    fn write_checkpoint(&mut self) -> Result<()> {
        let curseg = self.segalloc.get_curseg_info();
        // fsck 期望 sit/nat_ver_bitmap_bytesize = ((seg/2)*blks_per_seg)/8
        let bitmap_size = |seg: u32| ((seg / 2) * DEFAULT_BLOCKS_PER_SEGMENT / 8) as usize;
        let mut sit_bitmap = self.sit.version_bitmap();
        sit_bitmap.resize(bitmap_size(self.layout.segment_count_sit), 0);
        let mut nat_bitmap = self.nat_bitmap();
        nat_bitmap.resize(bitmap_size(self.layout.segment_count_nat), 0);

        let blocks_per_seg = DEFAULT_BLOCKS_PER_SEGMENT;
        let cp_payload = self.layout.cp_payload;

        // cp_pack_total = 1 (header) + cp_payload + 1 (compact summary) + 1 (footer)
        // 内核 validate_checkpoint 读 footer @ cp_blkaddr + cp_pack_total - 1,
        // 要求 footer.checkpoint_ver == header.checkpoint_ver (footer = header 副本)。
        // compact summary 写在 header+payload 之后、footer 之前。
        let cp_pack_total = 3 + cp_payload;

        // compact summary 两份共用 (NAT/SIT journal 内容一致)
        let compact_summary = self.build_compact_summary();

        // 写 pack 0 (ver=1, 活跃)
        let cp0_header = self.build_cp_header(1, &curseg, &nat_bitmap, &sit_bitmap, cp_pack_total);
        let cp0_base = self.layout.cp_blkaddr;
        self.write_block_at(cp0_base, &cp0_header)?;
        self.write_block_at(cp0_base + 1 + cp_payload, &compact_summary)?;
        self.write_block_at(cp0_base + cp_pack_total - 1, &cp0_header)?;

        // 写 pack 1 (ver=0, 旧): 内容一致仅版本号不同
        let cp1_header = self.build_cp_header(0, &curseg, &nat_bitmap, &sit_bitmap, cp_pack_total);
        let cp1_base = self.layout.cp_blkaddr + blocks_per_seg;
        self.write_block_at(cp1_base, &cp1_header)?;
        self.write_block_at(cp1_base + 1 + cp_payload, &compact_summary)?;
        self.write_block_at(cp1_base + cp_pack_total - 1, &cp1_header)?;

        // nat_bits: 写在每个 pack 末尾的 nat_bits_blocks 个块。
        // 布局: [8B CP crc] + [nat_bits_bytes full bitmap] + [nat_bits_bytes empty bitmap]。
        // fsck 见 CP_NAT_BITS_FLAG 后从 pack 末尾读 nat_bits, 跳过 f2fs_init_nid_bitmap
        // 对保留 inode addr(1) 的校验 (无 nat_bits 时报 "addr(1) is invalid")。
        let nat_bits = self.build_nat_bits(&cp0_header);
        let log_blks_per_seg: u32 = 9; // DEFAULT_BLOCKS_PER_SEGMENT=512 的 log2
        let nat_bits_blocks = nat_bits.len() / F2FS_BLKSIZE;
        let nb_base = self.layout.cp_blkaddr + (1u32 << log_blks_per_seg) - nat_bits_blocks as u32;
        for (i, chunk) in nat_bits.chunks(F2FS_BLKSIZE).enumerate() {
            self.write_block_at(nb_base + i as u32, chunk)?;
        }
        // pack1 末尾 (cp_blkaddr + 2*blks_per_seg - nat_bits_blocks)
        let pack1_tail = self.layout.cp_blkaddr + blocks_per_seg + (1u32 << log_blks_per_seg)
            - nat_bits_blocks as u32;
        for (i, chunk) in nat_bits.chunks(F2FS_BLKSIZE).enumerate() {
            self.write_block_at(pack1_tail + i as u32, chunk)?;
        }

        Ok(())
    }

    /// 构建 nat_bits 数据 (整个 CP pack 末尾的 nat_bits_blocks 个块)。
    /// 布局: [8B get_cp_crc] + [full_bits] + [empty_bits], 其中 full/empty 各 nat_bits_bytes。
    /// get_cp_crc = cp_ver | (crc << 32), 与内核/官方一致。
    /// full/empty bitmap 默认全零 (无满 NAT block, 无 empty 标记)。
    fn build_nat_bits(&self, cp_header: &[u8]) -> Vec<u8> {
        // nat_bits_bytes = segment_count_nat << 5 (= /8 per NAT block)
        let nat_bits_bytes =
            (self.layout.segment_count_nat as usize) * DEFAULT_BLOCKS_PER_SEGMENT as usize / 8;
        let total = 8 + nat_bits_bytes * 2; // crc + full + empty
        let nat_bits_blocks = total.div_ceil(F2FS_BLKSIZE);
        let mut buf = vec![0u8; nat_bits_blocks * F2FS_BLKSIZE];

        // 首字段: get_cp_crc = cp_ver(低32) | (crc << 32)
        let crc = u32::from_le_bytes([
            cp_header[CP_CHKSUM_OFFSET],
            cp_header[CP_CHKSUM_OFFSET + 1],
            cp_header[CP_CHKSUM_OFFSET + 2],
            cp_header[CP_CHKSUM_OFFSET + 3],
        ]);
        let cp_ver = self.cp_ver; // 1
        let cp_crc = (cp_ver & 0xFFFF_FFFF) | ((u64::from(crc)) << 32);
        buf[..8].copy_from_slice(&cp_crc.to_le_bytes());

        // full/empty bitmap: 全零 (官方对照确认; 无满 NAT block, 无 empty 标记)
        buf
    }

    /// 构建指定版本号的 checkpoint 头部 (4KiB)。
    fn build_cp_header(
        &self,
        version: u64,
        curseg: &CursegInfo,
        nat_bitmap: &[u8],
        sit_bitmap: &[u8],
        cp_pack_total: u32,
    ) -> Vec<u8> {
        let ovp = self
            .segalloc
            .free_segments()
            .saturating_sub(NR_CURSEG_TYPE as u32)
            .max(1);
        // fsck 公式: (seg_main - overprov) * blocks_per_seg, 必须严格小于 seg_main<<log
        let user_blocks = (u64::from(self.layout.segment_count_main)
            .saturating_sub(u64::from(ovp)))
            * u64::from(DEFAULT_BLOCKS_PER_SEGMENT);
        let mut cp = CheckpointBuilder::new()
            .with_version(version)
            .with_user_block_count(user_blocks)
            .with_valid_block_count(self.valid_block_count)
            .with_free_segment_count(self.segalloc.free_segments())
            .with_rsvd_segment_count(NR_CURSEG_TYPE as u32)
            .with_overprov_segment_count(ovp)
            .with_flags(CP_UMOUNT_FLAG | CP_COMPACT_SUM_FLAG_W | CP_NAT_BITS_FLAG)
            .with_valid_node_count(self.valid_node_count)
            .with_valid_inode_count(self.valid_inode_count)
            .with_next_free_nid(self.nat.next_free_nid())
            .with_sit_bitmap(sit_bitmap.to_vec())
            .with_nat_bitmap(nat_bitmap.to_vec())
            .with_cp_pack_total_block_count(cp_pack_total);

        cp.set_cur_node_seg(0, curseg.node_segno[0], curseg.node_blkoff[0]);
        cp.set_cur_node_seg(1, curseg.node_segno[1], curseg.node_blkoff[1]);
        cp.set_cur_node_seg(2, curseg.node_segno[2], curseg.node_blkoff[2]);
        cp.set_cur_data_seg(0, curseg.data_segno[0], curseg.data_blkoff[0]);
        cp.set_cur_data_seg(1, curseg.data_segno[1], curseg.data_blkoff[1]);
        cp.set_cur_data_seg(2, curseg.data_segno[2], curseg.data_blkoff[2]);

        cp.build()
    }

    /// 构建 compact summary 块 (NAT journal + SIT journal + data summaries)。
    ///
    /// NAT journal 条目布局 (与读取侧 load_nat_journal 一致):
    /// nid(4) + version(1) + ino(4) + block_addr(4) = 13 字节。
    fn build_compact_summary(&self) -> Vec<u8> {
        let mut buf = vec![0u8; F2FS_BLKSIZE];

        // 1. NAT journal (前 SUM_JOURNAL_SIZE 字节): count(2) + 条目
        let journal = self.nat.journal_entries();
        let max_nats = (SUM_JOURNAL_SIZE - 2) / NAT_JOURNAL_ENTRY_SIZE;
        let n_nats = journal.len().min(max_nats) as u16;
        buf[0..2].copy_from_slice(&n_nats.to_le_bytes());
        for (i, &(nid, ino, block_addr)) in journal.iter().take(max_nats).enumerate() {
            let off = 2 + i * NAT_JOURNAL_ENTRY_SIZE;
            buf[off..off + 4].copy_from_slice(&nid.to_le_bytes());
            buf[off + 4] = 0; // version
            buf[off + 5..off + 9].copy_from_slice(&ino.to_le_bytes());
            buf[off + 9..off + 13].copy_from_slice(&block_addr.to_le_bytes());
        }

        // 2. SIT journal (偏移 SUM_JOURNAL_SIZE, 长度 SUM_JOURNAL_SIZE): count(2) + 条目
        //    新建镜像不向 SIT journal 写增量, count=0 (SIT 状态由 SIT 区域 + 版本 bitmap 承载)
        buf[SUM_JOURNAL_SIZE..SUM_JOURNAL_SIZE + 2].copy_from_slice(&0u16.to_le_bytes());

        // 3. footer: summary 类型 + CRC32
        let footer = F2FS_BLKSIZE - 5;
        buf[footer] = 0; // SUM_TYPE_DATA (compact summary 块用 DATA 类型)
        let crc = crate::f2fs::write::crc::crc32(&buf[..=footer]);
        buf[footer + 1..footer + 5].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    fn nat_bitmap(&self) -> Vec<u8> {
        let needed = self
            .nat
            .next_free_nid()
            .div_ceil(crate::f2fs::write::consts::NAT_ENTRY_PER_BLOCK_W as u32);
        let size = (needed as usize).div_ceil(8);
        let mut bm = vec![0u8; size];
        for i in 0..needed {
            bm[i as usize / 8] |= 1 << (i % 8);
        }
        bm
    }

    fn write_superblock(&mut self) -> Result<()> {
        let sb = self.superblock_builder.build()?;
        // 超级块写两份: 偏移 1024 (块 0) 和 偏移 1024 + 4096 (块 1)
        self.writer.seek(SeekFrom::Start(1024))?;
        self.writer.write_all(&sb)?;
        self.writer
            .seek(SeekFrom::Start(1024 + F2FS_BLKSIZE as u64))?;
        self.writer.write_all(&sb)?;
        Ok(())
    }
}

/// 顶层入口: 构建 F2FS 镜像。
pub fn build_f2fs_image(cfg: MkfsConfig) -> Result<()> {
    let sparse = cfg.sparse;
    let output_path = cfg.output_path.clone();
    let mut builder = F2fsBuilder::new(cfg)?;
    builder.build()?;
    if sparse {
        crate::f2fs::write::sparse::convert_file(&output_path)?;
    }
    Ok(())
}
