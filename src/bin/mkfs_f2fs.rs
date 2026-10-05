//! mkfs.f2fs — 一站式 F2FS 镜像打包工具。
//!
//! 复刻 AOSP `mkfs.f2fs -g android` + `sload.f2fs` 流程:
//! 从源目录树 + `fs_config` + `file_contexts` 直接产出可挂载的 F2FS 镜像。
//!
//! 用法:
//! ```text
//! mkfs.f2fs -f <源目录> -o <输出镜像> -s <镜像大小> -t <挂载点>
//!           [-C <fs_config>] [-s <file_contexts>] [-T <时间戳>] [-L <卷标>] [-R]
//! ```
//!
//! 示例:
//! ```text
//! mkfs.f2fs -f ./product -o product.img -s 1073741824 -t /product \
//!           -C ./config/product_fs_config -S ./config/product_file_contexts
//! ```

use clap::Parser;
use f2fs_utils::mkfs::{MkfsF2fsConfig, mkfs};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "mkfs.f2fs",
    about = "One-stop F2FS image builder for Android (mkfs + sload)",
    version
)]
struct Cli {
    /// 源目录 (要装载的文件树根)
    #[arg(short, long)]
    from: PathBuf,

    /// 输出镜像路径
    #[arg(short, long)]
    output: PathBuf,

    /// 镜像大小 (字节)
    #[arg(short, long)]
    size: u64,

    /// 挂载点 (如 /system, /product)
    #[arg(short, long)]
    r#type: String,

    /// `fs_config` 文件路径
    #[arg(short = 'C', long)]
    fs_config: Option<PathBuf>,

    /// `file_contexts` 文件路径
    #[arg(short = 'S', long = "file-contexts")]
    file_contexts: Option<PathBuf>,

    /// 固定时间戳 (Unix 秒, AOSP 用 1230768000)
    #[arg(short = 'T', long)]
    timestamp: Option<u64>,

    /// 卷标 (默认 = 挂载点)
    #[arg(short = 'L', long)]
    label: Option<String>,

    /// 只读镜像 (启用 RO 特性)
    #[arg(short = 'R', long)]
    readonly: bool,

    /// 输出 Android sparse 镜像 (全零块合并为 `DONT_CARE`)
    #[arg(long)]
    sparse: bool,

    /// 启用文件压缩 (`F2FS_FEATURE_COMPRESSION`)
    #[arg(short = 'z', long)]
    compression: bool,

    /// 压缩算法 (默认 lz4)
    #[arg(long = "compress-algo", default_value = "lz4")]
    compress_algo: String,

    /// log2(压缩簇块数), 默认 2 = 4 块 = 16KB
    #[arg(long = "cluster-log", default_value_t = 2)]
    cluster_log: u8,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mkfs.f2fs: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let cfg = MkfsF2fsConfig {
        source_dir: cli.from,
        output_path: cli.output,
        image_size: cli.size,
        mount_point: cli.r#type,
        label: cli.label,
        fs_config: cli.fs_config,
        file_contexts: cli.file_contexts,
        timestamp: cli.timestamp,
        readonly: cli.readonly,
        sparse: cli.sparse,
        compression: cli.compression,
        compress_algo: cli.compress_algo,
        cluster_log: cli.cluster_log,
    };
    mkfs(cfg)
}
