# Folio Flow

A fast, minimal PDF reader and page editor for Windows, written in Rust. It
reads, searches, copies text, prints, exports to Markdown, and edits page order,
rotation, and deletion.

Rendering is [PDFium](https://pdfium.googlesource.com/pdfium/) (the engine in
Chrome); the interface is [Iced](https://iced.rs) on the GPU.

## What it does

- **Reading.** Continuous scroll, zoom from 10 % to 800 %, fit width / fit
  page, outline and page-thumbnail sidebar, light and dark themes, drag and
  drop to open.
- **Searching.** Ctrl+F finds as you type, starting from where you are, and
  highlights every match. Case and accents are ignored, a phrase is found
  across a line break, and so is a word hyphenated over two lines.
- **Links.** Click a link to go where it leads: a place in the document, or
  a web page or mail address, which the browser or mail program opens.
  Nothing else a document asks to have opened is. A web address that is
  only written out in the text is a link too, whole even where it runs over
  several lines.
- **Copying.** Drag to select; double-click for a word, triple-click for a
  line, Shift-click to extend, Ctrl+A for everything; Ctrl+C copies.
- **Printing.** The Windows print dialog, with page ranges. Pages are drawn
  onto the printer by PDFium directly, so text and vector art stay vector.
- **Markdown export.** Headings (from font size and the document outline),
  paragraphs with hyphenation undone, nested lists, bold / italic / inline
  code, fenced code blocks, and tables; running headers, footers and page
  numbers are dropped. Also available without the window:
  `folio --export-markdown in.pdf out.md`.
- **导出与压缩。** 导出窗口支持 Markdown、JPG、PNG；图片以 72 DPI
  为 1×，可选 1.5×、2×、3×，每页单独输出到新文件夹。压缩窗口支持无损
  整理及纯图 PDF；纯图模式提供 72/144/216 DPI 与 JPEG 质量选项，抽样预估
  文件大小。纯图 PDF 仅保留页面外观，移除文字层、链接、书签和表单数据，
  保存为副本。导出使用当前编辑后的页序和旋转。界面已中文化。
- **Page editing.** A virtualized Organizer reorders, rotates, and deletes pages
  with undo/redo; Ctrl+S saves and Ctrl+Shift+S saves a copy. Reordering keeps
  existing thumbnails, rotation refreshes only changed pages, and deletion
  keeps surviving thumbnails.

`folio --help` lists the command-line options.

## 本地增强版日常使用

- 拖入 PNG、JPG、WebP、BMP、GIF、TIFF 到页面导航栏或页面编排视图，
  可按光标位置插入一页；拖入 PDF 则插入其中全部页面。每次拖入一个文件。
  悬停显示插入位置并让位，靠近边缘自动滚动；松手才提交一次编辑。
- 图片按 72 DPI（1 像素 = 1 点）生成页面；读取 EXIF 方向，透明部分铺白。
  GIF/TIFF 使用首幅图像。导入使用无损像素嵌入，不额外做 JPEG 有损重压缩。
- 插入后可旋转、排序、删除和撤销/重做；移动或删除导入文件不会破坏重做。
  原文档的缩略图继续复用。文件名前的圆点表示尚未保存。
- Ctrl+S 保存，Ctrl+Shift+S 另存为。关闭或打开另一文档前，会询问保存、
  放弃或取消；保存失败会保留当前编辑结果。打印也包含尚未保存的页面编辑。
- 拖到阅读画布而非页面导航栏的 PDF，维持“打开另一文档”的行为；未保存时先确认。
- 运行包位于 `dist/Folio Flow/`，exe 与 pdfium.dll 应保持在同一文件夹。

当前仍不支持密码 PDF。重排、删除和插页采用 PDFium 页面导入，复杂文档的
签名、表单、附件、命名目的地及物理书签结构保全仍有限制；纯图 PDF 则明确
仅保留页面外观。此版本的真实系统拖拽手感、DPI 切换和最新 GUI 性能仍需实机确认。

## Installing

Run `Folio Flow-<version>-setup.exe`. It installs for the current user, so it does
not ask for administrator rights, and adds a Start menu entry. Folio is added
to the *Open with* list for PDF files; whichever program opens PDFs today
keeps doing so until you choose Folio under Settings › Apps › Default apps.
Uninstall from Settings › Apps › Installed apps.

The installer is not code-signed, so Windows SmartScreen will warn on first
run: choose *More info*, then *Run anyway*. `Folio Flow-<version>-setup.exe /S`
installs silently.

There is also a portable folder, `Folio Flow`, that runs from anywhere.

## Shortcuts

| | |
|---|---|
| Open | Ctrl+O |
| Save / Save as | Ctrl+S / Ctrl+Shift+S |
| Undo / Redo | Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z |
| Rotate left / right | Ctrl+Shift+R / Ctrl+R |
| Delete selected pages (Organizer) | Delete |
| Print | Ctrl+P |
| Export as Markdown | Ctrl+E |
| Find | Ctrl+F |
| Next / previous match | Enter / Shift+Enter, F3 / Shift+F3, or Ctrl+G / Ctrl+Shift+G |
| Close the find bar | Esc |
| Copy / Select all / Clear selection | Ctrl+C / Ctrl+A / Esc |
| Zoom in / out / actual size | Ctrl++ / Ctrl+− / Ctrl+0, or Ctrl+wheel |
| Reader: fit width / fit page | Ctrl+1 / Ctrl+2 |
| Organizer: Reader / Organizer | Ctrl+1 / Ctrl+2 |
| Toggle sidebar | Ctrl+B |
| Scroll | wheel, arrows, PgUp / PgDn, Space, Home / End |

## Building

On Windows with PowerShell 7, build the canonical portable package:

```powershell
pwsh -File scripts/fetch-pdfium.ps1
pwsh -File scripts/build-windows.ps1
& ".\dist\Folio Flow\Folio Flow.exe"
```

The build script compiles Release, copies the runtime and licences, and verifies
Markdown export with a path containing spaces. Output stays in `dist/Folio Flow/`.
See [使用说明](docs/USAGE.zh-CN.md) and [开发说明](docs/DEVELOPMENT.md).


PDFium is loaded at run time from next to the executable. Fetch the prebuilt
library once (it is not checked in), then build as usual; the build script
copies it into place.

On Windows:

```powershell
.\scripts\fetch-pdfium.ps1
cargo run --release -- path\to\file.pdf
```

On Linux or WSL (for development, and to cross-compile the Windows build):

```sh
./scripts/fetch-pdfium.sh
cargo run --release -- path/to/file.pdf      # native
cargo test                                   # no GPU needed
./scripts/package-windows.sh                 # -> dist/
```

`package-windows.sh` cross-compiles the release and produces both
`dist/Folio Flow-<version>-setup.exe` and the portable `dist/Folio Flow/`.
It needs [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin) and
[NSIS](https://nsis.sourceforge.io) (`apt install nsis`; on Debian/Ubuntu
without root the script unpacks NSIS into `target/nsis` by itself). Set
`PUBLISHER="Your Name"` to name a publisher in the installer. The installer
is defined in `installer/folio.nsi`.

The icon is drawn in `assets/icon/folio.svg`; after changing it, run
`cargo run --example make-icon` to regenerate the `.ico`, the window icon and
the installer artwork.
`build.rs` links the `.ico` into `folio.exe` as its icon; it writes the
resource file itself, so the cross-build needs no resource compiler.

The delivered executable is always **`Folio Flow.exe`**. Keep it beside `pdfium.dll`.
Cargo keeps its internal `folio` target name; use `scripts/build-windows.ps1` to build and verify the named portable package.

## How it stays fast

- **One engine thread.** PDFium is not thread safe, so a single worker owns it
  and the UI never waits on it. The UI publishes what the current view is
  missing; that list is replaced on every scroll, so work for content that has
  scrolled away is never started. What the engine produces is taken in a batch
  per frame, so a burst of tiles redraws the interface once.
- **The first page is rendered before there is a window.** Nearly all of
  start-up is the graphics driver creating its device. The engine does not
  wait for it: it is told the shape the view will have, and has the first
  screen rendered some 30 ms after launch. The first frame that shows the
  document shows it complete. Fonts are discovered on another thread in the
  same interval.
- **Tiles, not pages.** Pages are rendered in 510 px tiles at exactly the
  device resolution, only where they intersect the viewport (plus a screenful
  of prefetch in the direction of travel). Zooming to 800 % costs the same as
  100 %. PDFium renders straight into the buffer that is handed to the GPU.
- **Virtualised everything.** The page stack, outline, thumbnail list and
  Organizer grid only build what is on screen; cost does not grow with document
  length.
- **Bounded memory.** Rendered tiles are kept for the view, its prefetch, and
  half as much again for scrolling back: some twenty megabytes in a window of
  ordinary size. Text layers and thumbnails live in small LRUs.
  The file is read on demand, never loaded whole. PDFium itself keeps
  everything it has parsed for as long as a document is open, so no handle on
  the document is kept for long: searches and exports run through one of
  their own that is closed afterwards, and the main one is traded for a fresh
  one every hundred pages or so.
- **Search reads a page once.** The text of every page searched is remembered
  as search sees it, at about a byte a character. Later searches, which is to
  say every keystroke after the first, only open the pages they will find
  something on.
- **Links come with the text.** A page's links are read once, with its text
  layer, and only for pages on show; the addresses written in the text are
  picked out of that layer in the same pass. Hovering and clicking are answered from
  that on the UI thread, and nothing is drawn for them. On the book below
  that is a few hundredths of a millisecond a page; a page of several hundred
  links costs PDFium a few milliseconds to parse, once.
- **Idle means idle.** The app owns one conditional animation-frame
  subscription; sidebars and the Organizer only report motion. Once every
  animation settles there are no timers or frame loops until the next input.
- **One graphics backend.** On Windows the renderer is pinned to Direct3D 12
  instead of probing Vulkan and OpenGL as well, which saves start-up time and
  about 80 MB. Set `WGPU_BACKEND` to override; `gl` in particular is worth
  trying, see below.

**The figures below are historical upstream measurements, not current Folio Flow benchmarks.**

Measured on Windows 11 (Ryzen 9 9950X3D, RTX 4090, 240 Hz display) with a
501-page book, release build:

| | |
|---|---|
| Launch to first page fully rendered | 0.35 s, of which 0.3 s is the graphics driver starting |
| First screen rendered by the engine | 30 ms after launch |
| Rendering | 0.66 ms per tile, 250 megapixels a second |
| Continuous scrolling | steady at the display's 240 Hz, 4.5 ms p95 frame interval, no frame with a tile missing |
| First search of the whole book | 0.28 s, with matches shown as they are found, from the page being read |
| Later searches | 6 ms for a phrase that is not there, 47 ms for a word found 174 times, 0.27 s for one on nearly every page |
| Markdown export of all 501 pages | 0.42 s |
| Memory with the book open | 120 MB working set, 98 MB of it the window and the graphics driver |
| Memory after scrolling through 40 pages | 140 MB |
| Memory of the engine alone, after searching the whole book | 18 MB |
| Threads | 23 |
| CPU while idle | 0 |
| `folio.exe` | 10 MB |

The graphics driver is what the two headline numbers are made of, and it is
the one part that differs from machine to machine: an empty window is 98 MB
and 0.3 s of the figures above.

### A faster start on OpenGL

`WGPU_BACKEND=gl` runs the same renderer on OpenGL, and draws the same
pixels. On the machine above its driver is ready in a third of the time:

| | Direct3D 12 | OpenGL |
|---|---|---|
| Launch to first page fully rendered | 0.35 s | 0.17 s |
| Memory with the book open | 120 MB working set, 76 MB of it private (Task Manager's figure) | 90 MB, 54 MB private |
| Memory after scrolling through 40 pages | 140 MB | 113 MB |
| Processor time while scrolling at 240 Hz | 16 % of a core | 48 % of a core |
| Threads | 23 | 18 |

It is not the default for two reasons. It has been measured on that one
NVIDIA card, and OpenGL drivers vary more between makers than Direct3D ones
do. And it costs three times the processor while scrolling, most of it on the
driver's own threads, with one hitch of 20 ms on the first scroll after
launch. Folio already does what it can about the first: left to wait for the
display itself, the driver spins a core, so on OpenGL frames are presented at
once and paced against the compositor instead (`src/graphics.rs`).

### Measuring it

Three undocumented switches produce the numbers above:

```sh
FOLIO_TRACE=trace.txt folio book.pdf --screenshot shot.png   # start-up timeline, memory once settled
FOLIO_TRACE=trace.txt folio book.pdf --scroll-test 1500      # frame intervals over 1500 frames of scrolling
folio --bench book.pdf                                       # the engine alone: open, render, text, search
```

`FOLIO_TRACE` names a file that the timeline is appended to, in milliseconds
since the process was created. Adding `FOLIO_TRACE_LOG=1` includes what the
windowing and graphics libraries log, which is how the start-up time was
attributed, at some cost to the timings themselves.

## Layout

```
src/
  main.rs            command line
  graphics.rs        choice of graphics backend, frame pacing on OpenGL
  app.rs             state, messages, update loop
  trace.rs           start-up timeline and memory report (FOLIO_TRACE)
  bench.rs           engine benchmark (--bench)
  web.rs             opening web and mail links, and nothing else
  engine/            no UI dependencies
    mod.rs           Engine handle, events, work queue
    worker.rs        the PDFium thread: rendering, text extraction, jobs
    text.rs          text layer and links, hit testing, selection ranges
    markdown.rs      structure inference and Markdown output
    table.rs         table detection from ruling lines
    print.rs         Windows print dialog and device rendering
  motion.rs         springs, tweens and shared motion state
  ui/
    viewer.rs        page view: layout, scrolling, zoom, tile cache, selection
    sidebar.rs       outline and thumbnails
    organizer.rs     virtualized page grid and drag preview
    order.rs         stable source PageId and edit history
    chrome.rs        floating toolbars and window controls
    theme.rs         colour tokens and widget styles
    icon.rs          Lucide icons, rasterised at the size they are shown
installer/folio.nsi  Windows installer (NSIS)
examples/make-icon.rs  renders the icon artwork to .ico / .png / .bmp
```

## Not there yet

- Password-protected documents.
- There is no going back to where a link was followed from, and links into
  other files are not followed. An address written in the text is only
  joined across a line break when the line is full and what follows could
  only be more of it, so one broken in the middle of a plain word stops
  there.
- Search matches whole text only: no whole-word or regular-expression
  options, a phrase split across two pages is not found, and it stops
  counting at 20,000 matches.
- Annotations and form filling (annotations are displayed, not editable).
- Markdown export only recognises tables that are drawn: a grid, or the
  horizontal rules of a typeset table. Tables laid out by spacing alone stay
  as text, since they cannot be told from columns of prose. Merged cells are
  flattened (Markdown has none), a table continuing onto the next page
  becomes two tables, and images are not extracted.
- Printing outside Windows hands the whole file to `lp`.
- The installer is unsigned.

## Licence

Folio is released under the [MIT licence](LICENSE).

## Credits

PDFium (BSD-3-Clause / Apache-2.0), binaries from
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries);
[pdfium-render](https://github.com/ajrcarey/pdfium-render); Iced (MIT);
[Lucide](https://lucide.dev) icons (ISC).
