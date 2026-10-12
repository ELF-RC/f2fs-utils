//! fs_config / file_contexts 解析器 (sload 侧消费)。
//!
//! 解析 AOSP 格式的 `<partition>_fs_config` 与 `<partition>_file_contexts`,
//! 供 mkfs.f2fs 在装载文件时查询 inode 属性与 SELinux 上下文。

use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 规范化配置路径: 确保以 / 开头, 去除尾部 /。
fn normalize(path: &str) -> String {
    let mut s = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

/// fs_config 单条记录。
#[derive(Debug, Clone)]
pub struct FsConfigEntry {
    pub path: String,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub capabilities: Option<u64>,
}

/// fs_config 管理器。
#[derive(Debug)]
pub struct FsConfig {
    entries: HashMap<String, FsConfigEntry>,
    order: HashMap<String, usize>,
    default_uid: u32,
    default_gid: u32,
    default_dir_mode: u32,
    default_file_mode: u32,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: HashMap::new(),
            default_uid: 0,
            default_gid: 0,
            default_dir_mode: 0o755,
            default_file_mode: 0o644,
        }
    }
}

impl FsConfig {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> anyhow::Result<Self> {
        let mut cfg = Self::default();
        for (i, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // 格式: path uid gid mode [capabilities]
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            let path = parts[0].to_string();
            let uid = parts[1].parse::<u32>().unwrap_or(0);
            let gid = parts[2].parse::<u32>().unwrap_or(0);
            let mode = u32::from_str_radix(parts[3], 8).unwrap_or(0);
            let caps = parts.get(4).and_then(|s| s.parse::<u64>().ok());

            let normalized = normalize(&path);
            cfg.entries.insert(
                normalized.clone(),
                FsConfigEntry {
                    path: normalized.clone(),
                    uid,
                    gid,
                    mode,
                    capabilities: caps,
                },
            );
            cfg.order.insert(normalized, i);
        }
        Ok(cfg)
    }

    /// 查询路径属性, 返回 (uid, gid, mode)。
    pub fn get_attrs(&self, path: &str, is_dir: bool) -> (u32, u32, u32) {
        let normalized = normalize(path);
        if let Some(e) = self.entries.get(&normalized) {
            return (e.uid, e.gid, e.mode);
        }
        let default_mode = if is_dir {
            self.default_dir_mode
        } else {
            self.default_file_mode
        };
        (self.default_uid, self.default_gid, default_mode)
    }

    pub fn order_of(&self, path: &str) -> Option<usize> {
        self.order.get(&normalize(path)).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// file_contexts 单条记录。
#[derive(Debug, Clone)]
pub struct SelinuxEntry {
    pub pattern: String,
    pub regex: Regex,
    pub context: String,
}

/// file_contexts 管理器。
#[derive(Debug)]
pub struct SelinuxContexts {
    entries: Vec<SelinuxEntry>,
    cache: HashMap<String, String>,
}

impl SelinuxContexts {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> anyhow::Result<Self> {
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // 格式: <path_pattern> <context>
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let pattern = parts[0];
            let selinux_ctx = parts[1];
            // file_contexts 模式为 PCRE, Rust regex 支持子集
            let re = format!("^{pattern}$");
            if let Ok(regex) = Regex::new(&re) {
                entries.push(SelinuxEntry {
                    pattern: pattern.to_string(),
                    regex,
                    context: selinux_ctx.to_string(),
                });
            }
        }
        Ok(Self {
            entries,
            cache: HashMap::new(),
        })
    }

    /// 查询路径对应的 SELinux 上下文 (从后向前匹配, 优先更具体规则)。
    pub fn lookup(&mut self, path: &str) -> Option<String> {
        if let Some(ctx) = self.cache.get(path) {
            return Some(ctx.clone());
        }
        let normalized = normalize(path);
        for entry in self.entries.iter().rev() {
            if entry.regex.is_match(&normalized) {
                self.cache.insert(path.to_string(), entry.context.clone());
                return Some(entry.context.clone());
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
