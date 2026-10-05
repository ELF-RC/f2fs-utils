//! F2FS 镜像分解工作流: 遍历文件树、落盘文件、生成 Android
//! `*_fs_config` 与 `*_file_contexts`。
//!
//! 约定 (与用户示例一致):
//! - 输出目录 (`-o`) 直接承载提取出的文件树 (不再嵌套分区子目录)。
//! - 配置输出有两种方式 (可混用, 逐文件独立):
//!   - 给目录 (`-c`): 自动按 `<partition>_fs_config` / `<partition>_file_contexts` 命名。
//!   - 给具体文件 (`--fs-config` / `--file-contexts`): 直接写到该路径, 覆盖目录命名。
//! - 配置内容仅含镜像中实际解析出的条目, 不合成 catch-all / 根行。
//!
//! 路径前缀 = 分区名 (取自镜像文件名 stem):
//! - fs_config:   `<partition>/<relpath> <uid> <gid> <octal_mode> [target]`
//! - file_contexts: `/<partition>/<relpath> <selinux_context>`

use crate::f2fs::F2fsVolume;
use crate::f2fs::consts::{F2FS_FT_DIR, F2FS_FT_REG_FILE, F2FS_FT_SYMLINK, F2FS_ROOT_INO};
use crate::f2fs::types::{Inode, Nid};
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 提取配置。
pub struct ExtractConfig {
    /// 输入镜像路径。
    pub input_image: String,
    /// 提取输出根目录 (直接承载文件树)。
    pub output_dir: PathBuf,
    /// 配置输出目录 (当未显式指定文件路径时, 按 `<partition>_fs_config` /
    /// `<partition>_file_contexts` 自动命名)。
    pub config_dir: Option<PathBuf>,
    /// 显式指定 fs_config 文件路径 (覆盖 `config_dir` 的自动命名)。
    pub fs_config_path: Option<PathBuf>,
    /// 显式指定 file_contexts 文件路径 (覆盖 `config_dir` 的自动命名)。
    pub file_contexts_path: Option<PathBuf>,
}

/// 一条 fs_config 记录。
struct FsEntry {
    rel_path: String, // 不含分区前缀, 无前导 /
    uid: u32,
    gid: u32,
    mode: u16,
    target: Option<String>, // 符号链接目标
}

/// 提取入口。
pub fn extract(cfg: &ExtractConfig) -> Result<()> {
    let volume = F2fsVolume::open(&cfg.input_image)
        .with_context(|| format!("failed to open F2FS image: {}", cfg.input_image))?;

    let partition = Path::new(&cfg.input_image)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("cannot determine partition name from input path"))?
        .to_string();

    fs::create_dir_all(&cfg.output_dir)?;
    let config_dir = cfg.config_dir.as_deref();
    if let Some(dir) = config_dir {
        fs::create_dir_all(dir)?;
    }

    let mut fs_entries: Vec<FsEntry> = Vec::new();
    let mut ctx_entries: BTreeMap<String, String> = BTreeMap::new();

    // 根 inode 自身: 不产出配置行, 但其 xattr 供子项继承语义参考 (此处不继承)。
    let root_nid = Nid(F2FS_ROOT_INO);

    walk(
        &volume,
        root_nid,
        Path::new(""),
        &cfg.output_dir,
        &partition,
        &mut fs_entries,
        &mut ctx_entries,
    )?;

    // 逐文件解析目标路径: 显式指定优先, 否则回退到 config_dir + 自动命名
    let fs_config_path = cfg
        .fs_config_path
        .clone()
        .or_else(|| config_dir.map(|d| d.join(format!("{partition}_fs_config"))));
    let file_contexts_path = cfg
        .file_contexts_path
        .clone()
        .or_else(|| config_dir.map(|d| d.join(format!("{partition}_file_contexts"))));

    if let Some(p) = &fs_config_path {
        write_fs_config(p, &partition, &fs_entries)?;
    }
    if let Some(p) = &file_contexts_path {
        write_file_contexts(p, &partition, &ctx_entries)?;
    }

    Ok(())
}

/// 递归遍历目录, 落盘并收集配置。
#[allow(clippy::too_many_arguments)]
fn walk(
    vol: &F2fsVolume<std::fs::File>,
    nid: Nid,
    rel: &Path,
    out_root: &Path,
    partition: &str,
    fs_entries: &mut Vec<FsEntry>,
    ctx_entries: &mut BTreeMap<String, String>,
) -> Result<()> {
    let inode = vol.read_inode(nid)?;

    // 当前层目录自身 (非根) 的配置: rel 非空表示非根
    if !rel.as_os_str().is_empty() {
        record_entry(&inode, rel, partition, fs_entries);
        record_context(vol, &inode, nid, rel, partition, ctx_entries);
    }

    // 仅目录需要展开子项
    if !inode.is_dir() {
        return Ok(());
    }

    let entries = vol
        .read_dir(&inode, nid)
        .with_context(|| format!("failed to read directory: {}", rel.display()))?;

    for e in &entries {
        let child_rel = sanitize_join(rel, &e.name)?;
        let child_out = out_root.join(&child_rel);

        match e.file_type {
            F2FS_FT_DIR => {
                fs::create_dir_all(&child_out)?;
                walk(
                    vol,
                    e.nid,
                    &child_rel,
                    out_root,
                    partition,
                    fs_entries,
                    ctx_entries,
                )?;
            }
            F2FS_FT_REG_FILE => {
                let child_inode = vol.read_inode(e.nid)?;
                let data = vol
                    .read_file_data(&child_inode, e.nid)
                    .with_context(|| format!("failed to read file: {}", child_rel.display()))?;
                if let Some(parent) = child_out.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&child_out, &data)?;
                record_entry(&child_inode, &child_rel, partition, fs_entries);
                record_context(vol, &child_inode, e.nid, &child_rel, partition, ctx_entries);
            }
            F2FS_FT_SYMLINK => {
                let child_inode = vol.read_inode(e.nid)?;
                let target = vol.read_symlink_target(&child_inode, e.nid)?;
                if let Some(parent) = child_out.parent() {
                    fs::create_dir_all(parent)?;
                }
                create_symlink(&target, &child_out)?;
                let mut entry = make_fs_entry(&child_inode, &child_rel);
                entry.target = Some(target);
                fs_entries.push(entry);
                record_context(vol, &child_inode, e.nid, &child_rel, partition, ctx_entries);
            }
            _ => {
                // 其它类型 (字符/块设备/FIFO/套接字): 跳过, 不落盘
            }
        }
    }
    Ok(())
}

fn record_entry(inode: &Inode, rel: &Path, _partition: &str, fs_entries: &mut Vec<FsEntry>) {
    fs_entries.push(make_fs_entry(inode, rel));
}

fn make_fs_entry(inode: &Inode, rel: &Path) -> FsEntry {
    FsEntry {
        rel_path: rel.to_string_lossy().into_owned(),
        uid: inode.uid,
        gid: inode.gid,
        mode: inode.mode & 0o7777,
        target: None,
    }
}

fn record_context(
    vol: &F2fsVolume<std::fs::File>,
    inode: &Inode,
    nid: Nid,
    rel: &Path,
    _partition: &str,
    ctx_entries: &mut BTreeMap<String, String>,
) {
    if let Ok(xattrs) = vol.read_xattrs(inode, nid) {
        for (name, value) in xattrs {
            if name == "security.selinux" {
                let ctx = String::from_utf8_lossy(&value)
                    .trim_start_matches('\0')
                    .trim_end_matches('\0')
                    .to_string();
                if !ctx.is_empty() {
                    let mut ctx = ctx;
                    if !ctx.ends_with(":s0") {
                        ctx.push_str(":s0");
                    }
                    ctx_entries.insert(rel.to_string_lossy().into_owned(), ctx);
                }
            }
        }
    }
}

/// 写 fs_config 到指定文件路径。
fn write_fs_config(path: &Path, partition: &str, entries: &[FsEntry]) -> Result<()> {
    let mut f =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;

    // 按路径排序, 保证输出稳定
    let mut sorted: Vec<&FsEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    for e in sorted {
        let line = match &e.target {
            Some(t) => format!(
                "{}/{} {} {} {:04o} {}",
                partition, e.rel_path, e.uid, e.gid, e.mode, t
            ),
            None => format!(
                "{}/{} {} {} {:04o}",
                partition, e.rel_path, e.uid, e.gid, e.mode
            ),
        };
        writeln!(f, "{line}")?;
    }
    Ok(())
}

/// 写 file_contexts 到指定文件路径。
fn write_file_contexts(
    path: &Path,
    partition: &str,
    entries: &BTreeMap<String, String>,
) -> Result<()> {
    let mut f =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;

    for (rel, ctx) in entries {
        writeln!(f, "/{partition}/{rel} {ctx}")?;
    }
    Ok(())
}

/// 将 name 接到 rel 之后, 做基本的路径净化 (拒绝 .. 与绝对路径)。
fn sanitize_join(rel: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(anyhow!("invalid entry name: {name:?}"));
    }
    Ok(rel.join(name))
}

/// 跨平台创建符号链接 (仅 unix)。
#[cfg(unix)]
fn create_symlink(target: &str, link_path: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    if link_path.exists() {
        fs::remove_file(link_path)?;
    }
    symlink(target, link_path)
        .with_context(|| format!("failed to create symlink {}", link_path.display()))
}

#[cfg(not(unix))]
fn create_symlink(_target: &str, _link_path: &Path) -> Result<()> {
    Err(anyhow!("symlinks are not supported on this platform"))
}
