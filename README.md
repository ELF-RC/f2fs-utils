# f2fs-utils

F2FS tools for Android, written in Rust.

当前实现:

- **`extract.f2fs`** — 分解 F2FS 镜像,提取文件树并生成 Android 的
  `*_fs_config` / `*_file_contexts`。
- **`mkfs.f2fs`** — 一站式打包 F2FS 镜像: 从源目录树 + `fs_config` +
  `file_contexts` 直接产出可挂载的 F2FS 镜像 (复刻 AOSP `mkfs.f2fs -g android`
  + `sload.f2fs` 流程)。

## extract.f2fs 用法

```bash
# 目录模式: 两份配置自动按 <partition>_fs_config / <partition>_file_contexts 命名
extract.f2fs -i <镜像> -o <分解输出目录> -c <配置输出目录>

# 文件模式: 分别指定两份配置的输出文件 (覆盖 -c 的自动命名)
extract.f2fs -i <镜像> -o <分解输出目录> --fs-config <文件> --file-contexts <文件>

# 混用: 只指定其中一个, 另一个回退到 -c 目录
extract.f2fs -i <镜像> -o <分解输出目录> -c <目录> --fs-config <文件>
```

示例:

```bash
extract.f2fs -i product.img -o ./product -c ./config
```

对包含 `/app`、`/etc` 的 `product.img`,结果:

```
./product/app/...
./product/etc/...
./config/product_fs_config
./config/product_file_contexts
```

## mkfs.f2fs 用法

```bash
mkfs.f2fs -f <源目录> -o <输出镜像> -s <镜像大小(字节)> -t <挂载点> \
          [-C <fs_config>] [-S <file_contexts>] [-T <时间戳>] [-L <卷标>] [-R]
```

示例 (与 extract.f2fs 互为逆操作):

```bash
mkfs.f2fs -f ./product -o product.img -s 1073741824 -t /product \
          -C ./config/product_fs_config -S ./config/product_file_contexts
```

参数说明:

| 参数 | 含义 |
|------|------|
| `-f <dir>` | 源目录 (要装载的文件树根) |
| `-o <path>` | 输出镜像路径 |
| `-s <bytes>` | 镜像大小 (字节) |
| `-t <mp>` | 挂载点 (如 `/system`、`/product`) |
| `-C <file>` | `fs_config` 文件路径 |
| `-S <file>` | `file_contexts` 文件路径 |
| `-T <ts>` | 固定时间戳 (Unix 秒, AOSP 用 `1230768000`) |
| `-L <label>` | 卷标 (默认 = 挂载点) |
| `-R` | 只读镜像 (启用 RO 特性) |

## 构建

```bash
cargo build --release
```

产物在 `target/release/`。

> **产物命名约定**: Cargo 不允许二进制名包含 `.` 字符, 因此源码级产物名为
> `extract_f2fs` / `mkfs_f2fs`。CI 构建会将其重命名为 Android 习惯的
> `extract.f2fs` / `mkfs.f2fs`。本地开发若需要该名称, 手动复制即可:
>
> ```bash
> cp target/release/extract_f2fs target/release/extract.f2fs
> cp target/release/mkfs_f2fs target/release/mkfs.f2fs
> ```

## 测试

```bash
cargo test
```

## Lint

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```
