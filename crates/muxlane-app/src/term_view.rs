//! 终端视图：真彩色 cell renderer + 光标 + 低延迟 PTY 输入。
//! TermView 在 GPUI task 内 drain PTY 输出，chunk 到达即 process + notify。
use crate::theme::Theme;
use gpui::{
    canvas, div, fill, point, prelude::*, px, rgba, size, App, Bounds, ClipboardEntry,
    ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, Font, FontFallbacks,
    FontFeatures, FontStyle, FontWeight, Hsla, ImageFormat, InputHandler, MouseButton,
    ParentElement, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, ShapedLine, Styled, Task,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use muxlane_core::model::AgentId;
use muxlane_term::{PtySession, VTerm};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const FONT_SIZE: f32 = 13.0;
const TERM_PADDING_X: f32 = 12.0;
const TERM_PADDING_Y: f32 = 8.0;
const FALLBACK_CELL_W: f32 = 8.2;
const FALLBACK_CELL_H: f32 = 17.0;
const SCROLLBAR_WIDTH: f32 = 10.0;
const SCROLLBAR_RIGHT: f32 = 2.0;
const MIN_SCROLLBAR_THUMB: f32 = 24.0;
const MAX_OSC52_CLIPBOARD_BYTES: usize = 64 * 1024;

fn osc52_clipboard_allowed(enabled: bool, text: &str) -> bool {
    if text.len() > MAX_OSC52_CLIPBOARD_BYTES {
        tracing::warn!(
            size = text.len(),
            max = MAX_OSC52_CLIPBOARD_BYTES,
            "discarding oversized OSC52 clipboard payload"
        );
        return false;
    }
    enabled
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollbarGeometry {
    track_height: f32,
    thumb_top: f32,
    thumb_height: f32,
}

fn scrollbar_geom(
    track_height: f32,
    visible_lines: usize,
    history: usize,
    display_offset: usize,
) -> Option<ScrollbarGeometry> {
    if history == 0 || track_height <= 0.0 {
        return None;
    }

    let visible_lines = visible_lines.max(1);
    let total_lines = history.saturating_add(visible_lines);
    let proportional = track_height * visible_lines as f32 / total_lines as f32;
    let thumb_height = proportional
        .max(MIN_SCROLLBAR_THUMB.min(track_height))
        .min(track_height);
    let travel = (track_height - thumb_height).max(0.0);
    let offset = display_offset.min(history);
    let thumb_top = travel * (history - offset) as f32 / history as f32;

    Some(ScrollbarGeometry {
        track_height,
        thumb_top,
        thumb_height,
    })
}

fn scrollbar_drag_offset(
    geometry: ScrollbarGeometry,
    pointer_y: f32,
    grab_offset: f32,
    history: usize,
    display_offset: usize,
) -> usize {
    let travel = (geometry.track_height - geometry.thumb_height).max(0.0);
    if travel == 0.0 {
        return display_offset.min(history);
    }
    let thumb_top = (pointer_y - grab_offset).clamp(0.0, travel);
    ((1.0 - thumb_top / travel) * history as f32).round() as usize
}

struct PaintRun {
    shaped: ShapedLine,
    start_col: usize,
    row: usize,
    cells: usize,
    bg: Hsla,
    selected: bool,
}

struct TerminalPaintState {
    inner: Bounds<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    base_half: Pixels,
    runs: Vec<PaintRun>,
    visible_cursor: Option<Bounds<Pixels>>,
    logical_cursor: Option<Bounds<Pixels>>,
}

#[derive(Debug)]
pub enum RemoteTermCommand {
    Input(Vec<u8>),
    Resize(u16, u16),
}

#[derive(Clone)]
enum InputSink {
    Local(Arc<PtySession>),
    Remote(tokio::sync::mpsc::UnboundedSender<RemoteTermCommand>),
}

impl InputSink {
    fn write(&self, bytes: &[u8]) {
        match self {
            InputSink::Local(session) => session.write_input(bytes),
            InputSink::Remote(sender) => {
                let _ = sender.send(RemoteTermCommand::Input(bytes.to_vec()));
            }
        }
    }
}

struct TerminalInputHandler {
    sink: InputSink,
    marked_text: Arc<std::sync::Mutex<Option<String>>>,
    cursor_bounds: Option<Bounds<Pixels>>,
}

fn set_terminal_preedit(marked: &std::sync::Mutex<Option<String>>, text: &str) {
    if let Ok(mut current) = marked.lock() {
        *current = (!text.is_empty()).then(|| text.to_string());
    }
}

fn take_terminal_preedit(marked: &std::sync::Mutex<Option<String>>) -> Option<String> {
    marked.lock().ok().and_then(|mut current| current.take())
}

fn commit_terminal_text(marked: &std::sync::Mutex<Option<String>>, text: &str) -> Vec<u8> {
    if let Ok(mut current) = marked.lock() {
        *current = None;
    }
    text.as_bytes().to_vec()
}

impl InputHandler for TerminalInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &mut self,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<std::ops::Range<usize>> {
        self.marked_text
            .lock()
            .ok()
            .and_then(|marked| marked.as_ref().map(|text| 0..text.encode_utf16().count()))
    }

    fn text_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        _adjusted: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        let bytes = commit_terminal_text(&self.marked_text, text);
        if !bytes.is_empty() {
            self.sink.write(&bytes);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        // Preedit 是可替换的组合态，不能提前写入 PTY；最终 commit 走 replace_text。
        set_terminal_preedit(&self.marked_text, new_text);
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut App) {
        if let Some(text) = take_terminal_preedit(&self.marked_text) {
            self.sink.write(text.as_bytes());
        }
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_bounds
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        None
    }

    fn accepts_text_input(&mut self, _window: &mut Window, _cx: &mut App) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct TermEnterEvent(pub AgentId);

pub struct TermView {
    pub agent: AgentId,
    font_family: String,
    theme: Theme,
    pub vterm: VTerm,
    pub focus: FocusHandle,
    writer: Option<Arc<PtySession>>,
    remote_input: Option<tokio::sync::mpsc::UnboundedSender<RemoteTermCommand>>,
    last_dims: Arc<std::sync::Mutex<(u16, u16)>>,
    last_bounds: Arc<std::sync::Mutex<Option<Bounds<Pixels>>>>,
    cell_size: Arc<std::sync::Mutex<(f32, f32)>>,
    scroll_accum: Arc<std::sync::Mutex<f32>>,
    scrollbar_drag: Option<f32>,
    selecting: bool,
    forwarding_mouse: bool,
    marked_text: Arc<std::sync::Mutex<Option<String>>>,
    osc52_clipboard_enabled: Arc<AtomicBool>,
    _drain: Task<()>,
    _clipboard: Task<()>,
}

impl EventEmitter<TermEnterEvent> for TermView {}

impl Focusable for TermView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

fn paste_image_path(format: ImageFormat) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        std::path::PathBuf::from(format!(
            "/tmp/muxlane-paste-{}.{}",
            muxlane_core::model::new_id("image"),
            format.extension()
        ))
    }

    #[cfg(not(unix))]
    {
        std::env::temp_dir().join(format!(
            "muxlane-paste-{}.{}",
            muxlane_core::model::new_id("image"),
            format.extension()
        ))
    }
}

fn write_paste_image(image: &gpui::Image) -> Option<std::path::PathBuf> {
    let path = paste_image_path(image.format());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).ok()?;
    if file.write_all(image.bytes()).is_err() {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(path)
}

impl TermView {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_primary(cx: &mut Context<Self>, text: String) {
        cx.write_to_primary(ClipboardItem::new_string(text));
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn write_primary(_cx: &mut Context<Self>, _text: String) {}

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_primary(cx: &mut Context<Self>) -> Option<String> {
        cx.read_from_primary().and_then(|item| item.text())
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn read_primary(_cx: &mut Context<Self>) -> Option<String> {
        None
    }

    fn clipboard_task(
        mut clipboard_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
        enabled: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |view, cx| {
            while let Some(text) = clipboard_rx.recv().await {
                if !osc52_clipboard_allowed(enabled.load(Ordering::Relaxed), &text) {
                    continue;
                }
                let stop = view
                    .update(cx, move |_view, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        Self::write_primary(cx, text);
                    })
                    .is_err();
                if stop {
                    break;
                }
            }
        })
    }

    /// 本地 PTY：订阅 replay+增量，输出到达即 `cx.notify()`，无 1 秒轮询。
    pub fn new_local(
        agent: AgentId,
        session: Arc<PtySession>,
        font_family: String,
        theme: Theme,
        osc52_clipboard_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let (vterm, clipboard_rx) = VTerm::new_with_clipboard(120, 32);
        let osc52_clipboard_enabled = Arc::new(AtomicBool::new(osc52_clipboard_enabled));
        let clipboard =
            Self::clipboard_task(clipboard_rx, Arc::clone(&osc52_clipboard_enabled), cx);
        let (replay, mut rx) = session.subscribe();
        vterm.feed(&replay);
        let vterm_for_task = vterm.clone();
        let session_for_task = Arc::clone(&session);
        let drain = cx.spawn(async move |view, cx| {
            loop {
                let first = match rx.recv().await {
                    Ok(b) => b,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let (snapshot, synced_rx) = session_for_task.subscribe();
                        rx = synced_rx;
                        let vterm = vterm_for_task.clone();
                        let stop = view
                            .update(cx, move |_view, cx| {
                                vterm.feed(b"\x1bc");
                                vterm.feed(&snapshot);
                                cx.notify();
                            })
                            .is_err();
                        if stop {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                // 输出调度：交互 8ms / 聚焦 stream 33ms / 后台 100ms。
                let delay = if session_for_task.interaction_recent() {
                    std::time::Duration::from_millis(8)
                } else if session_for_task.is_focused() {
                    std::time::Duration::from_millis(33)
                } else {
                    std::time::Duration::from_millis(100)
                };
                cx.background_executor().timer(delay).await;
                let mut output = first.to_vec();
                let mut lagged = false;
                while output.len() < 256 * 1024 {
                    match rx.try_recv() {
                        Ok(b) => output.extend_from_slice(&b),
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                            lagged = true;
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if lagged {
                    let (snapshot, synced_rx) = session_for_task.subscribe();
                    rx = synced_rx;
                    let vterm = vterm_for_task.clone();
                    let stop = view
                        .update(cx, move |_view, cx| {
                            vterm.feed(b"\x1bc");
                            vterm.feed(&snapshot);
                            cx.notify();
                        })
                        .is_err();
                    if stop {
                        break;
                    }
                    continue;
                }
                let vterm = vterm_for_task.clone();
                let stop = view
                    .update(cx, move |_view, cx| {
                        vterm.feed(&output);
                        cx.notify();
                    })
                    .is_err();
                if stop {
                    break;
                }
            }
        });
        Self {
            agent,
            font_family,
            theme,
            vterm,
            focus: cx.focus_handle(),
            writer: Some(session),
            remote_input: None,
            last_dims: Arc::new(std::sync::Mutex::new((120, 32))),
            last_bounds: Arc::new(std::sync::Mutex::new(None)),
            cell_size: Arc::new(std::sync::Mutex::new((FALLBACK_CELL_W, FALLBACK_CELL_H))),
            scroll_accum: Arc::new(std::sync::Mutex::new(0.0)),
            scrollbar_drag: None,
            selecting: false,
            forwarding_mouse: false,
            marked_text: Arc::new(std::sync::Mutex::new(None)),
            osc52_clipboard_enabled,
            _drain: drain,
            _clipboard: clipboard,
        }
    }

    /// 远程镜像：P2 客户端把 TermData 喂入同一个 VTerm；输入保持 None（v1 只读）。
    pub fn new_remote(
        agent: AgentId,
        terminal: (VTerm, tokio::sync::mpsc::UnboundedReceiver<String>),
        remote_input: tokio::sync::mpsc::UnboundedSender<RemoteTermCommand>,
        font_family: String,
        theme: Theme,
        osc52_clipboard_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let (vterm, clipboard_rx) = terminal;
        let osc52_clipboard_enabled = Arc::new(AtomicBool::new(osc52_clipboard_enabled));
        let clipboard =
            Self::clipboard_task(clipboard_rx, Arc::clone(&osc52_clipboard_enabled), cx);
        let idle = cx.spawn(async move |_view, _cx| {
            std::future::pending::<()>().await;
        });
        Self {
            agent,
            font_family,
            theme,
            vterm,
            focus: cx.focus_handle(),
            writer: None,
            remote_input: Some(remote_input),
            last_dims: Arc::new(std::sync::Mutex::new((120, 32))),
            last_bounds: Arc::new(std::sync::Mutex::new(None)),
            cell_size: Arc::new(std::sync::Mutex::new((FALLBACK_CELL_W, FALLBACK_CELL_H))),
            scroll_accum: Arc::new(std::sync::Mutex::new(0.0)),
            scrollbar_drag: None,
            selecting: false,
            forwarding_mouse: false,
            marked_text: Arc::new(std::sync::Mutex::new(None)),
            osc52_clipboard_enabled,
            _drain: idle,
            _clipboard: clipboard,
        }
    }

    pub fn set_osc52_clipboard_enabled(&mut self, enabled: bool) {
        self.osc52_clipboard_enabled
            .store(enabled, Ordering::Relaxed);
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn set_font_family(&mut self, font_family: String, cx: &mut Context<Self>) {
        self.font_family = font_family;
        cx.notify();
    }

    fn input_sink(&self) -> Option<InputSink> {
        self.writer
            .as_ref()
            .map(|session| InputSink::Local(Arc::clone(session)))
            .or_else(|| self.remote_input.clone().map(InputSink::Remote))
    }

    fn grid_point(&self, position: Point<Pixels>) -> Option<(i32, usize, bool)> {
        let bounds = self.last_bounds.lock().ok().and_then(|bounds| *bounds)?;
        let (cell_w, cell_h) = self
            .cell_size
            .lock()
            .map(|size| *size)
            .unwrap_or((FALLBACK_CELL_W, FALLBACK_CELL_H));
        let (cols, rows) = self.last_dims.lock().map(|dims| *dims).unwrap_or((2, 2));
        let local = position - bounds.origin;
        let col = (f32::from(local.x).max(0.0) / cell_w) as usize;
        let visual_row = (f32::from(local.y).max(0.0) / cell_h) as usize;
        let (_, offset) = self.vterm.scroll_metrics();
        let row = visual_row.min(rows.saturating_sub(1) as usize) as i32 - offset as i32;
        Some((
            row,
            col.min(cols.saturating_sub(1) as usize),
            f32::from(local.x) >= cell_w * (col as f32 + 0.5),
        ))
    }

    fn finish_mouse(&mut self, event: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        let mut handled = false;
        if self.forwarding_mouse {
            if let (Some(sink), Some((line, col, _))) =
                (self.input_sink(), self.grid_point(event.position))
            {
                let sgr = self.vterm.modes().mouse && self.vterm.modes().sgr_mouse;
                sink.write(&mouse_report(
                    0,
                    col,
                    line.max(0) as usize,
                    sgr,
                    false,
                    event.modifiers.shift,
                    event.modifiers.alt,
                    event.modifiers.control,
                    false,
                ));
            }
            self.forwarding_mouse = false;
            handled = true;
        }
        if event.button == MouseButton::Left && self.selecting {
            self.selecting = false;
            if let Some(text) = self
                .vterm
                .selection_to_string()
                .filter(|text| !text.is_empty())
            {
                Self::write_primary(cx, text);
            }
            handled = true;
        }
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn report_mouse(
        &self,
        button: u8,
        col: usize,
        line: i32,
        pressed: bool,
        motion: bool,
        shift: bool,
        alt: bool,
        control: bool,
    ) {
        let Some(sink) = self.input_sink() else {
            return;
        };
        // tmux 作为外层终端默认认 X10 鼠标协议；应用自开 SGR 时再切 SGR。
        let sgr = self.vterm.modes().mouse && self.vterm.modes().sgr_mouse;
        sink.write(&mouse_report(
            button,
            col,
            line.max(0) as usize,
            sgr,
            pressed,
            shift,
            alt,
            control,
            motion,
        ));
    }

    fn paste_clipboard(&self, cx: &mut Context<Self>) {
        let clipboard = cx.read_from_clipboard();
        let text = match clipboard {
            Some(item) => {
                if let Some(text) = item.text().filter(|text| !text.is_empty()) {
                    Some(text)
                } else {
                    item.entries().iter().find_map(|entry| match entry {
                        ClipboardEntry::Image(image) => {
                            write_paste_image(image).map(|path| path.to_string_lossy().into_owned())
                        }
                        _ => None,
                    })
                }
            }
            None => read_system_clipboard_text()
                .or_else(|| Self::read_primary(cx))
                .filter(|text| !text.is_empty()),
        };
        let Some(text) = text else {
            return;
        };
        let Some(sink) = self.input_sink() else {
            return;
        };
        if self.vterm.modes().bracketed_paste {
            sink.write(b"\x1b[200~");
            sink.write(text.as_bytes());
            sink.write(b"\x1b[201~");
        } else {
            sink.write(text.as_bytes());
        }
    }

    fn scroll_wheel(&self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let bounds = self.last_bounds.lock().ok().and_then(|bounds| *bounds);
        let Some(bounds) = bounds else { return };
        let (cell_w, cell_h) = self
            .cell_size
            .lock()
            .map(|size| *size)
            .unwrap_or((FALLBACK_CELL_W, FALLBACK_CELL_H));
        let (cols, rows) = self.last_dims.lock().map(|dims| *dims).unwrap_or((2, 2));
        let local = event.position - bounds.origin;
        let col =
            ((f32::from(local.x).max(0.0) / cell_w) as usize).min(cols.saturating_sub(1) as usize);
        let row =
            ((f32::from(local.y).max(0.0) / cell_h) as usize).min(rows.saturating_sub(1) as usize);
        let delta = match event.delta {
            ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / cell_h,
            ScrollDelta::Lines(lines) => lines.y,
        };
        let lines = {
            let Ok(mut accumulated) = self.scroll_accum.lock() else {
                return;
            };
            *accumulated += delta;
            let lines = accumulated.trunc() as i32;
            *accumulated -= lines as f32;
            lines
        };
        if lines == 0 {
            cx.stop_propagation();
            return;
        }

        let modes = self.vterm.modes();
        let count = lines.unsigned_abs().min(100) as usize;
        // tmux mouse on 时，主屏滚轮也发给 PTY，由 tmux 进 copy-mode 滚完整 history。
        // 应用自开 mouse tracking（vim/htop）时同样转发。
        if modes.mouse || !modes.alt_screen {
            let Some(sink) = self.input_sink() else {
                return;
            };
            let report = wheel_report(
                lines > 0,
                col,
                row,
                modes.mouse && modes.sgr_mouse,
                event.modifiers.shift,
                event.modifiers.alt,
                event.modifiers.control,
            );
            for _ in 0..count {
                sink.write(&report);
            }
        } else if modes.alternate_scroll || modes.alt_screen {
            let Some(sink) = self.input_sink() else {
                return;
            };
            let seq: &[u8] = match (lines > 0, modes.app_cursor) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            for _ in 0..count {
                sink.write(seq);
            }
        } else if self.vterm.scroll_display(lines) {
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn scrollbar_geometry(&self) -> Option<(f32, ScrollbarGeometry)> {
        let bounds = self.last_bounds.lock().ok().and_then(|bounds| *bounds)?;
        let visible_lines = self
            .last_dims
            .lock()
            .map(|dims| dims.1 as usize)
            .unwrap_or(1);
        let (history, display_offset) = self.vterm.scroll_metrics();
        scrollbar_geom(
            f32::from(bounds.size.height),
            visible_lines,
            history,
            display_offset,
        )
        .map(|geometry| (f32::from(bounds.origin.y), geometry))
    }

    fn apply_scrollbar_drag(&self, pointer_y: Pixels, grab_offset: f32) -> bool {
        let Some((track_top, geometry)) = self.scrollbar_geometry() else {
            return false;
        };
        let (history, display_offset) = self.vterm.scroll_metrics();
        let desired_offset = scrollbar_drag_offset(
            geometry,
            f32::from(pointer_y) - track_top,
            grab_offset,
            history,
            display_offset,
        );
        let delta = desired_offset as i64 - display_offset as i64;
        self.vterm
            .scroll_display(delta.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }
}

fn read_system_clipboard_text() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return clipboard_command("pbpaste", &[]);
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            if let Some(text) =
                clipboard_command("wl-paste", &["--no-newline", "--type", "text/plain"])
            {
                return Some(text);
            }
        }
        return clipboard_command(
            "xclip",
            &["-selection", "clipboard", "-o", "-t", "text/plain"],
        )
        .or_else(|| clipboard_command("xsel", &["--clipboard", "--output"]));
    }

    #[cfg(target_os = "windows")]
    {
        return clipboard_command("powershell", &["-NoProfile", "-Command", "Get-Clipboard"]);
    }

    #[allow(unreachable_code)]
    None
}

fn clipboard_command(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    (!text.is_empty()).then_some(text)
}

fn wheel_report(
    up: bool,
    col: usize,
    row: usize,
    sgr: bool,
    shift: bool,
    alt: bool,
    control: bool,
) -> Vec<u8> {
    let mut button = if up { 64 } else { 65 };
    if shift {
        button += 4;
    }
    if alt {
        button += 8;
    }
    if control {
        button += 16;
    }
    if sgr {
        format!("\x1b[<{button};{};{}M", col + 1, row + 1).into_bytes()
    } else {
        vec![
            0x1b,
            b'[',
            b'M',
            32 + button,
            (33 + col.min(222)) as u8,
            (33 + row.min(222)) as u8,
        ]
    }
}

#[allow(clippy::too_many_arguments)]
fn mouse_report(
    button: u8,
    col: usize,
    row: usize,
    sgr: bool,
    pressed: bool,
    shift: bool,
    alt: bool,
    control: bool,
    motion: bool,
) -> Vec<u8> {
    let mut code = if motion {
        32 + button
    } else if sgr || pressed {
        button
    } else {
        3
    };
    if shift {
        code += 4;
    }
    if alt {
        code += 8;
    }
    if control {
        code += 16;
    }
    if sgr {
        let suffix = if pressed { 'M' } else { 'm' };
        format!("\x1b[<{code};{};{}{}", col + 1, row + 1, suffix).into_bytes()
    } else {
        vec![
            0x1b,
            b'[',
            b'M',
            32 + code,
            (33 + col.min(222)) as u8,
            (33 + row.min(222)) as u8,
        ]
    }
}

impl Render for TermView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.vterm.render_snapshot();
        let focused = self.focus.is_focused(window);
        let font_family = self.font_family.clone();
        let term_theme = self.theme;
        if let Some(writer) = &self.writer {
            writer.set_focused(focused);
        }

        let vt_resize = self.vterm.clone();
        let writer_resize = self.writer.clone();
        let remote_resize = self.remote_input.clone();
        let input_sink = self.input_sink();
        let input_focus = self.focus.clone();
        let input_marked = Arc::clone(&self.marked_text);
        let dims = Arc::clone(&self.last_dims);
        let pane_bounds = Arc::clone(&self.last_bounds);
        let cell_size = Arc::clone(&self.cell_size);
        let scrollbar = self.scrollbar_geometry().map(|(_, geometry)| geometry);

        div()
            .id("term-pane")
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                let ks = &ev.keystroke;
                let copy_or_paste = (ks.modifiers.control || ks.modifiers.platform)
                    && !ks.modifiers.alt
                    && matches!(ks.key.as_str(), "c" | "v");
                if copy_or_paste && ks.key.as_str() == "c" {
                    if let Some(text) = this
                        .vterm
                        .selection_to_string()
                        .filter(|text| !text.is_empty())
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        Self::write_primary(cx, text);
                        cx.stop_propagation();
                        return;
                    }
                    if ks.modifiers.shift || ks.modifiers.platform {
                        cx.stop_propagation();
                        return;
                    }
                    // Ctrl+C 且没有选区：继续当 SIGINT 发给 PTY。
                }
                if copy_or_paste && ks.key.as_str() == "v" {
                    this.paste_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                if let Some(sink) = this.input_sink() {
                    if let Some(bytes) =
                        crate::terminal_keys::encode_event(ev, this.vterm.modes().app_cursor)
                    {
                        let is_enter = bytes.contains(&b'\r') || bytes.contains(&b'\n');
                        sink.write(&bytes);
                        if is_enter {
                            cx.emit(TermEnterEvent(this.agent.clone()));
                        }
                        cx.stop_propagation();
                    }
                }
            }))
            .on_click(cx.listener(|this, _ev: &gpui::ClickEvent, window, cx| {
                this.focus.focus(window, cx);
                cx.notify();
            }))
            .cursor_text()
            .size_full()
            .overflow_hidden()
            .bg(rgba(term_theme.bg0))
            .child(
                canvas(
                    move |bounds, window, _cx| {
                        let padding_x = px(TERM_PADDING_X);
                        let padding_y = px(TERM_PADDING_Y);
                        let inner = Bounds {
                            origin: point(bounds.origin.x + padding_x, bounds.origin.y + padding_y),
                            size: size(
                                px((f32::from(bounds.size.width) - TERM_PADDING_X * 2.0).max(1.0)),
                                px((f32::from(bounds.size.height) - TERM_PADDING_Y * 2.0).max(1.0)),
                            ),
                        };

                        let family = font_family.clone().into();
                        let base_font = Font {
                            family,
                            features: FontFeatures::disable_ligatures(),
                            fallbacks: Some(FontFallbacks::from_fonts(vec![
                                "DejaVu Sans Mono".into(),
                                "Noto Sans Mono".into(),
                                "Noto Sans CJK SC".into(),
                                "Liberation Mono".into(),
                            ])),
                            weight: FontWeight::NORMAL,
                            style: FontStyle::Normal,
                        };
                        let text_system = window.text_system();
                        let font_size = px(FONT_SIZE);
                        let font_id = text_system.resolve_font(&base_font);
                        let measured_cell = text_system
                            .advance(font_id, font_size, 'm')
                            .map(|advance| advance.width)
                            .unwrap_or(px(FALLBACK_CELL_W));
                        let line_height = font_size * 1.3;
                        let base_shape = text_system.shape_line(
                            "m".into(),
                            font_size,
                            &[TextRun {
                                len: 1,
                                font: base_font.clone(),
                                color: rgba(term_theme.fg0).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            Some(measured_cell),
                        );
                        let base_half = (base_shape.ascent - base_shape.descent) * 0.5;

                        let cols = (((f32::from(inner.size.width) - 0.5)
                            / f32::from(measured_cell))
                        .floor()
                        .max(2.0)) as u16;
                        let rows = (((f32::from(inner.size.height) - 0.5) / f32::from(line_height))
                            .floor()
                            .max(2.0)) as u16;
                        if let Ok(mut last) = pane_bounds.lock() {
                            *last = Some(inner);
                        }
                        if let Ok(mut size) = cell_size.lock() {
                            *size = (f32::from(measured_cell), f32::from(line_height));
                        }
                        if let Ok(mut last) = dims.lock() {
                            if *last != (cols, rows) {
                                *last = (cols, rows);
                                vt_resize.resize(cols, rows);
                                if let Some(writer) = &writer_resize {
                                    let _ = writer.resize(cols, rows);
                                }
                                if let Some(remote) = &remote_resize {
                                    let _ = remote.send(RemoteTermCommand::Resize(cols, rows));
                                }
                                window.request_animation_frame();
                            }
                        }

                        let mut runs = Vec::new();
                        for (row, render_row) in snapshot.rows.iter().enumerate() {
                            for run in &render_row.runs {
                                let fg = if run.style.fg == 0x2a2e38ff {
                                    term_theme.fg0
                                } else if run.style.dim {
                                    dim_u32(run.style.fg)
                                } else {
                                    run.style.fg
                                };
                                let font = Font {
                                    weight: if run.style.bold {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::NORMAL
                                    },
                                    style: if run.style.italic {
                                        FontStyle::Italic
                                    } else {
                                        FontStyle::Normal
                                    },
                                    ..base_font.clone()
                                };
                                let color: Hsla = rgba(fg).into();
                                let shaped = text_system.shape_line(
                                    run.text.clone().into(),
                                    font_size,
                                    &[TextRun {
                                        len: run.text.len(),
                                        font,
                                        color,
                                        background_color: None,
                                        underline: run.style.underline.then_some(UnderlineStyle {
                                            color: Some(color),
                                            thickness: px(1.0),
                                            wavy: false,
                                        }),
                                        strikethrough: None,
                                    }],
                                    Some(measured_cell),
                                );
                                runs.push(PaintRun {
                                    shaped,
                                    start_col: run.start_col,
                                    row,
                                    cells: run.cells,
                                    bg: rgba(if run.style.bg == 0xffffffff {
                                        term_theme.bg0
                                    } else {
                                        run.style.bg
                                    })
                                    .into(),
                                    selected: run.style.selected,
                                });
                            }
                        }

                        let cursor_bounds = |cursor: &muxlane_term::RenderCursor| Bounds {
                            origin: point(
                                inner.origin.x + measured_cell * cursor.col as f32,
                                inner.origin.y + line_height * cursor.row as f32,
                            ),
                            size: size(measured_cell, line_height),
                        };
                        TerminalPaintState {
                            inner,
                            cell_width: measured_cell,
                            line_height,
                            base_half,
                            runs,
                            visible_cursor: snapshot.cursor.as_ref().map(cursor_bounds),
                            logical_cursor: snapshot.logical_cursor.as_ref().map(cursor_bounds),
                        }
                    },
                    move |_bounds, state, window, cx| {
                        if let Some(sink) = &input_sink {
                            window.handle_input(
                                &input_focus,
                                TerminalInputHandler {
                                    sink: sink.clone(),
                                    marked_text: Arc::clone(&input_marked),
                                    cursor_bounds: state.logical_cursor,
                                },
                                cx,
                            );
                        }
                        window.with_content_mask(
                            Some(gpui::ContentMask {
                                bounds: state.inner,
                            }),
                            |window| {
                                for run in &state.runs {
                                    let origin = point(
                                        state.inner.origin.x
                                            + state.cell_width * run.start_col as f32,
                                        state.inner.origin.y + state.line_height * run.row as f32,
                                    );
                                    let bg = if run.selected {
                                        rgba(term_theme.selection()).into()
                                    } else {
                                        run.bg
                                    };
                                    if bg != rgba(term_theme.bg0).into() {
                                        window.paint_quad(fill(
                                            Bounds {
                                                origin,
                                                size: size(
                                                    state.cell_width * run.cells as f32,
                                                    state.line_height,
                                                ),
                                            },
                                            bg,
                                        ));
                                    }
                                    let run_half = (run.shaped.ascent - run.shaped.descent) * 0.5;
                                    let text_origin =
                                        point(origin.x, origin.y + (state.base_half - run_half));
                                    let _ = run.shaped.paint(
                                        text_origin,
                                        state.line_height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    );
                                }
                                if let Some(cursor) = state.visible_cursor {
                                    let mut color = rgba(term_theme.cursor());
                                    if !focused {
                                        color.a = 0.35;
                                    }
                                    window.paint_quad(fill(cursor, color));
                                }
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id("term-selection-layer")
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            this.focus.focus(window, cx);
                            let Some((line, col, right)) = this.grid_point(event.position) else {
                                return;
                            };
                            if event.modifiers.control || event.modifiers.platform {
                                if let Some(url) = this.vterm.url_at(line.max(0) as usize, col) {
                                    #[cfg(target_os = "macos")]
                                    let _ = std::process::Command::new("open").arg(&url).spawn();
                                    #[cfg(not(target_os = "macos"))]
                                    let _ =
                                        std::process::Command::new("xdg-open").arg(&url).spawn();
                                    cx.stop_propagation();
                                    return;
                                }
                            }
                            // Shift：本地划选。其余拖选/点击交给 tmux（mouse on + copy-mode）。
                            if event.modifiers.shift {
                                this.vterm.begin_selection(line, col, right);
                                this.selecting = true;
                                this.forwarding_mouse = false;
                            } else {
                                this.vterm.stop_selection();
                                this.selecting = false;
                                this.forwarding_mouse = true;
                                this.report_mouse(
                                    0,
                                    col,
                                    line,
                                    true,
                                    false,
                                    false,
                                    event.modifiers.alt,
                                    event.modifiers.control,
                                );
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                        this.scroll_wheel(event, cx)
                    }))
                    .on_mouse_move(cx.listener(
                        |this, event: &gpui::MouseMoveEvent, _window, cx| {
                            if this.selecting {
                                if let Some((line, col, right)) = this.grid_point(event.position) {
                                    this.vterm.update_selection(line, col, right);
                                    cx.notify();
                                }
                                cx.stop_propagation();
                                return;
                            }
                            if this.forwarding_mouse {
                                if let Some((line, col, _)) = this.grid_point(event.position) {
                                    this.report_mouse(
                                        0,
                                        col,
                                        line,
                                        true,
                                        true,
                                        event.modifiers.shift,
                                        event.modifiers.alt,
                                        event.modifiers.control,
                                    );
                                }
                                cx.stop_propagation();
                                return;
                            }
                            let Some(grab_offset) = this.scrollbar_drag else {
                                return;
                            };
                            if event.pressed_button != Some(MouseButton::Left) {
                                this.scrollbar_drag = None;
                                cx.notify();
                                return;
                            }
                            if this.apply_scrollbar_drag(event.position.y, grab_offset) {
                                cx.notify();
                            }
                            cx.stop_propagation();
                        },
                    ))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event, _window, cx| {
                            this.finish_mouse(event, cx);
                            if this.scrollbar_drag.take().is_some() {
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, event, _window, cx| {
                            this.finish_mouse(event, cx);
                            if this.scrollbar_drag.take().is_some() {
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }),
                    )
                    .when_some(scrollbar, |layer, geometry| {
                        layer.child(
                            div()
                                .absolute()
                                .right(px(SCROLLBAR_RIGHT))
                                .top(px(TERM_PADDING_Y))
                                .w(px(SCROLLBAR_WIDTH))
                                .h(px(geometry.track_height))
                                .bg(rgba(term_theme.scrollbar_track()))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        |this, event: &gpui::MouseDownEvent, window, cx| {
                                            let Some((track_top, geometry)) =
                                                this.scrollbar_geometry()
                                            else {
                                                return;
                                            };
                                            let pointer_y = f32::from(event.position.y) - track_top;
                                            let on_thumb = pointer_y >= geometry.thumb_top
                                                && pointer_y
                                                    <= geometry.thumb_top + geometry.thumb_height;
                                            let grab_offset = if on_thumb {
                                                pointer_y - geometry.thumb_top
                                            } else {
                                                geometry.thumb_height * 0.5
                                            };
                                            this.scrollbar_drag = Some(grab_offset);
                                            this.focus.focus(window, cx);
                                            if this
                                                .apply_scrollbar_drag(event.position.y, grab_offset)
                                            {
                                                cx.notify();
                                            }
                                            cx.stop_propagation();
                                        },
                                    ),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top(px(geometry.thumb_top))
                                        .w_full()
                                        .h(px(geometry.thumb_height))
                                        .bg(rgba(term_theme.scrollbar_thumb())),
                                ),
                        )
                    }),
            )
    }
}

fn dim_u32(c: u32) -> u32 {
    let r = ((c >> 24) & 0xff) as f32 * 0.66;
    let g = ((c >> 16) & 0xff) as f32 * 0.66;
    let b = ((c >> 8) & 0xff) as f32 * 0.66;
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_clipboard_is_opt_in_and_size_limited() {
        assert!(!osc52_clipboard_allowed(false, "copy me"));
        assert!(osc52_clipboard_allowed(true, "copy me"));
        assert!(!osc52_clipboard_allowed(
            true,
            &"x".repeat(MAX_OSC52_CLIPBOARD_BYTES + 1)
        ));
    }

    #[test]
    fn paste_image_paths_use_format_extensions_and_unique_names() {
        let png = paste_image_path(ImageFormat::Png);
        let jpeg = paste_image_path(ImageFormat::Jpeg);
        let webp = paste_image_path(ImageFormat::Webp);
        assert!(png.to_string_lossy().ends_with(".png"));
        assert!(jpeg.to_string_lossy().ends_with(".jpg"));
        assert!(webp.to_string_lossy().ends_with(".webp"));
        assert_ne!(png, jpeg);
        assert_ne!(png, paste_image_path(ImageFormat::Png));
    }

    #[test]
    fn terminal_preedit_is_not_committed_until_final_text() {
        let marked = std::sync::Mutex::new(None);
        set_terminal_preedit(&marked, "n");
        set_terminal_preedit(&marked, "ni");
        assert_eq!(marked.lock().unwrap().as_deref(), Some("ni"));
        assert_eq!(commit_terminal_text(&marked, "你"), "你".as_bytes());
        assert!(marked.lock().unwrap().is_none());
        set_terminal_preedit(&marked, "好");
        assert_eq!(take_terminal_preedit(&marked).as_deref(), Some("好"));
    }

    #[test]
    fn wheel_reports_use_terminal_mouse_protocol() {
        assert_eq!(
            wheel_report(true, 4, 2, true, false, false, false),
            b"\x1b[<64;5;3M"
        );
        assert_eq!(
            wheel_report(false, 0, 0, false, false, false, false),
            vec![0x1b, b'[', b'M', 97, 33, 33]
        );
    }

    #[test]
    fn scrollbar_geometry_tracks_display_offset() {
        let bottom = scrollbar_geom(200.0, 25, 75, 0).unwrap();
        assert_eq!(bottom.thumb_height, 50.0);
        assert_eq!(bottom.thumb_top, 150.0);

        let middle = scrollbar_geom(200.0, 25, 75, 38).unwrap();
        assert!((middle.thumb_top - 74.0).abs() < 0.01);

        let top = scrollbar_geom(200.0, 25, 75, 75).unwrap();
        assert_eq!(top.thumb_top, 0.0);
        assert!(scrollbar_geom(200.0, 25, 0, 0).is_none());
    }

    #[test]
    fn scrollbar_thumb_has_minimum_height_and_drag_clamps() {
        let geometry = scrollbar_geom(100.0, 10, 990, 0).unwrap();
        assert_eq!(geometry.thumb_height, MIN_SCROLLBAR_THUMB);
        assert_eq!(scrollbar_drag_offset(geometry, -20.0, 12.0, 990, 0), 990);
        assert_eq!(scrollbar_drag_offset(geometry, 120.0, 12.0, 990, 0), 0);
        assert_eq!(scrollbar_drag_offset(geometry, 50.0, 12.0, 990, 0), 495);

        let immovable = scrollbar_geom(20.0, 10, 10, 7).unwrap();
        assert_eq!(scrollbar_drag_offset(immovable, 10.0, 10.0, 10, 7), 7);
    }

    #[test]
    fn mouse_reports_encode_press_release_and_motion() {
        assert_eq!(
            mouse_report(0, 1, 2, true, true, false, false, false, false),
            b"\x1b[<0;2;3M"
        );
        assert_eq!(
            mouse_report(0, 1, 2, true, false, false, false, false, false),
            b"\x1b[<0;2;3m"
        );
        assert_eq!(
            mouse_report(0, 1, 2, false, true, false, false, false, true),
            vec![0x1b, b'[', b'M', 64, 34, 35]
        );
        assert_eq!(
            mouse_report(0, 1, 2, false, true, false, false, false, false),
            vec![0x1b, b'[', b'M', 32, 34, 35]
        );
        assert_eq!(
            mouse_report(0, 1, 2, false, false, false, false, false, false),
            vec![0x1b, b'[', b'M', 35, 34, 35]
        );
    }

    #[test]
    fn repeated_click_reports_include_press_release_pairs() {
        let first = mouse_report(0, 3, 1, false, true, false, false, false, false);
        let release = mouse_report(0, 3, 1, false, false, false, false, false, false);
        let second = mouse_report(0, 3, 1, false, true, false, false, false, false);
        let mut double = first.clone();
        double.extend_from_slice(&release);
        double.extend_from_slice(&second);
        assert_eq!(double.len(), first.len() * 3);
        assert_eq!(&double[..first.len()], first.as_slice());
        assert_eq!(&double[first.len()..first.len() * 2], release.as_slice());
    }
}
