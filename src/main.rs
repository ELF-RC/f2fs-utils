//! f2fs-utils — 合并的 F2FS 工具二进制。
//!
//! 通过主参数 (子命令) 区分功能:
//! - `mkfs`:   一站式 F2FS 镜像打包 (mkfs + sload)
//! - `extract`: F2FS 镜像分解
//!
//! 用法:
//! ```text
//! f2fs-utils mkfs --from <源目录> --output <镜像> --size <大小> --type <挂载点>
//! f2fs-utils extract --input <镜像> --output <输出目录>
//! ```

use clap::{Parser, Subcommand};
use f2fs_utils::extract::{ExtractConfig, extract};
use f2fs_utils::mkfs::{MkfsF2fsConfig, mkfs};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "f2fs-utils",
    about = "F2FS tool for Android (mkfs + extract)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// One-stop F2FS image builder for Android (mkfs + sload)
    Mkfs(MkfsArgs),
    /// Extract an F2FS image and emit Android fs_config / file_contexts
    Extract(ExtractArgs),
}

// ── mkfs 子命令 ──────────────────────────────────────────

#[derive(Parser)]
struct MkfsArgs {
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

// ── extract 子命令 ───────────────────────────────────────

#[derive(Parser)]
struct ExtractArgs {
    /// 输入镜像路径
    #[arg(short, long)]
    input: PathBuf,

    /// 分解输出目录 (文件树落盘根)
    #[arg(short, long)]
    output: PathBuf,

    /// 配置输出目录 (未显式指定文件时, 按 `<partition>_fs_config` /
    /// `<partition>_file_contexts` 自动命名)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// 显式指定 `fs_config` 文件路径 (覆盖 `-c` 的自动命名)
    #[arg(long)]
    fs_config: Option<PathBuf>,

    /// 显式指定 `file_contexts` 文件路径 (覆盖 `-c` 的自动命名)
    #[arg(long = "file-contexts")]
    file_contexts: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Mkfs(args) => run_mkfs(args),
        Command::Extract(args) => run_extract(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("f2fs-utils: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_mkfs(args: MkfsArgs) -> anyhow::Result<()> {
    let cfg = MkfsF2fsConfig {
        source_dir: args.from,
        output_path: args.output,
        image_size: args.size,
        mount_point: args.r#type,
        label: args.label,
        fs_config: args.fs_config,
        file_contexts: args.file_contexts,
        timestamp: args.timestamp,
        readonly: args.readonly,
        sparse: args.sparse,
        compression: args.compression,
        compress_algo: args.compress_algo,
        cluster_log: args.cluster_log,
    };
    mkfs(cfg)
}

fn run_extract(args: ExtractArgs) -> anyhow::Result<()> {
    let cfg = ExtractConfig {
        input_image: args
            .input
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("input path is not valid UTF-8"))?
            .to_string(),
        output_dir: args.output,
        config_dir: args.config,
        fs_config_path: args.fs_config,
        file_contexts_path: args.file_contexts,
    };
    extract(&cfg)
}
