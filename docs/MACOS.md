# Folio Flow macOS 兼容指南

本指南记录 0.4.1 的 macOS 适配内容、Windows 合并方法和验收步骤。目标是用平台条件编译隔离差异，不改变现有 Windows 行为。

## 本次改动

| 文件 | 内容 | Windows 影响 |
| --- | --- | --- |
| `Cargo.toml` / `Cargo.lock` | 增加 macOS 专用 `objc2` 依赖 | 无，依赖受 `target_os` 限制 |
| `src/app.rs` | macOS 窗口透明，NSView 图层使用 10pt 圆角 | 保留原 DWM 圆角、边框和窗口设置 |
| `src/main.rs` | 字体发现线程在 macOS 加载苹方 | 无 |
| `src/ui/mod.rs` | 优先使用 PingFang SC，缺失时回退 Hiragino Sans；增加字体加载与测试 | 继续使用 Microsoft YaHei UI |
| `docs/MACOS.md` | 本指南 | 无 |

不要用整文件覆盖的方式合并 `src/app.rs` 或 `src/ui/mod.rs`，应保留冲突两侧的平台分支，再按上表逐段核对。

## Windows 下合并

1. 先提交或暂存 Windows 本机的其他改动，确认工作区干净。
2. 拉取包含本次适配的提交：

   ```powershell
   git pull --ff-only origin master
   ```

3. 确认 `Cargo.lock` 中 `folio` 的依赖列表包含 `objc2`，但 `Cargo.toml` 中只有 macOS 条件依赖块新增它。
4. 在 Windows 执行原有检查：

   ```powershell
   cargo fmt --check
   cargo test --locked -- --test-threads=1
   cargo clippy --locked --all-targets -- -D warnings
   cargo check --locked --target x86_64-pc-windows-msvc
   pwsh -File scripts/build-windows.ps1
   ```

5. 打开 `dist/Folio Flow/Folio Flow.exe`，确认中文字体仍为微软雅黑、窗口圆角和边框颜色未变化、PDF 能正常打开。

如果本机尚未安装 Windows target，先执行 `rustup target add x86_64-pc-windows-msvc`。Windows 合并不需要安装 macOS SDK，也不需要 `libpdfium.dylib`。

## macOS 构建前提

- 使用仓库 `rust-toolchain.toml` 指定的 Rust 1.97.1。
- Intel 包使用 `x86_64-apple-darwin`；Apple Silicon 包使用 `aarch64-apple-darwin`。
- 安装对应 target，例如 Apple Silicon 本机构建：

  ```sh
  rustup target add aarch64-apple-darwin
  ```

- macOS 上的窗口、字体、拖放、Retina 和打印必须在实机验收；Windows 只能完成代码合并和 Windows 回归。

## 获取 PDFium

当前 `scripts/fetch-pdfium.ps1` 和 `scripts/fetch-pdfium.sh` 尚未下载 macOS 资产，需要从 [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) 的 `chromium/7881` Release 手动获取：

- Intel：`pdfium-mac-x64.tgz`
- Apple Silicon：`pdfium-mac-arm64.tgz`

解压后按下表放置，并保留许可证：

```text
vendor/pdfium/x86_64-apple-darwin/libpdfium.dylib
vendor/pdfium/x86_64-apple-darwin/LICENSE.pdfium
vendor/pdfium/aarch64-apple-darwin/libpdfium.dylib
vendor/pdfium/aarch64-apple-darwin/LICENSE.pdfium
```

`build.rs` 会根据 Cargo target 自动选择对应目录。PDFium 版本必须与 `Cargo.toml` 的 `pdfium_latest` 选择一致；版本不匹配可能导致运行期符号解析失败。`vendor/pdfium/` 已被 `.gitignore` 排除，不要提交二进制。

## 编译与测试

在 macOS 本机执行，以 Intel Release 为例：

```sh
cargo fmt --check
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked --target x86_64-apple-darwin
./target/x86_64-apple-darwin/release/folio path/to/file.pdf
```

Apple Silicon 本机可将 target 替换为 `aarch64-apple-darwin`。若只做 Windows 交叉编译检查，可安装 Apple target 后运行 `cargo check --locked --target x86_64-apple-darwin`，但这不能验证 AppKit 运行效果或系统字体。

`pingfang_is_loaded_and_selected` 是 macOS 专用测试。它要求保留目录中的 `PingFangUI.ttc` 存在；运行时允许回退，但该测试不会自动跳过缺少字体文件的环境。

## 打包为 .app

至少采用以下结构：

```text
Folio Flow.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/
    │   ├── Folio Flow
    │   └── libpdfium.dylib
    └── Resources/
        └── Folio Flow.icns
```

`libpdfium.dylib` 必须与主可执行文件位于同一目录，因为 PDFium 优先从 `current_exe()` 所在目录加载。不要只放进 `Contents/Frameworks`。

`Info.plist` 应保持当前包配置：可执行文件名为 `Folio Flow`、bundle id 为 `com.kelsoz.folio-flow`、版本与 `Cargo.toml` 一致、声明 PDF 文档类型，并启用高分辨率显示。

本机测试可使用 ad-hoc 签名：

```sh
codesign --force --sign - "Folio Flow.app/Contents/MacOS/libpdfium.dylib"
codesign --force --sign - "Folio Flow.app"
codesign --verify --deep --strict --verbose=2 "Folio Flow.app"
hdiutil create -volname "Folio Flow 0.4.1" -srcfolder "dist/Folio Flow-0.4.1-macos-x86_64" -ov -format UDZO "dist/Folio Flow-0.4.1-macos-x86_64.dmg"
hdiutil verify "dist/Folio Flow-0.4.1-macos-x86_64.dmg"
```

ad-hoc 签名只用于本机测试，不等于 Developer ID 签名或 Apple 公证。正式分发前还需确定最低 macOS 版本、签名身份、公证和 Gatekeeper 交付方式。

## 窗口与字体实现说明

### 窗口圆角

macOS 使用透明无边框窗口，并通过 AppKit 将 NSView 的 `CALayer` 圆角半径设为 10pt。Windows 继续使用 DWM 的圆角偏好、边框颜色和厚度，两条路径互不替换。

窗口按钮仍是应用自绘的最小化、最大化和关闭按钮，不会自动变成 macOS 原生红黄绿交通灯。合并时不要删除 `src/ui/chrome.rs` 的窗口控制胶囊。

### 中文字体

Iced 的字体数据库默认不扫描 macOS 保留字体目录，因此只指定字体名称可能仍回退到宋体。当前实现会显式读取：

```text
/System/Library/PrivateFrameworks/FontServices.framework/Resources/Reserved/PingFangUI.ttc
```

然后选择字体族 `PingFang SC`，并在启动时预热中英文排版。文件不存在时回退 `Hiragino Sans`。不要把系统字体文件复制进应用；该私有路径可能随系统版本变化，需要在 macOS 更新后回归。

标题字重仍保留平台差异：Windows 使用 Semibold，macOS 使用 Bold。这是有意的，不属于字体回退失败。

## 已知差异与验收边界

- 目前没有 macOS CI、自动 PDFium 下载脚本或 `.app` 打包脚本，macOS 构建依赖本机流程。
- macOS 打印交给系统打印后台，不具备 Windows 的 PDFium 矢量打印路径。
- macOS 未实现 Windows 的 Per-Monitor V2 DPI 初始化；Retina、多显示器和缩放切换需实机确认。
- 测试通过不代表真实窗口交互通过。至少验收：窗口拖动、边缘缩放、全屏/还原、关闭、中文排版、PDF 打开、文件拖放、打印和不同显示器缩放。
- 临时安装包采用 ad-hoc 签名，未公证；其他 Mac 首次打开可能被 Gatekeeper 拦截。
