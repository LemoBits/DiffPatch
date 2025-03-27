# DiffPatch

A directory comparison tool written in Rust that generates executable patch files.

一个用Rust编写的目录比较工具，可以生成可执行的补丁文件。

[中文](#diffpatch-中文)

## Features

- Compare file differences between two directories
- Generate executable patch files (.exe)
- Verify target directory before applying patches
- Support for specifying verification files to ensure patches are applied to the correct directory
- Exclude specific file extensions or directories from comparison
- Efficient file difference extraction and application
- Utilize parallel processing to accelerate comparison and patch application for large directories
- Smart I/O optimization to avoid disk bottlenecks from excessive parallelism
- Incremental patching with diff patches to reduce patch size

## Usage

### Create Patch

```bash
diffpatch create --source <SOURCE_DIR> --target <TARGET_DIR> --output <PATCH_FILE> --check-files <VERIFY_FILE1,VERIFY_FILE2,...> --exclude-extensions <EXT1,EXT2,...> --exclude-dirs <DIR1,DIR2,...> --use-diff-patches
```

#### Options

- `--source <DIR>`: Source directory (original files)
- `--target <DIR>`: Target directory (modified files)
- `--output <FILE>`: Output patch file name (default to target directory)
- `--check-files <FILES>`: Comma-separated list of verification files that must exist in the target directory
- `--exclude-extensions <EXTENSIONS>`: Comma-separated list of file extensions to exclude (e.g., `.tmp,.bak`)
- `--exclude-dirs <DIRS>`: Comma-separated list of directories to exclude (e.g., `node_modules,dist`)
- `--use-diff-patches <true|false>`: Use file difference patches instead of storing full files (reduces patch size)

#### Performance Tuning

You can control I/O parallelism via environment variables, especially when dealing with large directories:

```bash
# Set the number of file I/O parallel threads (default is the lesser of CPU cores and 4)
# For high-performance SSDs, you can increase this value
# For mechanical hard drives, reducing this value might be more effective
export DIFFPATCH_IO_THREADS=2
diffpatch create --source ... --target ...
```

### Apply Patch

Place the generated patch file in the directory that needs to be updated, and double-click to run it. The patch program will first verify that the directory is correct, then quickly apply the file changes using parallel processing.

## Build

```bash
cargo build --release
```

The compiled executable will be located in the `target/release/` directory.

## TODO

- [ ] Cross-platform compatibility for generated patches
- [ ] Support for symbolic links and other special file types
- [ ] Digital signature verification for patches
- [ ] Web interface for patch creation and management
- [ ] Compression level configuration for patch size optimization

---

# DiffPatch (中文)

一个用Rust编写的目录比较工具，可以生成可执行的补丁文件。

## 功能特性

- 对比两个目录的文件差异
- 生成可执行的补丁文件（.exe）
- 补丁文件运行时会先验证目标目录是否正确
- 支持指定必要的验证文件，确保补丁应用在正确的目录中
- 支持排除特定后缀名的文件或特定文件夹
- 高效的文件差异提取和应用
- 利用并行处理加速大型目录的比较和补丁应用
- 智能I/O优化，避免过度并行导致的磁盘瓶颈
- 增量差异补丁以减小补丁文件大小

## 使用方法

### 创建补丁

```bash
diffpatch create --source <未修改的目录> --target <修改后的目录> --output <补丁文件名> --check-files <验证文件1,验证文件2,...> --exclude-extensions <排除文件后缀名1,排除文件后缀名2,...> --exclude-dirs <排除文件夹1,排除文件夹2,...> --use-diff-patches
```

#### 选项说明

- `--source <DIR>`: 源目录（原始文件夹）
- `--target <DIR>`: 目标目录（修改后的文件夹）
- `--output <FILE>`: 输出补丁文件名（默认输出到目标目录）
- `--check-files <FILES>`: 逗号分隔的验证文件列表，这些文件必须存在于目标目录中
- `--exclude-extensions <EXTENSIONS>`: 逗号分隔的要排除的文件扩展名列表（例如，`.tmp,.bak`）
- `--exclude-dirs <DIRS>`: 逗号分隔的要排除的目录列表（例如，`node_modules,dist`）
- `--use-diff-patches <true|false>`: 使用文件差异补丁而不是存储完整文件（减小补丁大小）

#### 性能调优

可以通过环境变量控制I/O并行度，特别是在处理大型目录时：

```bash
# 设置文件I/O并行线程数（默认为CPU核心数和4之间的较小值）
# 对于高性能SSD，可以增加此值；对于机械硬盘，减小此值可能会更有效
export DIFFPATCH_IO_THREADS=2
diffpatch create --source ... --target ...
```

### 应用补丁

将生成的补丁文件放到需要更新的目录中，双击运行即可。补丁程序会先验证目录是否正确，然后利用并行处理快速应用文件更改。

## 构建

```bash
cargo build --release
```

编译后的可执行文件将位于 `target/release/` 目录中。
