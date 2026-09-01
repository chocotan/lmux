//! 终端视图：真彩色 cell renderer + 光标 + 低延迟 PTY 输入。
//! 架构直接遵循 muxel：TermView 在 GPUI task 内 drain PTY 输出，chunk 到达即 process + notify。
use gpui::{
    canvas, div, fill, point, prelude::*, px, rgba, size, App, Bounds, Context, FocusHandle,
    Focusable, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, InputHandler,
    MouseButton, ParentElement, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, ShapedLine,
    Styled, Task, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use lmux_core::model::AgentId;
use lmux_term::{PtySession, VTerm};
use std::sync::Arc;

const FONT_SIZE: f32 = 13.0;
const TERM_PADDING: f32 = 4.0;
const FALLBACK_CELL_W: f32 = 8.2;
const FALLBACK_CELL_H: f32 = 17.0;
const SCROLLBAR_WIDTH: f32 = 10.0;
const SCROLLBAR_RIGHT: f32 = 2.0;
const MIN_SCROLLBAR_THUMB: f32 = 24.0;

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

pub struct TermView {
    pub vterm: VTerm,
    pub focus: FocusHandle,
    writer: Option<Arc<PtySession>>,
    remote_input: Option<tokio::sync::mpsc::UnboundedSender<RemoteTermCommand>>,
    last_dims: Arc<std::sync::Mutex<(u16, u16)>>,
    last_bounds: Arc<std::sync::Mutex<Option<Bounds<Pixels>>>>,
    cell_size: Arc<std::sync::Mutex<(f32, f32)>>,
    scroll_accum: Arc<std::sync::Mutex<f32>>,
    scrollbar_drag: Option<f32>,
    marked_text: Arc<std::sync::Mutex<Option<String>>>,
    _drain: Task<()>,
}

impl Focusable for TermView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TermView {
    /// 本地 PTY：订阅 replay+增量，输出到达即 `cx.notify()`，无 1 秒轮询。
    pub fn new_local(_agent: AgentId, session: Arc<PtySession>, cx: &mut Context<Self>) -> Self {
        let vterm = VTerm::new(120, 32);
        let (replay, mut rx) = session.subscribe();
        vterm.feed(&replay);
        let vterm_for_task = vterm.clone();
        let session_for_task = Arc::clone(&session);
        let drain = cx.spawn(async move |view, cx| {
            loop {
                let first = match rx.recv().await {
                    Ok(b) => b,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                // 参考 muxel paint scheduler：交互 8ms / 聚焦 stream 33ms / 后台 100ms。
                let delay = if session_for_task.interaction_recent() {
                    std::time::Duration::from_millis(8)
                } else if session_for_task.is_focused() {
                    std::time::Duration::from_millis(33)
                } else {
                    std::time::Duration::from_millis(100)
                };
                cx.background_executor().timer(delay).await;
                let mut output = first.to_vec();
                while output.len() < 256 * 1024 {
                    match rx.try_recv() {
                        Ok(b) => output.extend_from_slice(&b),
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
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
            vterm,
            focus: cx.focus_handle(),
            writer: Some(session),
            remote_input: None,
            last_dims: Arc::new(std::sync::Mutex::new((120, 32))),
            last_bounds: Arc::new(std::sync::Mutex::new(None)),
            cell_size: Arc::new(std::sync::Mutex::new((FALLBACK_CELL_W, FALLBACK_CELL_H))),
            scroll_accum: Arc::new(std::sync::Mutex::new(0.0)),
            scrollbar_drag: None,
            marked_text: Arc::new(std::sync::Mutex::new(None)),
            _drain: drain,
        }
    }

    /// 远程镜像：P2 客户端把 TermData 喂入同一个 VTerm；输入保持 None（v1 只读）。
    pub fn new_remote(
        _agent: AgentId,
        vterm: VTerm,
        remote_input: tokio::sync::mpsc::UnboundedSender<RemoteTermCommand>,
        cx: &mut Context<Self>,
    ) -> Self {
        let idle = cx.spawn(async move |_view, _cx| {
            std::future::pending::<()>().await;
        });
        Self {
            vterm,
            focus: cx.focus_handle(),
            writer: None,
            remote_input: Some(remote_input),
            last_dims: Arc::new(std::sync::Mutex::new((120, 32))),
            last_bounds: Arc::new(std::sync::Mutex::new(None)),
            cell_size: Arc::new(std::sync::Mutex::new((FALLBACK_CELL_W, FALLBACK_CELL_H))),
            scroll_accum: Arc::new(std::sync::Mutex::new(0.0)),
            scrollbar_drag: None,
            marked_text: Arc::new(std::sync::Mutex::new(None)),
            _drain: idle,
        }
    }

    fn input_sink(&self) -> Option<InputSink> {
        self.writer
            .as_ref()
            .map(|session| InputSink::Local(Arc::clone(session)))
            .or_else(|| self.remote_input.clone().map(InputSink::Remote))
    }

    fn keystroke_bytes(ks: &gpui::Keystroke) -> Vec<u8> {
        let key = ks.key.as_str();
        if ks.modifiers.control {
            // 全局命令面板快捷键，交给 root action，不发给 PTY。
            if key == "k" || key == "w" || (ks.modifiers.shift && key == "t") {
                return vec![];
            }
            if key.len() == 1 {
                let c = key.as_bytes()[0].to_ascii_lowercase();
                if c.is_ascii_lowercase() {
                    return vec![c - b'a' + 1];
                }
            }
        }
        // 可打印文本（含 IME）由 InputHandler 提交，避免与 KeyDown 重复写入。
        let printable = ks.key_char.as_ref().is_some_and(|text| !text.is_empty());
        if printable && !ks.modifiers.control && !ks.modifiers.alt {
            return vec![];
        }
        let mut out = match key {
            "enter" => b"\r".to_vec(),
            "tab" => b"\t".to_vec(),
            "backspace" => vec![0x7f],
            "escape" => b"\x1b".to_vec(),
            "up" => b"\x1b[A".to_vec(),
            "down" => b"\x1b[B".to_vec(),
            "right" => b"\x1b[C".to_vec(),
            "left" => b"\x1b[D".to_vec(),
            "home" => b"\x1b[H".to_vec(),
            "end" => b"\x1b[F".to_vec(),
            "pageup" => b"\x1b[5~".to_vec(),
            "pagedown" => b"\x1b[6~".to_vec(),
            "delete" => b"\x1b[3~".to_vec(),
            "space" => b" ".to_vec(),
            _ => ks
                .key_char
                .as_ref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&ks.key)
                .as_bytes()
                .to_vec(),
        };
        if ks.modifiers.alt && !out.is_empty() {
            out.insert(0, 0x1b);
        }
        out
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
            return;
        }

        let modes = self.vterm.modes();
        let count = lines.unsigned_abs().min(100) as usize;
        if modes.mouse {
            let Some(sink) = self.input_sink() else {
                return;
            };
            let report = wheel_report(
                lines > 0,
                col,
                row,
                modes.sgr_mouse,
                event.modifiers.shift,
                event.modifiers.alt,
                event.modifiers.control,
            );
            for _ in 0..count {
                sink.write(&report);
            }
        } else if modes.alt_screen && modes.alternate_scroll {
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

impl Render for TermView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.vterm.render_snapshot();
        let focused = self.focus.is_focused(window);
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
                if let Some(sink) = this.input_sink() {
                    let bytes = Self::keystroke_bytes(&ev.keystroke);
                    if !bytes.is_empty() {
                        sink.write(&bytes);
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
            .bg(rgba(0xffffffff))
            .child(
                canvas(
                    move |bounds, window, _cx| {
                        let padding = px(TERM_PADDING);
                        let inner = Bounds {
                            origin: point(bounds.origin.x + padding, bounds.origin.y + padding),
                            size: size(
                                px((f32::from(bounds.size.width) - TERM_PADDING * 2.0).max(1.0)),
                                px((f32::from(bounds.size.height) - TERM_PADDING * 2.0).max(1.0)),
                            ),
                        };

                        let family = "JetBrains Mono".into();
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
                                color: rgba(0x2a2e38ff).into(),
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
                                let fg = if run.style.dim {
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
                                    bg: rgba(run.style.bg).into(),
                                });
                            }
                        }

                        let cursor_bounds = |cursor: &lmux_term::RenderCursor| Bounds {
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
                                    if run.bg != rgba(0xffffffff).into() {
                                        window.paint_quad(fill(
                                            Bounds {
                                                origin,
                                                size: size(
                                                    state.cell_width * run.cells as f32,
                                                    state.line_height,
                                                ),
                                            },
                                            run.bg,
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
                                    let mut color = rgba(0x3d6cd8c0);
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
                    .absolute()
                    .size_full()
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                        this.scroll_wheel(event, cx)
                    }))
                    .on_mouse_move(cx.listener(
                        |this, event: &gpui::MouseMoveEvent, _window, cx| {
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
                        cx.listener(|this, _event, _window, cx| {
                            if this.scrollbar_drag.take().is_some() {
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| {
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
                                .top(px(TERM_PADDING))
                                .w(px(SCROLLBAR_WIDTH))
                                .h(px(geometry.track_height))
                                .rounded_sm()
                                .bg(rgba(0x2a2e381a))
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
                                        .rounded_sm()
                                        .bg(rgba(0x6f7480aa)),
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
    fn ks(key: &str, ch: Option<&str>, ctrl: bool, alt: bool) -> gpui::Keystroke {
        let modifiers = gpui::Modifiers {
            control: ctrl,
            alt,
            ..Default::default()
        };
        gpui::Keystroke {
            modifiers,
            key: key.into(),
            key_char: ch.map(Into::into),
        }
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
    fn terminal_key_mapping() {
        assert_eq!(
            TermView::keystroke_bytes(&ks("enter", None, false, false)),
            b"\r"
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("backspace", None, false, false)),
            vec![0x7f]
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("up", None, false, false)),
            b"\x1b[A"
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("c", Some("c"), true, false)),
            vec![0x03]
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("k", Some("k"), true, false)),
            Vec::<u8>::new()
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("w", Some("w"), true, false)),
            Vec::<u8>::new()
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("a", Some("a"), false, false)),
            Vec::<u8>::new()
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("中", Some("中"), false, false)),
            Vec::<u8>::new()
        );
        assert_eq!(
            TermView::keystroke_bytes(&ks("s", Some("ß"), false, true)),
            [vec![0x1b], "ß".as_bytes().to_vec()].concat()
        );
    }
}
