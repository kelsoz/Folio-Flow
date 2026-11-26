use std::collections::{HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use pdfium_render::prelude::*;

use crate::trace;

use super::table::Rule;
use super::text::{self, Glyph, LINK_LIMIT, Rect, URI_LIMIT};
use super::{
    Bitmap, CopyRequest, DocInfo, EditRequest, Event, ImportRequest, Job, JobKind, Link,
    MATCH_LIMIT, Match, Open, OutlineItem, PageSize, PageText, SearchRequest, Shared,
    THUMBNAIL_WIDTH, TILE, Target, TileKey, Wanted, markdown, page_pixels, print, raster,
};

type Emit = Box<dyn Fn(Event) + Send>;

/// Parsed pages kept alive between tasks. Tiles of the same page arrive in
/// bursts, and re-parsing a content stream per tile would dominate. No more
/// than the pages around the view, though: a parsed page holds every image
/// on it decoded, which runs to megabytes each.
const PAGE_CACHE: usize = 4;

const OUTLINE_LIMIT: usize = 20_000;

/// Pages parsed through one handle on the document before it is replaced.
const RECYCLE_AFTER: u32 = 96;

/// Pages a search may rule out, from what it remembers of their text, before
/// it looks at the queue again.
const SEARCH_STRIDE: u32 = 256;

/// PDFium keeps state that every document in the process shares, and is not
/// thread safe. The bindings promise to serialise calls to it, but their lock
/// leaves out a good third of its functions, rendering among them; nor could
/// it cover the calls `print` makes to the library directly. So an engine
/// holds this for as long as it is in the library: for each task, and to
/// open and close a document.
///
/// The application has one engine, which never waits here. It is for where
/// there are several, as in the tests: without it they parse over each other
/// and documents come back missing pages, text or outline.
static EXCLUSIVE: Mutex<()> = Mutex::new(());
static NEXT_DOCUMENT: AtomicU64 = AtomicU64::new(1);

fn next_document_id() -> u64 {
    NEXT_DOCUMENT.fetch_add(1, Ordering::Relaxed)
}

fn exclusive() -> MutexGuard<'static, ()> {
    // There is nothing behind the lock for a panic to have left half done.
    EXCLUSIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn run(shared: Arc<Shared>, emit: Emit) {
    let pdfium = {
        let _exclusive = exclusive();

        pdfium()
    };
    trace::mark("engine: pdfium bound");

    loop {
        let Some(Open { path, plan }) = next_open(&shared) else {
            return;
        };

        let pdfium = match pdfium {
            Ok(pdfium) => pdfium,
            Err(error) => {
                emit(Event::OpenFailed {
                    path,
                    error: error.clone(),
                });
                continue;
            }
        };

        let opened = {
            let _exclusive = exclusive();

            pdfium.load_pdf_from_file(&path, None).map(|document| {
                (
                    Arc::new(describe(&document, next_document_id(), &path)),
                    document,
                )
            })
        };

        let (info, document) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                emit(Event::OpenFailed {
                    path,
                    error: describe_open_error(&error),
                });
                continue;
            }
        };

        trace::mark("engine: document described");

        // Start on what the UI is expected to ask for, unless it has moved
        // on to another document already.
        if let Some(plan) = plan {
            let wanted = plan(&info);

            if let Ok(mut queue) = shared.queue.lock()
                && queue.open.is_none()
            {
                queue.doc = info.id;
                queue.wanted = wanted;
            }
        }

        emit(Event::Opened(info.clone()));

        let mut session = Session {
            pdfium,
            pages: VecDeque::new(),
            document,
            source_baseline: None,
            modified: false,
            parsed: 0,
            scratch: None,
            haystacks: (0..info.pages.len()).map(|_| None).collect(),
            source_outline: info.outline.clone(),
            source_title: info.title.clone(),
            info,
            shared: &shared,
            emit: &emit,
            sent: VecDeque::new(),
            tiles_sent: 0,
            copy: None,
            search: None,
            job: None,
        };

        let exit = session.serve();

        // Closing the document and its pages is a call into the library too.
        {
            let _exclusive = exclusive();

            drop(session);
        }

        if exit == Exit::Shutdown {
            return;
        }
    }
}

/// PDFium can be bound once per process, so every engine shares this
/// instance. The bindings serialise calls internally.
fn pdfium() -> &'static Result<Pdfium, String> {
    static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

    PDFIUM.get_or_init(load_pdfium)
}

fn load_pdfium() -> Result<Pdfium, String> {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));

    let bindings = beside_exe
        .as_deref()
        .map(Pdfium::pdfium_platform_library_name_at_path)
        .and_then(|path| Pdfium::bind_to_library(path).ok())
        .map(Ok)
        .unwrap_or_else(Pdfium::bind_to_system_library)
        .map_err(|error| {
            format!(
                "Could not load the PDFium library ({error}). \
                 It should sit next to the application executable."
            )
        })?;

    Ok(Pdfium::new(bindings))
}

/// Blocks until a document should be opened. `None` means shut down.
fn next_open(shared: &Shared) -> Option<Open> {
    let mut queue = shared.queue.lock().ok()?;

    loop {
        if queue.shutdown {
            return None;
        }

        if let Some(open) = queue.open.take() {
            return Some(open);
        }

        // Nothing is open, so nothing else in the queue can be served.
        queue.wanted = Wanted::default();
        queue.copy = None;
        queue.search = None;
        queue.edit = None;
        queue.import = None;
        queue.jobs.clear();

        queue = shared.wake.wait(queue).ok()?;
    }
}

fn describe_open_error(error: &PdfiumError) -> String {
    match error {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            "此文档有密码保护。".into()
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            "此文件不是有效的 PDF，或已损坏。".into()
        }
        PdfiumError::IoError(error) => error.to_string(),
        other => format!("{other:?}"),
    }
}

fn describe(document: &PdfDocument, id: u64, path: &Path) -> DocInfo {
    let pages = document
        .pages()
        .page_sizes()
        .unwrap_or_default()
        .iter()
        .map(|rect| PageSize {
            width: rect.width().value.max(1.0),
            height: rect.height().value.max(1.0),
        })
        .collect();

    let title = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|tag| tag.value().trim().to_owned())
        .filter(|title| !title.is_empty())
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();

    let mut outline = Vec::new();

    if let Some(root) = document.bookmarks().root() {
        collect_outline(root, 0, &mut outline);
    }

    DocInfo {
        id,
        path: path.to_path_buf(),
        title,
        pages,
        outline,
    }
}

fn collect_outline(first: PdfBookmark, depth: u16, out: &mut Vec<OutlineItem>) {
    let mut next = Some(first);

    while let Some(bookmark) = next {
        if out.len() >= OUTLINE_LIMIT || depth > 32 {
            return;
        }

        out.push(OutlineItem {
            title: bookmark
                .title()
                .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default(),
            depth,
            page: bookmark
                .destination()
                .and_then(|destination| destination.page_index().ok())
                .map(|index| index as u32),
        });

        if let Some(child) = bookmark.first_child() {
            collect_outline(child, depth + 1, out);
        }

        next = bookmark.next_sibling();
    }
}

#[derive(PartialEq)]
enum Exit {
    /// Another document is waiting in the queue.
    Reopen,
    Shutdown,
}

enum Task {
    /// Nothing is left to do: let go of what the work needed.
    Tidy,
    /// A tile, and how many the UI had taken in when it asked.
    Tile(TileKey, u64),
    Text(u32),
    Thumbnail(u32),
    Copy(CopyRequest),
    CopyStep,
    Search(SearchRequest),
    SearchStep,
    Edit(EditRequest),
    Import(ImportRequest),
    Start(Job),
    Step,
}

struct Copy {
    request: CopyRequest,
    page: u32,
    out: String,
}

struct Search {
    id: u64,
    needle: text::Needle,
    from: u32,
    /// Pages searched so far.
    done: u32,
    found: usize,
}

enum Active<'p> {
    Raster {
        dest: PathBuf,
        settings: raster::Settings,
        page: u32,
        output: raster::Output<'p>,
    },
    Estimate {
        request: u64,
        settings: raster::Settings,
        samples: Vec<u32>,
        done: usize,
        bytes: u64,
        area: f64,
    },
    Export {
        dest: PathBuf,
        page: u32,
        pages: Vec<markdown::Page>,
    },
    Print(print::Session),
    Save {
        dest: PathBuf,
    },
    Extract {
        dest: PathBuf,
        total: u32,
        output: PdfDocument<'p>,
    },
    Compact {
        dest: PathBuf,
    },
}

impl Active<'_> {
    fn kind(&self) -> JobKind {
        match self {
            Active::Raster { .. } | Active::Estimate { .. } => JobKind::Raster,
            Active::Export { .. } => JobKind::Export,
            Active::Print(_) => JobKind::Print,
            Active::Save { .. } => JobKind::Save,
            Active::Extract { .. } => JobKind::Extract,
            Active::Compact { .. } => JobKind::Compact,
        }
    }
}

struct Session<'s, 'p> {
    pdfium: &'p Pdfium,
    /// Parsed pages of `document`, most recently used first. Declared, and
    /// so dropped, ahead of it.
    pages: VecDeque<(u32, PdfPage<'p>)>,
    /// PDFium keeps what it has parsed of a document for as long as the
    /// document is open, which once every page has been read comes to more
    /// than the file itself. So no handle is kept for long: this one is
    /// traded for a fresh one every [`RECYCLE_AFTER`] pages.
    document: PdfDocument<'p>,
    source_baseline: Option<PdfDocument<'p>>,
    modified: bool,
    /// Pages parsed through `document` since it was opened.
    parsed: u32,
    /// A second handle on the file, for work that walks the whole document.
    /// It is closed as soon as the work is done.
    scratch: Option<PdfDocument<'p>>,
    /// The text of each page searched so far, as search sees it.
    haystacks: Vec<Option<text::Haystack>>,
    source_outline: Vec<OutlineItem>,
    source_title: String,
    info: Arc<DocInfo>,
    shared: &'s Shared,
    emit: &'s Emit,
    /// Tiles sent that the UI is not known to have taken in yet, each with
    /// its place in the count of all sent. See [`Wanted::received`].
    sent: VecDeque<(TileKey, u64)>,
    tiles_sent: u64,
    copy: Option<Copy>,
    search: Option<Search>,
    job: Option<Active<'p>>,
}

impl<'p> Session<'_, 'p> {
    fn serve(&mut self) -> Exit {
        loop {
            let task = {
                let Ok(mut queue) = self.shared.queue.lock() else {
                    return Exit::Shutdown;
                };

                loop {
                    if queue.shutdown {
                        return Exit::Shutdown;
                    }

                    if queue.open.is_some() {
                        return Exit::Reopen;
                    }

                    // Left over from a document that has since been replaced.
                    if queue.doc != self.info.id {
                        queue.wanted = Wanted::default();
                    }

                    if !queue.wanted.tiles.is_empty() {
                        break Task::Tile(queue.wanted.tiles.remove(0), queue.wanted.received);
                    }

                    if !queue.wanted.texts.is_empty() {
                        break Task::Text(queue.wanted.texts.remove(0));
                    }

                    // A copy is interactive, so it runs ahead of long jobs.
                    if let Some(request) = queue.copy.take() {
                        break Task::Copy(request);
                    }

                    if self.copy.is_some() {
                        break Task::CopyStep;
                    }

                    // So is a search: its results show as the query is typed.
                    if let Some(request) = queue.search.take() {
                        break Task::Search(request);
                    }

                    if self.search.is_some() {
                        break Task::SearchStep;
                    }

                    if self.job.is_some() {
                        break Task::Step;
                    }

                    if let Some(import) = queue.import.take() {
                        break Task::Import(import);
                    }
                    if let Some(edit) = queue.edit.take() {
                        break Task::Edit(edit);
                    }

                    if let Some(job) = queue.jobs.pop_front() {
                        break Task::Start(job);
                    }

                    if !queue.wanted.thumbnails.is_empty() {
                        break Task::Thumbnail(queue.wanted.thumbnails.remove(0));
                    }

                    if self.scratch.is_some() || self.parsed >= RECYCLE_AFTER {
                        break Task::Tidy;
                    }

                    let Ok(guard) = self.shared.wake.wait(queue) else {
                        return Exit::Shutdown;
                    };
                    queue = guard;
                }
            };

            let _exclusive = exclusive();

            match task {
                Task::Tidy => self.tidy(),
                Task::Tile(key, received) => self.tile(key, received),
                Task::Text(page) => self.text(page),
                Task::Thumbnail(page) => self.thumbnail(page),
                Task::Copy(request) => {
                    self.copy = Some(Copy {
                        page: request.from.page,
                        request,
                        out: String::new(),
                    });
                }
                Task::CopyStep => self.copy_step(),
                Task::Search(request) => {
                    let last = (self.info.pages.len() as u32).saturating_sub(1);

                    self.search = text::Needle::new(&request.query).map(|needle| Search {
                        id: request.id,
                        needle,
                        from: request.from.min(last),
                        done: 0,
                        found: 0,
                    });
                }
                Task::SearchStep => self.search_step(),
                Task::Edit(edit) => self.apply_edit(edit),
                Task::Import(import) => self.import_pages(import),
                Task::Start(job) => self.start(job),
                Task::Step => self.step(),
            }
        }
    }

    fn page(&mut self, index: u32) -> Option<&PdfPage<'p>> {
        if let Some(position) = self.pages.iter().position(|(page, _)| *page == index) {
            if position != 0 {
                let entry = self.pages.remove(position)?;
                self.pages.push_front(entry);
            }
        } else {
            let page = self.document.pages().get(index as PdfPageIndex).ok()?;

            self.parsed += 1;
            self.pages.push_front((index, page));
            self.pages.truncate(PAGE_CACHE);
        }

        self.pages.front().map(|(_, page)| page)
    }

    /// Lets go of what PDFium has built up, now that nothing is waiting.
    fn tidy(&mut self) {
        self.scratch = None;

        if self.parsed < RECYCLE_AFTER {
            return;
        }

        self.parsed = 0;

        // Should the file have gone, or been replaced by another, since it
        // was opened, the handle that has it open carries on.
        let fresh = if self.modified {
            self.document
                .save_to_bytes()
                .and_then(|bytes| self.pdfium.load_pdf_from_byte_vec(bytes, None))
        } else {
            self.pdfium.load_pdf_from_file(&self.info.path, None)
        };
        if let Ok(fresh) = fresh
            && fresh.pages().len() as usize == self.info.pages.len()
        {
            self.pages.clear();
            self.document = fresh;
        }
    }

    /// Loads a page for a pass over the whole document: from the cache if
    /// it happens to be there, and otherwise through the scratch handle,
    /// without keeping it. Caching it would push out the pages on screen.
    fn with_passing_page<T>(&mut self, index: u32, f: impl FnOnce(&PdfPage) -> T) -> Option<T> {
        if let Some((_, page)) = self.pages.iter().find(|(page, _)| *page == index) {
            return Some(f(page));
        }

        if !self.modified && self.scratch.is_none() {
            self.scratch = self
                .pdfium
                .load_pdf_from_file(&self.info.path, None)
                .ok()
                .filter(|scratch| scratch.pages().len() as usize == self.info.pages.len());
        }

        // The file may have gone since it was opened; the handle that has
        // it open still works.
        let document = self.scratch.as_ref().unwrap_or(&self.document);
        let page = document.pages().get(index as PdfPageIndex).ok()?;

        Some(f(&page))
    }

    fn size(&self, page: u32) -> Option<PageSize> {
        self.info.pages.get(page as usize).copied()
    }

    fn tile(&mut self, key: TileKey, received: u64) {
        // What the UI had by the time it asked is no longer on its way; if
        // it asks for any of that, it has let go of it since.
        while self
            .sent
            .front()
            .is_some_and(|(_, place)| *place <= received)
        {
            self.sent.pop_front();
        }

        if self.sent.iter().any(|(sent, _)| *sent == key) {
            return;
        }

        let Some(size) = self.size(key.page) else {
            return;
        };

        let (width, height) = page_pixels(size, key.scale());
        let x = key.col as u32 * TILE;
        let y = key.row as u32 * TILE;

        if x >= width || y >= height {
            return;
        }

        let region = Region {
            page_width: width,
            page_height: height,
            x,
            y,
            width: TILE.min(width - x),
            height: TILE.min(height - y),
        };

        let Some(page) = self.page(key.page) else {
            return;
        };

        if let Some(bitmap) = render(page, region) {
            trace::mark("engine: first tile rendered");
            self.tiles_sent += 1;
            self.sent.push_back((key, self.tiles_sent));

            (self.emit)(Event::Tile {
                doc: self.info.id,
                key,
                bitmap,
            });
        }
    }

    fn thumbnail(&mut self, index: u32) {
        let Some(size) = self.size(index) else {
            return;
        };

        let scale = THUMBNAIL_WIDTH as f32 / size.width;
        let (width, height) = page_pixels(size, scale);
        // Guard against absurdly tall pages.
        let height = height.min(THUMBNAIL_WIDTH * 8);

        let region = Region {
            page_width: width,
            page_height: height,
            x: 0,
            y: 0,
            width,
            height,
        };

        // A thumbnail is all that is wanted of most pages it is asked for,
        // so the page is not kept for more: see `PAGE_CACHE`.
        let bitmap = match self.pages.iter().find(|(page, _)| *page == index) {
            Some((_, page)) => render(page, region),
            None => {
                self.parsed += 1;
                self.document
                    .pages()
                    .get(index as PdfPageIndex)
                    .ok()
                    .and_then(|page| render(&page, region))
            }
        };

        if let Some(bitmap) = bitmap {
            (self.emit)(Event::Thumbnail {
                doc: self.info.id,
                page: index,
                bitmap,
            });
        }
    }

    fn text(&mut self, index: u32) {
        let info = self.info.clone();
        // The links come with the text of a page on show, and with no other:
        // nothing that reads the whole document has a use for them.
        let text = self.page(index).and_then(|page| {
            let mut text = extract(page, false)?;

            let width = info
                .pages
                .get(index as usize)
                .map_or(0.0, |size| size.width);

            text.set_links(links(page, &info.pages), width);
            Some(text)
        });

        // A page that could not be read is not remembered as blank: that
        // would rule it out of every search to come.
        if let Some(text) = &text {
            self.remember(index, text);
        }

        let text = text.unwrap_or_default();

        (self.emit)(Event::Text {
            doc: self.info.id,
            page: index,
            text: Arc::new(text),
        });
    }

    /// Keeps what a search needs of a page's text, for the searches to come.
    fn remember(&mut self, index: u32, text: &PageText) {
        if let Some(haystack @ None) = self.haystacks.get_mut(index as usize) {
            *haystack = Some(text.haystack());
        }
    }

    fn saved_info(&self, path: &Path) -> DocInfo {
        let mut info = describe(&self.document, self.info.id, path);
        info.title = self.info.title.clone();
        info.outline = self.info.outline.clone();
        info
    }

    fn reset_document_state(&mut self, page_count: usize) {
        self.pages.clear();
        self.scratch = None;
        self.parsed = 0;
        self.sent.clear();
        self.haystacks = (0..page_count).map(|_| None).collect();
    }

    fn replace_document(&mut self, temp: &Path, dest: &Path) -> Result<(), String> {
        let recovery = self.document.save_to_bytes().map_err(|e| e.to_string())?;
        self.pages.clear();
        self.scratch = None;

        let placeholder = match self.pdfium.create_new_pdf() {
            Ok(document) => document,
            Err(error) => {
                let _ = fs::remove_file(temp);
                return Err(format!("无法关闭先前的 PDF：{error}"));
            }
        };
        let previous = std::mem::replace(&mut self.document, placeholder);
        drop(previous);

        if let Err(error) = publish_save(temp, dest) {
            self.document = self
                .pdfium
                .load_pdf_from_byte_vec(recovery, None)
                .map_err(|e| {
                    format!(
                        "保存失败，无法恢复编辑内容：{e}；临时副本位于 {}",
                        temp.display()
                    )
                })?;
            self.reset_document_state(self.info.pages.len());
            let _ = fs::remove_file(temp);
            return Err(error);
        }

        self.document = self
            .pdfium
            .load_pdf_from_file(dest, None)
            .or_else(|_| self.pdfium.load_pdf_from_byte_vec(recovery, None))
            .map_err(|error| format!("无法重新打开保存后的 PDF：{error}"))?;
        self.reset_document_state(self.info.pages.len());
        Ok(())
    }

    fn save_current(&mut self, dest: PathBuf) -> Result<Arc<DocInfo>, String> {
        if dest != self.info.path {
            atomic_save(&self.document, &dest)?;
            let info = Arc::new(self.saved_info(&dest));
            self.source_outline = info.outline.clone();
            self.source_title = info.title.clone();
            self.info = info.clone();
            self.source_baseline = None;
            self.modified = false;
            return Ok(info);
        }

        let temp = stage_save(&self.document, &dest)?;
        self.replace_document(&temp, &dest)?;

        let info = Arc::new(self.saved_info(&dest));
        self.source_outline = info.outline.clone();
        self.source_title = info.title.clone();
        self.info = info.clone();
        self.source_baseline = None;
        self.modified = false;
        Ok(info)
    }

    fn apply_edit(&mut self, request: EditRequest) {
        if request.doc != self.info.id {
            return;
        }

        match self.rebuild(&request.sources, &request.rotations) {
            Ok(document) => {
                let mut info = describe(&document, next_document_id(), &self.info.path);
                info.title = self.source_title.clone();
                info.outline = remap_outline(&self.source_outline, &request.sources);

                self.pages.clear();
                self.document = document;
                self.modified = true;
                self.scratch = None;
                self.parsed = 0;
                self.sent.clear();
                self.copy = None;
                self.search = None;
                self.haystacks = (0..info.pages.len()).map(|_| None).collect();
                self.info = Arc::new(info);

                if let Ok(mut queue) = self.shared.queue.lock() {
                    queue.doc = self.info.id;
                    queue.wanted = Wanted::default();
                }

                (self.emit)(Event::Edited(self.info.clone()));
            }
            Err(error) => (self.emit)(Event::EditFailed {
                doc: request.doc,
                error,
            }),
        }
    }

    fn rebuild(&self, sources: &[u32], rotations: &[(u32, u8)]) -> Result<PdfDocument<'p>, String> {
        let baseline = if let Some(source) = &self.source_baseline {
            self.pdfium
                .load_pdf_from_byte_vec(source.save_to_bytes().map_err(|e| e.to_string())?, None)
        } else {
            self.pdfium.load_pdf_from_file(&self.info.path, None)
        }
        .map_err(|error| format!("无法重新加载源 PDF：{error}"))?;
        self.rebuild_from(baseline, sources, rotations)
    }

    fn rebuild_from(
        &self,
        baseline: PdfDocument<'p>,
        sources: &[u32],
        rotations: &[(u32, u8)],
    ) -> Result<PdfDocument<'p>, String> {
        let page_count = baseline.pages().len() as u32;

        if sources.is_empty() || sources.len() > page_count as usize {
            return Err("PDF 必须至少保留一页。".into());
        }

        let unique: HashSet<_> = sources.iter().copied().collect();
        if unique.len() != sources.len() || sources.iter().any(|source| *source >= page_count) {
            return Err("所选页面已失效。".into());
        }
        if rotations
            .iter()
            .any(|(source, turns)| *source >= page_count || *turns >= 4)
        {
            return Err("页面旋转参数已失效。".into());
        }

        if sources.len() == page_count as usize && sources.iter().copied().eq(0..page_count) {
            let mut output = baseline;
            for (source, turns) in rotations {
                let mut page = output
                    .pages_mut()
                    .get(*source as PdfPageIndex)
                    .map_err(|error| format!("无法旋转第 {} 页：{error}", source + 1))?;
                let original = page.rotation().unwrap_or(PdfPageRenderRotation::None);
                page.set_rotation(rotate(original, *turns));
            }
            return Ok(output);
        }

        let mut output = self
            .pdfium
            .create_new_pdf()
            .map_err(|error| format!("无法创建编辑后的 PDF：{error}"))?;

        let range = sources
            .iter()
            .map(|source| (source + 1).to_string())
            .collect::<Vec<_>>()
            .join(",");
        output
            .pages_mut()
            .copy_pages_from_document(&baseline, &range, 0)
            .map_err(|error| format!("无法复制所选页面：{error}"))?;

        for (slot, source) in sources.iter().copied().enumerate() {
            if let Some((_, turns)) = rotations.iter().find(|(page, _)| *page == source) {
                let mut page = output
                    .pages_mut()
                    .get(slot as PdfPageIndex)
                    .map_err(|error| format!("无法旋转第 {} 页：{error}", slot + 1))?;
                let original = page.rotation().unwrap_or(PdfPageRenderRotation::None);
                page.set_rotation(rotate(original, *turns));
            }
        }

        Ok(output)
    }

    fn import_pages(&mut self, request: ImportRequest) {
        if request.edit.doc != self.info.id {
            return;
        }
        let result = (|| {
            let baseline = if let Some(source) = &self.source_baseline {
                self.pdfium.load_pdf_from_byte_vec(
                    source.save_to_bytes().map_err(|e| e.to_string())?,
                    None,
                )
            } else {
                self.pdfium.load_pdf_from_file(&self.info.path, None)
            }
            .map_err(|e| e.to_string())?;
            let mut combined = self.pdfium.create_new_pdf().map_err(|e| e.to_string())?;
            let first_source = baseline.pages().len() as u32;
            combined
                .pages_mut()
                .append(&baseline)
                .map_err(|e| e.to_string())?;
            let extension = request
                .path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if extension == "pdf" {
                let incoming = self
                    .pdfium
                    .load_pdf_from_file(&request.path, None)
                    .map_err(|e| describe_open_error(&e))?;
                if first_source + incoming.pages().len() as u32 > u16::MAX as u32 {
                    return Err("合并后的 PDF 页数超出支持范围".into());
                }
                combined
                    .pages_mut()
                    .append(&incoming)
                    .map_err(|e| e.to_string())?;
            } else {
                let image = raster::load_image(&request.path)?;
                if first_source == u16::MAX as u32 {
                    return Err("PDF 页数超出支持范围".into());
                }
                raster::append_image(&mut combined, &image)?;
            }
            let count = combined.pages().len() as u32 - first_source;
            if count == 0 {
                return Err("拖入的文件没有可插入页面".into());
            }
            if request.slot as usize > request.edit.sources.len() {
                return Err("插入位置已失效".into());
            }
            let mut sources = request.edit.sources.clone();
            sources.splice(
                request.slot as usize..request.slot as usize,
                first_source..first_source + count,
            );
            let copy = self
                .pdfium
                .load_pdf_from_byte_vec(combined.save_to_bytes().map_err(|e| e.to_string())?, None)
                .map_err(|e| e.to_string())?;
            let document = self.rebuild_from(copy, &sources, &request.edit.rotations)?;
            let mut info = describe(&document, next_document_id(), &self.info.path);
            info.title = self.source_title.clone();
            info.outline = remap_outline(&self.source_outline, &sources);
            self.reset_document_state(info.pages.len());
            self.document = document;
            self.source_baseline = Some(combined);
            self.modified = true;
            self.copy = None;
            self.search = None;
            self.info = Arc::new(info);
            if let Ok(mut queue) = self.shared.queue.lock() {
                queue.doc = self.info.id;
                queue.wanted = Wanted::default();
            }
            (self.emit)(Event::Imported {
                info: self.info.clone(),
                first_source,
                count,
                slot: request.slot,
            });
            Ok(())
        })();
        if let Err(error) = result {
            (self.emit)(Event::EditFailed {
                doc: request.edit.doc,
                error,
            });
        }
    }

    fn start(&mut self, job: Job) {
        let page_count = self.info.pages.len() as u32;

        self.job = Some(match job {
            Job::Raster { dest, settings } => {
                let same_path = fs::canonicalize(&dest)
                    .ok()
                    .zip(fs::canonicalize(&self.info.path).ok())
                    .is_some_and(|(a, b)| a == b);
                if same_path {
                    (self.emit)(Event::JobFailed {
                        kind: JobKind::Raster,
                        error: "请选择新路径，纯图 PDF 不覆盖源文档".into(),
                    });
                    return;
                }
                match raster::Output::new(self.pdfium, &dest, settings.format) {
                    Ok(output) => Active::Raster {
                        dest,
                        settings,
                        page: 0,
                        output,
                    },
                    Err(error) => {
                        (self.emit)(Event::JobFailed {
                            kind: JobKind::Raster,
                            error,
                        });
                        return;
                    }
                }
            }
            Job::Estimate { request, settings } => {
                let mut samples = vec![0, page_count / 2, page_count.saturating_sub(1)];
                samples.sort_unstable();
                samples.dedup();
                Active::Estimate {
                    request,
                    settings,
                    samples,
                    done: 0,
                    bytes: 0,
                    area: 0.0,
                }
            }
            Job::Export { dest } => Active::Export {
                dest,
                page: 0,
                pages: Vec::with_capacity(page_count as usize),
            },
            Job::Print(target) => match if self.modified {
                self.document
                    .save_to_bytes()
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| print::Session::begin_current(target, &self.info, bytes))
            } else {
                print::Session::begin(target, &self.info)
            } {
                Ok(session) => Active::Print(session),
                Err(error) => {
                    (self.emit)(Event::JobFailed {
                        kind: JobKind::Print,
                        error,
                    });
                    return;
                }
            },
            Job::Save { dest } => Active::Save { dest },
            Job::Compact { dest } => Active::Compact { dest },
            Job::Extract { dest, pages } => {
                if dest == self.info.path {
                    (self.emit)(Event::JobFailed {
                        kind: JobKind::Extract,
                        error: "请选择其他路径，以免替换当前 PDF。".into(),
                    });
                    return;
                }
                let valid = !pages.is_empty()
                    && pages.iter().all(|page| *page < page_count)
                    && pages.iter().copied().collect::<HashSet<_>>().len() == pages.len();
                let mut output = if valid {
                    self.pdfium
                        .create_new_pdf()
                        .map_err(|error| format!("无法创建提取的 PDF：{error}"))
                } else {
                    Err("所选页面已失效。".into())
                };

                if let Ok(document) = &mut output {
                    let range = pages
                        .iter()
                        .map(|page| (page + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    if let Err(error) =
                        document
                            .pages_mut()
                            .copy_pages_from_document(&self.document, &range, 0)
                    {
                        (self.emit)(Event::JobFailed {
                            kind: JobKind::Extract,
                            error: format!("无法复制所选页面：{error}"),
                        });
                        return;
                    }
                }

                match output {
                    Ok(output) => {
                        let total = pages.len() as u32;
                        (self.emit)(Event::Progress {
                            kind: JobKind::Extract,
                            done: total,
                            total,
                        });
                        Active::Extract {
                            dest,
                            total,
                            output,
                        }
                    }
                    Err(error) => {
                        (self.emit)(Event::JobFailed {
                            kind: JobKind::Extract,
                            error,
                        });
                        return;
                    }
                }
            }
        });
    }

    /// Appends one page's worth of selected text.
    fn copy_step(&mut self) {
        let Some(mut copy) = self.copy.take() else {
            return;
        };

        let CopyRequest {
            from,
            to,
            granularity,
        } = copy.request;
        let last = (self.info.pages.len() as u32).saturating_sub(1);

        if copy.page > to.page.min(last) || self.info.pages.is_empty() {
            (self.emit)(Event::Copied(
                copy.out.trim_end_matches(['\n', ' ']).to_owned(),
            ));
            return;
        }

        let page = copy.page;

        if let Some(text) = self
            .with_passing_page(page, |page| extract(page, false))
            .flatten()
        {
            let range = text::selected_range(&text, page, &from, &to, granularity);
            let piece = text.text(range);

            if !copy.out.is_empty() && !copy.out.ends_with('\n') && !piece.is_empty() {
                copy.out.push('\n');
            }
            copy.out.push_str(&piece);
        }

        copy.page += 1;
        self.copy = Some(copy);
    }

    /// Searches on: past the pages known not to hold the query, which cost
    /// next to nothing, and through one that might.
    fn search_step(&mut self) {
        let Some(mut search) = self.search.take() else {
            return;
        };

        let pages = self.info.pages.len() as u32;

        // Bounded, so that a long run of misses cannot hold up a tile.
        for _ in 0..SEARCH_STRIDE {
            let ruled_out = search.done < pages
                && self.haystacks[((search.from + search.done) % pages) as usize]
                    .as_ref()
                    .is_some_and(|haystack| !haystack.contains(&search.needle));

            if !ruled_out {
                break;
            }

            search.done += 1;
        }

        if search.done >= pages || search.found >= MATCH_LIMIT {
            (self.emit)(Event::SearchDone {
                doc: self.info.id,
                search: search.id,
                capped: search.found >= MATCH_LIMIT,
            });
            return;
        }

        let index = (search.from + search.done) % pages;

        // Still to be ruled out; the next step takes it from there.
        if self.haystacks[index as usize]
            .as_ref()
            .is_some_and(|haystack| !haystack.contains(&search.needle))
        {
            self.search = Some(search);
            return;
        }

        let text = self
            .with_passing_page(index, |page| extract(page, false))
            .flatten();

        if let Some(text) = text {
            self.remember(index, &text);

            let matches: Vec<Match> = text
                .find(&search.needle)
                .into_iter()
                .map(|range| Match {
                    page: index,
                    start: range.start,
                    rects: text.rects(range),
                })
                .filter(|found| !found.rects.is_empty())
                .take(MATCH_LIMIT - search.found)
                .collect();

            if !matches.is_empty() {
                search.found += matches.len();

                (self.emit)(Event::Matches {
                    doc: self.info.id,
                    search: search.id,
                    matches,
                });
            }
        }

        search.done += 1;
        self.search = Some(search);
    }

    /// Advances the active job by one page.
    fn step(&mut self) {
        let Some(job) = self.job.take() else {
            return;
        };

        let kind = job.kind();
        let page_count = self.info.pages.len() as u32;

        let outcome: Result<Option<Active>, String> = match job {
            Active::Raster {
                dest,
                settings,
                page,
                mut output,
            } => (|| {
                if page < page_count {
                    let size = self.info.pages[page as usize];
                    let image = raster::render(&self.document, page, size, settings)?;
                    output.add(page, size, &image, settings)?;
                    (self.emit)(Event::Progress {
                        kind,
                        done: page + 1,
                        total: page_count,
                    });
                    Ok(Some(Active::Raster {
                        dest,
                        settings,
                        page: page + 1,
                        output,
                    }))
                } else {
                    match &output {
                        raster::Output::Pdf(document) => atomic_save(document, &dest)?,
                        raster::Output::Images(_) => output.publish_images(&dest)?,
                    }
                    (self.emit)(Event::Exported(dest));
                    Ok(None)
                }
            })(),
            Active::Estimate {
                request,
                settings,
                samples,
                done,
                bytes,
                area,
            } => {
                let estimate = (|| {
                    if done < samples.len() {
                        let page = samples[done];
                        let size = self.info.pages[page as usize];
                        let image = raster::render(&self.document, page, size, settings)?;
                        let length = raster::jpeg(&image, settings.quality)?.len() as u64;
                        Ok(Some(Active::Estimate {
                            request,
                            settings,
                            samples,
                            done: done + 1,
                            bytes: bytes + length,
                            area: area + f64::from(size.width) * f64::from(size.height),
                        }))
                    } else {
                        let total_area: f64 = self
                            .info
                            .pages
                            .iter()
                            .map(|p| f64::from(p.width) * f64::from(p.height))
                            .sum();
                        let estimated = (bytes as f64 * total_area / area.max(1.0)) as u64
                            + u64::from(page_count) * 1024
                            + 4096;
                        (self.emit)(Event::RasterEstimated {
                            doc: self.info.id,
                            request,
                            bytes: Ok(estimated),
                        });
                        Ok(None)
                    }
                })();
                match estimate {
                    Ok(next) => Ok(next),
                    Err(error) => {
                        (self.emit)(Event::RasterEstimated {
                            doc: self.info.id,
                            request,
                            bytes: Err(error),
                        });
                        Ok(None)
                    }
                }
            }
            Active::Export {
                dest,
                page,
                mut pages,
            } => {
                if page < page_count {
                    let size = self.size(page).unwrap_or(PageSize {
                        width: 612.0,
                        height: 792.0,
                    });
                    let (text, rules) = self
                        .with_passing_page(page, |page| {
                            (extract(page, true).unwrap_or_default(), rules(page))
                        })
                        .unwrap_or_default();

                    pages.push(markdown::Page::new(&text, size, rules));

                    (self.emit)(Event::Progress {
                        kind,
                        done: page + 1,
                        total: page_count,
                    });

                    Ok(Some(Active::Export {
                        dest,
                        page: page + 1,
                        pages,
                    }))
                } else {
                    let output = markdown::render(&pages, &self.info.outline);

                    std::fs::write(&dest, output)
                        .map(|()| {
                            (self.emit)(Event::Exported(dest));
                            None
                        })
                        .map_err(|error| error.to_string())
                }
            }
            Active::Print(mut session) => match session.step() {
                Ok(Some((done, total))) => {
                    (self.emit)(Event::Progress { kind, done, total });
                    Ok(Some(Active::Print(session)))
                }
                Ok(None) => {
                    (self.emit)(Event::Printed);
                    Ok(None)
                }
                Err(error) => Err(error),
            },
            Active::Save { dest } => match self.save_current(dest.clone()) {
                Ok(info) => {
                    (self.emit)(Event::Saved { info, path: dest });
                    Ok(None)
                }
                Err(error) => Err(error),
            },
            Active::Compact { dest } => (|| {
                let before = fs::metadata(&self.info.path)
                    .map(|meta| meta.len())
                    .unwrap_or(0);
                let temp = stage_save(&self.document, &dest)?;
                let after = fs::metadata(&temp).map(|meta| meta.len()).unwrap_or(0);
                let same_path = dest == self.info.path;
                let replaced = !same_path || after < before;

                if same_path && after >= before {
                    let _ = fs::remove_file(&temp);
                } else if same_path {
                    self.replace_document(&temp, &dest)?;
                    let info = Arc::new(self.saved_info(&dest));
                    self.source_outline = info.outline.clone();
                    self.source_title = info.title.clone();
                    self.info = info.clone();
                    self.source_baseline = None;
                    self.modified = false;
                    (self.emit)(Event::Saved {
                        info,
                        path: dest.clone(),
                    });
                } else {
                    publish_save(&temp, &dest)?;
                }

                (self.emit)(Event::Compacted {
                    path: dest,
                    before,
                    after,
                    replaced,
                });
                Ok(None)
            })(),
            Active::Extract {
                dest,
                total,
                output,
            } => match atomic_save(&output, &dest) {
                Ok(()) => {
                    (self.emit)(Event::Extracted {
                        path: dest,
                        pages: total,
                    });
                    Ok(None)
                }
                Err(error) => Err(error),
            },
        };

        match outcome {
            Ok(next) => self.job = next,
            Err(error) => (self.emit)(Event::JobFailed { kind, error }),
        }
    }
}

fn rotate(original: PdfPageRenderRotation, quarter_turns: u8) -> PdfPageRenderRotation {
    let turns = match original {
        PdfPageRenderRotation::None => 0,
        PdfPageRenderRotation::Degrees90 => 1,
        PdfPageRenderRotation::Degrees180 => 2,
        PdfPageRenderRotation::Degrees270 => 3,
    };

    match (turns + quarter_turns) % 4 {
        0 => PdfPageRenderRotation::None,
        1 => PdfPageRenderRotation::Degrees90,
        2 => PdfPageRenderRotation::Degrees180,
        _ => PdfPageRenderRotation::Degrees270,
    }
}

fn remap_outline(outline: &[OutlineItem], sources: &[u32]) -> Vec<OutlineItem> {
    outline
        .iter()
        .filter_map(|item| {
            let page = item.page.and_then(|source| {
                sources
                    .iter()
                    .position(|candidate| *candidate == source)
                    .map(|slot| slot as u32)
            });
            (item.page.is_none() || page.is_some()).then(|| OutlineItem {
                title: item.title.clone(),
                depth: item.depth,
                page,
            })
        })
        .collect()
}

fn stage_save(document: &PdfDocument, dest: &Path) -> Result<PathBuf, String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    let parent = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.pdf".into());
    let temp = parent.join(format!(
        ".{name}.folio-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));

    let saved = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("无法创建临时 PDF：{error}"))?;
        document
            .save_to_writer(&mut file)
            .map_err(|error| format!("无法写入 PDF：{error}"))?;
        file.flush()
            .map_err(|error| format!("无法刷新 PDF 文件：{error}"))?;
        file.sync_all()
            .map_err(|error| format!("无法同步 PDF 文件：{error}"))
    })();

    if saved.is_err() {
        let _ = fs::remove_file(&temp);
    }

    saved.map(|()| temp)
}

fn publish_save(temp: &Path, dest: &Path) -> Result<(), String> {
    fs::rename(temp, dest).map_err(|error| format!("无法替换 {}：{error}", dest.display()))
}

fn atomic_save(document: &PdfDocument, dest: &Path) -> Result<(), String> {
    let temp = stage_save(document, dest)?;
    publish_save(&temp, dest).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

struct Region {
    page_width: u32,
    page_height: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Renders a region of a page straight into a buffer we own, so the pixels
/// travel to the GPU without an intermediate copy.
fn render(page: &PdfPage, region: Region) -> Option<Bitmap> {
    let mut rgba = vec![0xff_u8; region.width as usize * region.height as usize * 4];

    {
        let mut bitmap = PdfBitmap::from_bytes(
            region.width as Pixels,
            region.height as Pixels,
            PdfBitmapFormat::BGRA,
            &mut rgba,
        )
        .ok()?;

        let config = PdfRenderConfig::new()
            .set_fixed_size(region.page_width as Pixels, region.page_height as Pixels)
            .set_origin(-(region.x as Pixels), -(region.y as Pixels))
            // The buffer starts out opaque white already.
            .clear_before_rendering(false)
            .set_reverse_byte_order(true)
            .render_annotations(true);

        page.render_into_bitmap_with_config(&mut bitmap, &config)
            .ok()?;
    }

    Some(Bitmap {
        width: region.width,
        height: region.height,
        rgba,
    })
}

/// Maps PDF page space (origin bottom-left, unrotated, possibly offset by the
/// crop box) to view space (origin top-left, rotated as displayed).
struct ViewMap {
    origin: (f32, f32),
    x_axis: (f32, f32),
    y_axis: (f32, f32),
}

impl ViewMap {
    fn new(page: &PdfPage) -> Self {
        // Probe PDFium's own transform at a high scale and recover the
        // affine map from three points. Page rotation is always a multiple
        // of 90°, so the axes can be snapped to exact unit vectors.
        const SCALE: f32 = 64.0;
        const SPAN: f32 = 1024.0;

        let config = PdfRenderConfig::new().scale_page_by_factor(SCALE);
        let probe = |x: f32, y: f32| {
            page.points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &config)
                .map(|(x, y)| (x as f32 / SCALE, y as f32 / SCALE))
        };

        match (probe(0.0, 0.0), probe(SPAN, 0.0), probe(0.0, SPAN)) {
            (Ok(origin), Ok(x), Ok(y)) => Self {
                origin,
                x_axis: (
                    ((x.0 - origin.0) / SPAN).round(),
                    ((x.1 - origin.1) / SPAN).round(),
                ),
                y_axis: (
                    ((y.0 - origin.0) / SPAN).round(),
                    ((y.1 - origin.1) / SPAN).round(),
                ),
            },
            _ => Self {
                origin: (0.0, page.height().value),
                x_axis: (1.0, 0.0),
                y_axis: (0.0, -1.0),
            },
        }
    }

    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.origin.0 + self.x_axis.0 * x + self.y_axis.0 * y,
            self.origin.1 + self.x_axis.1 * x + self.y_axis.1 * y,
        )
    }

    /// Whether text running at `angle` in page space ends up anything other
    /// than left-to-right on screen.
    fn is_rotated(&self, angle: f32) -> bool {
        let (sin, cos) = angle.sin_cos();
        let across = self.x_axis.0 * cos + self.y_axis.0 * sin;
        let down = self.x_axis.1 * cos + self.y_axis.1 * sin;

        down.abs() > 0.2 || across < 0.0
    }

    fn rect(&self, rect: &PdfRect) -> Rect {
        let a = self.point(rect.left().value, rect.bottom().value);
        let b = self.point(rect.right().value, rect.top().value);

        Rect {
            x0: a.0.min(b.0),
            y0: a.1.min(b.1),
            x1: a.0.max(b.0),
            y1: a.1.max(b.1),
        }
    }
}

/// Paths with more segments than this are artwork, not ruling.
const RULE_PATH_SEGMENTS: usize = 4_000;
/// Upper bound on segments examined per page, so a page of dense vector art
/// cannot stall an export.
const RULE_PAGE_SEGMENTS: usize = 40_000;
/// Anything at most this thick is a line, however it was painted.
const RULE_THICKNESS: f32 = 3.0;
const RULE_MIN_LENGTH: f32 = 6.0;

/// Collects the straight horizontal and vertical lines drawn on a page, in
/// view space. These are what tables are recognised by.
fn rules(page: &PdfPage) -> Vec<Rule> {
    let map = ViewMap::new(page);
    let mut out = Vec::new();
    let mut budget = RULE_PAGE_SEGMENTS;

    collect_rules(page.objects().iter(), &[], &map, &mut budget, &mut out);
    out
}

fn collect_rules<'a>(
    objects: impl Iterator<Item = PdfPageObject<'a>>,
    // Matrices of the enclosing form objects, innermost first.
    forms: &[PdfMatrix],
    map: &ViewMap,
    budget: &mut usize,
    out: &mut Vec<Rule>,
) {
    for object in objects {
        if let Some(form) = object.as_x_object_form_object() {
            if forms.len() < 4
                && let Ok(matrix) = form.matrix()
            {
                let mut nested = vec![matrix];
                nested.extend_from_slice(forms);
                collect_rules(form.iter(), &nested, map, budget, out);
            }
            continue;
        }

        let Some(path) = object.as_path_object() else {
            continue;
        };

        let segments = path.segments();
        let count = segments.len() as usize;

        if !(2..=RULE_PATH_SEGMENTS).contains(&count) || count > *budget {
            continue;
        }

        *budget -= count;

        let Ok(own) = path.matrix() else {
            continue;
        };

        // Path coordinates are local: apply the path's matrix, then each
        // enclosing form's, then the page's own transform.
        let place = |x: PdfPoints, y: PdfPoints| {
            let mut point = own.apply_to_points(x, y);

            for form in forms {
                point = form.apply_to_points(point.0, point.1);
            }

            map.point(point.0.value, point.1.value)
        };

        let mut edges: Vec<((f32, f32), (f32, f32))> = Vec::new();
        let mut start = (0.0, 0.0);
        let mut current = (0.0, 0.0);
        let mut bounds = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        let mut curved = false;

        for segment in segments.iter() {
            let point = place(segment.x(), segment.y());

            bounds = (
                bounds.0.min(point.0),
                bounds.1.min(point.1),
                bounds.2.max(point.0),
                bounds.3.max(point.1),
            );

            match segment.segment_type() {
                PdfPathSegmentType::MoveTo => start = point,
                PdfPathSegmentType::LineTo => edges.push((current, point)),
                _ => curved = true,
            }

            current = point;

            if segment.is_close() {
                edges.push((current, start));
                current = start;
            }
        }

        let (width, height) = (bounds.2 - bounds.0, bounds.3 - bounds.1);

        // A sliver, stroked or filled, is a single rule along its length.
        if width.min(height) <= RULE_THICKNESS {
            if !curved && width.max(height) >= RULE_MIN_LENGTH {
                out.push(if width >= height {
                    Rule {
                        horizontal: true,
                        at: (bounds.1 + bounds.3) * 0.5,
                        from: bounds.0,
                        to: bounds.2,
                    }
                } else {
                    Rule {
                        horizontal: false,
                        at: (bounds.0 + bounds.2) * 0.5,
                        from: bounds.1,
                        to: bounds.3,
                    }
                });
            }
            continue;
        }

        // Otherwise only outlines count: the edges of a stroked box or of a
        // whole grid drawn as one path. A filled area is a background.
        if !path.is_stroked().unwrap_or(false) {
            continue;
        }

        let straight: Vec<Rule> = edges
            .iter()
            .filter_map(|(a, b)| {
                let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());

                if dy <= 0.7 && dx >= RULE_MIN_LENGTH {
                    Some(Rule {
                        horizontal: true,
                        at: (a.1 + b.1) * 0.5,
                        from: a.0.min(b.0),
                        to: a.0.max(b.0),
                    })
                } else if dx <= 0.7 && dy >= RULE_MIN_LENGTH {
                    Some(Rule {
                        horizontal: false,
                        at: (a.0 + b.0) * 0.5,
                        from: a.1.min(b.1),
                        to: a.1.max(b.1),
                    })
                } else {
                    None
                }
            })
            .collect();

        // A box with rounded corners: its four sides stop short of one
        // another, so give the frame its full extent or they would read as
        // unrelated rules.
        let sides = edges
            .iter()
            .filter(|(a, b)| (b.0 - a.0).abs().max((b.1 - a.1).abs()) > 0.5)
            .count();

        if curved && straight.len() == 4 && sides == 4 {
            for (horizontal, at) in [
                (true, bounds.1),
                (true, bounds.3),
                (false, bounds.0),
                (false, bounds.2),
            ] {
                out.push(Rule {
                    horizontal,
                    at,
                    from: if horizontal { bounds.0 } else { bounds.1 },
                    to: if horizontal { bounds.2 } else { bounds.3 },
                });
            }
        } else {
            out.extend(straight);
        }
    }
}

/// Reads the text layer. `styled` additionally resolves font size and style
/// per glyph, which costs extra library calls and is only needed for export.
fn extract(page: &PdfPage, styled: bool) -> Option<PageText> {
    let map = ViewMap::new(page);
    let text = page.text().ok()?;
    let chars = text.chars();
    let mut glyphs = Vec::with_capacity(chars.len());
    let mut fonts = Fonts::default();
    // Which font each glyph uses, parallel to `glyphs`.
    let mut glyph_fonts: Vec<usize> = Vec::new();

    for char in chars.iter() {
        let ch = match char.unicode_char() {
            Some('\r') | None => continue,
            Some('\n') => '\n',
            // PDFium reports a hyphen that breaks a word across lines as U+0002.
            Some('\u{2}') => '-',
            Some('\u{a0}') => ' ',
            Some(ch) if ch.is_control() || matches!(ch, '\u{fffe}' | '\u{ffff}') => continue,
            Some(ch) => ch,
        };

        let rect = if ch.is_whitespace() {
            Rect::default()
        } else {
            char.loose_bounds()
                .or_else(|_| char.tight_bounds())
                .map(|bounds| map.rect(&bounds))
                .unwrap_or_default()
        };

        let mut glyph = Glyph {
            ch,
            rect,
            size: 0.0,
            style: 0,
        };

        if styled {
            let mut font = usize::MAX;

            if !rect.is_empty() {
                glyph.size = char.scaled_font_size().value;
                font = fonts.intern(&char);
                glyph.style = fonts.entries[font].style;
                fonts.entries[font].observe(ch, rect.width() / glyph.size.max(0.1));

                if map.is_rotated(char.angle_radians().unwrap_or(0.0)) {
                    glyph.style |= markdown::ROTATED;
                }
            }

            glyph_fonts.push(font);
        }

        glyphs.push(glyph);
    }

    // Judged once per font, not once per glyph: it sorts every advance the
    // font was seen at.
    let monospaced: Vec<bool> = fonts.entries.iter().map(Font::is_monospaced).collect();

    for (glyph, font) in glyphs.iter_mut().zip(&glyph_fonts) {
        if monospaced.get(*font).copied().unwrap_or(false) {
            glyph.style |= text::MONO;
        }
    }

    Some(PageText::new(glyphs))
}

/// Reads the links a page marks, in the order PDFium paints its annotations.
/// `pages` is the document's, for placing where each link leads.
fn links(page: &PdfPage, pages: &[PageSize]) -> Vec<Link> {
    let annotations = page.annotations();
    let mut map = None;
    let mut links = Vec::new();

    // By place among the annotations of every kind: the library's own list
    // of links counts the same way, and so repeats a link once for every
    // other annotation ahead of it.
    for index in 0..annotations.len() {
        if links.len() >= LINK_LIMIT {
            break;
        }

        let Ok(annotation) = annotations.get(index) else {
            continue;
        };
        let Some(link) = annotation
            .as_link_annotation()
            .and_then(|annotation| annotation.link().ok())
        else {
            continue;
        };
        let Some(target) = target(&link, pages) else {
            continue;
        };
        let Ok(bounds) = annotation.bounds() else {
            continue;
        };

        let rect = map.get_or_insert_with(|| ViewMap::new(page)).rect(&bounds);

        if !rect.is_empty() {
            links.push(Link { rect, target });
        }
    }

    links.shrink_to_fit();
    links
}

/// Where a link leads, if it is somewhere a reader may be taken: a place in
/// this document or an address. A link that would run a program or open
/// another file leads nowhere.
fn target(link: &PdfLink, pages: &[PageSize]) -> Option<Target> {
    let destination = match link.action() {
        // A destination named outright, without an action.
        None => link.destination(),
        Some(action) => match action.action_type() {
            PdfActionType::GoToDestinationInSameDocument => link.destination(),
            PdfActionType::Uri => {
                let uri = match action.as_uri_action()?.uri() {
                    Ok(uri) => uri,
                    // Not UTF-8: an address with a letter outside ASCII,
                    // from a producer that writes one byte to the letter.
                    Err(PdfiumError::CStringConversionError(error)) => error
                        .into_cstring()
                        .as_bytes()
                        .iter()
                        .map(|byte| char::from(*byte))
                        .collect(),
                    Err(_) => return None,
                };

                return (!uri.is_empty() && uri.len() <= URI_LIMIT)
                    .then(|| Target::Uri(uri.into_boxed_str()));
            }
            // PDFium reads a destination out of these too, but the page it
            // names is one of another document.
            _ => None,
        },
    }?;

    let page = u32::try_from(destination.page_index().ok()?).ok()?;
    let size = pages.get(page as usize)?;

    let top = match destination.view_settings().ok()? {
        PdfDestinationViewSettings::SpecificCoordinatesAndZoom(_, top, _)
        | PdfDestinationViewSettings::FitPageHorizontallyToWindow(top)
        | PdfDestinationViewSettings::FitBoundsHorizontallyToWindow(top) => {
            top.map(|top| top.value)
        }
        PdfDestinationViewSettings::FitPageToRectangle(rect) => Some(rect.top().value),
        _ => None,
    };

    Some(Target::Page {
        page,
        // Measured up from the foot of the page, which is right unless that
        // page is rotated or cropped; finding out would mean parsing it.
        top: top.map(|top| (size.height - top).clamp(0.0, size.height)),
    })
}

/// Per-page font table. Style is a property of the font, so it is resolved
/// once per font rather than once per glyph.
#[derive(Default)]
struct Fonts {
    entries: Vec<Font>,
}

struct Font {
    name: String,
    style: u8,
    /// Advance of each glyph seen, in ems.
    advances: Vec<f32>,
    /// Whether letters that differ in width in any proportional design have
    /// both been seen: without them, equal advances prove nothing (digits
    /// alone are the same width in most fonts).
    saw_narrow: bool,
    saw_wide: bool,
}

impl Font {
    fn observe(&mut self, ch: char, advance: f32) {
        if advance > 0.05 {
            self.advances.push(advance);
            self.saw_narrow |= matches!(
                ch,
                'i' | 'l' | 'I' | 'j' | 't' | 'f' | '.' | ',' | ':' | ';' | '\'' | '!' | '|'
            );
            self.saw_wide |= matches!(
                ch,
                'm' | 'w' | 'M' | 'W' | '@' | 'G' | 'O' | 'D' | 'H' | 'N'
            );
        }
    }

    /// Many embedded fonts do not declare themselves fixed pitch; uniform
    /// advances give them away. Nearly uniform is enough: a fixed-pitch
    /// font may still carry the odd wide glyph.
    fn is_monospaced(&self) -> bool {
        if self.style & text::MONO != 0 {
            return true;
        }

        if self.advances.len() < 12 || !self.saw_narrow || !self.saw_wide {
            return false;
        }

        let mut sorted = self.advances.clone();
        sorted.sort_by(f32::total_cmp);

        let median = sorted[sorted.len() / 2];
        let typical = sorted
            .iter()
            .filter(|advance| (**advance - median).abs() <= median * 0.03)
            .count();

        typical * 100 >= sorted.len() * 96
    }
}

impl Fonts {
    fn intern(&mut self, char: &PdfPageTextChar) -> usize {
        let name = char.font_name();

        if let Some(index) = self.entries.iter().position(|font| font.name == name) {
            return index;
        }

        self.entries.push(Font {
            style: style_of(char, &name.to_ascii_lowercase()),
            name,
            advances: Vec::new(),
            saw_narrow: false,
            saw_wide: false,
        });

        self.entries.len() - 1
    }
}

fn style_of(char: &PdfPageTextChar, name: &str) -> u8 {
    let mut style = 0;

    let heavy = matches!(
        char.font_weight(),
        Some(
            PdfFontWeight::Weight600
                | PdfFontWeight::Weight700Bold
                | PdfFontWeight::Weight800
                | PdfFontWeight::Weight900
        )
    );

    if heavy
        || ["bold", "black", "heavy", "semibold", "demi"]
            .iter()
            .any(|hint| name.contains(hint))
    {
        style |= text::BOLD;
    }

    if char.font_is_italic() || name.contains("italic") || name.contains("oblique") {
        style |= text::ITALIC;
    }

    if char.font_is_fixed_pitch()
        || ["mono", "courier", "consolas", "menlo"]
            .iter()
            .any(|hint| name.contains(hint))
    {
        style |= text::MONO;
    }

    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycling_an_edited_document_keeps_its_content_and_returns_to_idle() {
        let _exclusive = exclusive();
        let pdfium = pdfium().as_ref().unwrap();
        let path = crate::engine::tests::fixture_path();
        let source = pdfium.load_pdf_from_file(&path, None).unwrap();
        let expected = source.pages().get(2).unwrap().text().unwrap().all();
        let mut document = pdfium.create_new_pdf().unwrap();
        document
            .pages_mut()
            .copy_pages_from_document(&source, "3,1,2", 0)
            .unwrap();
        let info = Arc::new(describe(&document, 1, &path));
        let shared = Shared {
            queue: Mutex::new(super::super::Queue::default()),
            wake: std::sync::Condvar::new(),
        };
        let emit: Emit = Box::new(|_| {});
        let mut session = Session {
            pdfium,
            pages: VecDeque::new(),
            document,
            source_baseline: None,
            modified: true,
            parsed: RECYCLE_AFTER,
            scratch: None,
            haystacks: vec![None, None, None],
            source_outline: Vec::new(),
            source_title: String::new(),
            info,
            shared: &shared,
            emit: &emit,
            sent: VecDeque::new(),
            tiles_sent: 0,
            copy: None,
            search: None,
            job: None,
        };
        session.tidy();
        assert_eq!(session.parsed, 0);
        assert!(session.scratch.is_none());
        assert_eq!(
            session
                .document
                .pages()
                .get(0)
                .unwrap()
                .text()
                .unwrap()
                .all(),
            expected
        );
    }
}
