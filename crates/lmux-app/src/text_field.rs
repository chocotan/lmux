use gpui::{
    canvas, div, prelude::*, px, rgba, Bounds, Context, ElementInputHandler, EntityInputHandler,
    FocusHandle, Focusable, Pixels, Point, Render, SharedString, UTF16Selection, Window,
};
use std::ops::Range;

fn utf16_to_byte(content: &str, target: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in content.char_indices() {
        if utf16 >= target {
            return byte;
        }
        utf16 += ch.len_utf16();
        if utf16 > target {
            return byte;
        }
    }
    content.len()
}

fn byte_to_utf16(content: &str, target: usize) -> usize {
    let mut boundary = target.min(content.len());
    while !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    content[..boundary].encode_utf16().count()
}

fn range_from_utf16_in(content: &str, range: Range<usize>) -> Range<usize> {
    utf16_to_byte(content, range.start)..utf16_to_byte(content, range.end)
}

fn marked_selection(base: usize, new_text: &str, selected_utf16: Range<usize>) -> Range<usize> {
    let selected = range_from_utf16_in(new_text, selected_utf16);
    base + selected.start..base + selected.end
}

pub struct TextField {
    focus: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    secure: bool,
}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: String::new(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            marked_range: None,
            secure: false,
        }
    }

    pub fn new_secure(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let mut field = Self::new(placeholder, cx);
        field.secure = true;
        field
    }

    pub fn text(&self) -> String {
        self.content.clone()
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.content.clear();
        self.selected_range = 0..0;
        self.marked_range = None;
        cx.notify();
    }

    fn utf16_to_byte(&self, target: usize) -> usize {
        utf16_to_byte(&self.content, target)
    }

    fn byte_to_utf16(&self, target: usize) -> usize {
        byte_to_utf16(&self.content, target)
    }

    fn range_from_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.utf16_to_byte(range.start)..self.utf16_to_byte(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.byte_to_utf16(range.start)..self.byte_to_utf16(range.end)
    }

    fn backspace(&mut self, cx: &mut Context<Self>) {
        let range = if !self.selected_range.is_empty() {
            self.selected_range.clone()
        } else if self.selected_range.start > 0 {
            let end = self.selected_range.start;
            let start = self.content[..end]
                .char_indices()
                .next_back()
                .map(|(byte, _)| byte)
                .unwrap_or(0);
            start..end
        } else {
            return;
        };
        self.content.replace_range(range.clone(), "");
        self.selected_range = range.start..range.start;
        self.marked_range = None;
        cx.notify();
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(range_utf16);
        *adjusted_range = Some(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.content.replace_range(range.clone(), text);
        let cursor = range.start + text.len();
        self.selected_range = cursor..cursor;
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.content.replace_range(range.clone(), new_text);
        let marked = range.start..range.start + new_text.len();
        self.marked_range = (!new_text.is_empty()).then_some(marked.clone());
        self.selected_range = new_selected_range_utf16
            .map(|selected| marked_selection(range.start, new_text, selected))
            .unwrap_or(marked.end..marked.end);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.byte_to_utf16(self.content.len()))
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let focus = self.focus.clone();
        let focused = focus.is_focused(window);
        let text: SharedString = if self.content.is_empty() {
            self.placeholder.clone()
        } else if self.secure {
            "•".repeat(self.content.chars().count()).into()
        } else {
            self.content.clone().into()
        };
        div()
            .id("text-field")
            .relative()
            .track_focus(&focus)
            .flex()
            .items_center()
            .w_full()
            .min_h(px(34.))
            .px_3()
            .border_1()
            .border_color(rgba(if focused { 0x3d6cd8ff } else { 0xd8d6ceff }))
            .bg(rgba(0xfafaf7ff))
            .text_size(px(12.))
            .text_color(rgba(if self.content.is_empty() {
                0x8b90a0ff
            } else {
                0x252525ff
            }))
            .on_click(cx.listener(|this, _event, window, cx| {
                this.focus.focus(window, cx);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_key_down(
                cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                    if event.keystroke.key == "backspace" {
                        this.backspace(cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(text)
            .when(focused, |field| {
                field.child(div().ml(px(1.)).w(px(1.)).h(px(16.)).bg(rgba(0x3d6cd8ff)))
            })
            .child(
                canvas(
                    |_bounds, _window, _cx| {},
                    move |bounds, _state, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_conversion_handles_chinese_and_surrogate_pairs() {
        let content = "A中😀B";
        assert_eq!(utf16_to_byte(content, 2), "A中".len());
        assert_eq!(byte_to_utf16(content, "A中😀".len()), 4);
        assert_eq!(byte_to_utf16(content, 2), 1);
    }

    #[test]
    fn marked_selection_is_relative_to_new_text() {
        assert_eq!(marked_selection(3, "中文", 2..2), 9..9);
        assert_eq!(marked_selection(3, "😀x", 2..3), 7..8);
    }
}
