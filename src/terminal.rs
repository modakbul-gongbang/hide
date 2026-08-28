use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{ClipboardType, Config, Term, TermMode, point_to_viewport};
use alacritty_terminal::vte::ansi::{self, Color, CursorShape, NamedColor, Rgb};
use anyhow::{Result, anyhow};

pub const DEFAULT_COLUMNS: usize = 100;
pub const DEFAULT_ROWS: usize = 30;
const SCROLLBACK_LINES: usize = 10_000;
type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalDimensions {
    pub columns: usize,
    pub rows: usize,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Default for TerminalDimensions {
    fn default() -> Self {
        Self {
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            cell_width: 9,
            cell_height: 18,
        }
    }
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

#[derive(Clone)]
struct TerminalEventProxy {
    writer: Option<PtyWriter>,
    events: Arc<Mutex<VecDeque<TerminalEvent>>>,
    generation: Arc<AtomicU64>,
    status: Arc<Mutex<Option<String>>>,
    dimensions: Arc<Mutex<TerminalDimensions>>,
}

impl TerminalEventProxy {
    fn write_pty(&self, text: &str) {
        let result = self
            .writer
            .as_ref()
            .ok_or_else(|| anyhow!("no PTY attached"))
            .and_then(|writer| {
                let mut writer = writer
                    .lock()
                    .map_err(|_| anyhow!("PTY writer lock poisoned"))?;
                writer
                    .write_all(text.as_bytes())
                    .map_err(Into::into)
                    .and_then(|_| writer.flush().map_err(Into::into))
            });
        if let Err(error) = result {
            self.set_failure(format!("Terminal response failed: {error:#}"));
            eprintln!(
                "event=terminal.response.failed stage=pty-write retryable=true error={error:?}"
            );
        }
    }

    fn set_failure(&self, message: String) {
        if let Ok(mut status) = self.status.lock() {
            *status = Some(message);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }

    fn push(&self, event: TerminalEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push_back(event);
        } else {
            self.set_failure("Terminal event queue unavailable".to_owned());
        }
        self.generation.fetch_add(1, Ordering::Release);
    }
}

impl EventListener for TerminalEventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.write_pty(&text),
            Event::ClipboardStore(kind, text) => {
                self.push(TerminalEvent::ClipboardStore(kind, text))
            }
            Event::ClipboardLoad(kind, formatter) => {
                self.push(TerminalEvent::ClipboardLoad(kind, formatter))
            }
            Event::Title(title) => self.push(TerminalEvent::Title(Some(title))),
            Event::ResetTitle => self.push(TerminalEvent::Title(None)),
            Event::TextAreaSizeRequest(formatter) => {
                let d = self
                    .dimensions
                    .lock()
                    .map(|value| *value)
                    .unwrap_or_default();
                self.write_pty(&formatter(WindowSize {
                    num_lines: d.rows.min(u16::MAX as usize) as u16,
                    num_cols: d.columns.min(u16::MAX as usize) as u16,
                    cell_width: d.cell_width,
                    cell_height: d.cell_height,
                }));
            }
            Event::ColorRequest(index, formatter) => {
                self.write_pty(&formatter(default_color_for_index(index)))
            }
            Event::Bell => self.push(TerminalEvent::Bell),
            Event::Exit => self.push(TerminalEvent::Exit),
            Event::ChildExit(status) => self.push(TerminalEvent::ChildExit(status.to_string())),
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                self.generation.fetch_add(1, Ordering::Release);
            }
        }
    }
}

#[derive(Clone)]
pub enum TerminalEvent {
    ClipboardStore(ClipboardType, String),
    ClipboardLoad(ClipboardType, Arc<dyn Fn(&str) -> String + Send + Sync>),
    Title(Option<String>),
    Bell,
    Exit,
    ChildExit(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalModes {
    pub alternate_screen: bool,
    pub bracketed_paste: bool,
    pub mouse_mode: bool,
    pub mouse_motion: bool,
    pub sgr_mouse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalCursor {
    pub row: usize,
    pub column: usize,
    pub shape: CursorShape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCell {
    pub text: String,
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub selected: bool,
    pub spacer: bool,
    pub hyperlink: Option<String>,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            text: " ".to_owned(),
            foreground: [226, 232, 240],
            background: [12, 16, 24],
            bold: false,
            italic: false,
            underlined: false,
            selected: false,
            spacer: false,
            hyperlink: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TerminalView {
    pub columns: usize,
    pub rows: usize,
    pub cells: Vec<TerminalCell>,
    pub cursor: Option<TerminalCursor>,
    pub display_offset: usize,
    pub scrollback_lines: usize,
    pub modes: TerminalModes,
    pub title: Option<String>,
    pub failure: Option<String>,
}

impl TerminalView {
    pub fn cell(&self, row: usize, column: usize) -> Option<&TerminalCell> {
        self.cells
            .get(row.checked_mul(self.columns)?.checked_add(column)?)
    }
    pub fn visible_text(&self) -> String {
        let mut output = String::new();
        for row in 0..self.rows {
            let start = output.len();
            for column in 0..self.columns {
                if let Some(cell) = self.cell(row, column).filter(|cell| !cell.spacer) {
                    output.push_str(&cell.text);
                }
            }
            while output.len() > start && output.ends_with(' ') {
                output.pop();
            }
            if row + 1 < self.rows {
                output.push('\n');
            }
        }
        output.trim_end_matches('\n').to_owned()
    }
}

#[derive(Clone)]
pub struct TerminalModel {
    term: Arc<Mutex<Term<TerminalEventProxy>>>,
    events: Arc<Mutex<VecDeque<TerminalEvent>>>,
    generation: Arc<AtomicU64>,
    status: Arc<Mutex<Option<String>>>,
    title: Arc<Mutex<Option<String>>>,
    dimensions: Arc<Mutex<TerminalDimensions>>,
}

impl TerminalModel {
    pub fn new(writer: Option<PtyWriter>, initial_failure: Option<String>) -> Self {
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let generation = Arc::new(AtomicU64::new(1));
        let status = Arc::new(Mutex::new(initial_failure));
        let title = Arc::new(Mutex::new(None));
        let dimensions = Arc::new(Mutex::new(TerminalDimensions::default()));
        let proxy = TerminalEventProxy {
            writer,
            events: Arc::clone(&events),
            generation: Arc::clone(&generation),
            status: Arc::clone(&status),
            dimensions: Arc::clone(&dimensions),
        };
        let term = Term::new(
            Config {
                scrolling_history: SCROLLBACK_LINES,
                ..Config::default()
            },
            &TerminalDimensions::default(),
            proxy,
        );
        Self {
            term: Arc::new(Mutex::new(term)),
            events,
            generation,
            status,
            title,
            dimensions,
        }
    }

    pub fn feed(&self, processor: &mut ansi::Processor, bytes: &[u8]) {
        match self.term.lock() {
            Ok(mut term) => {
                processor.advance(&mut *term, bytes);
                self.generation.fetch_add(1, Ordering::Release);
            }
            Err(_) => self.set_failure("Terminal state unavailable"),
        }
    }
    pub fn set_failure(&self, message: &str) {
        if let Ok(mut status) = self.status.lock() {
            *status = Some(message.to_owned());
        }
        self.generation.fetch_add(1, Ordering::Release);
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn resize(&self, dimensions: TerminalDimensions) -> Result<()> {
        *self
            .dimensions
            .lock()
            .map_err(|_| anyhow!("terminal dimensions lock poisoned"))? = dimensions;
        self.term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?
            .resize(dimensions);
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }
    pub fn scroll(&self, lines: i32) -> Result<()> {
        self.term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?
            .scroll_display(Scroll::Delta(lines));
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }
    pub fn begin_selection(&self, row: usize, column: usize) -> Result<()> {
        let mut term = self
            .term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?;
        let point = viewport_point(&term, row, column);
        term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }
    pub fn update_selection(&self, row: usize, column: usize) -> Result<()> {
        let mut term = self
            .term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?;
        let point = viewport_point(&term, row, column);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, Side::Right);
        }
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }
    pub fn selected_text(&self) -> Result<Option<String>> {
        Ok(self
            .term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?
            .selection_to_string())
    }
    pub fn encode_paste(&self, text: &str) -> Result<String> {
        let mode = *self
            .term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?
            .mode();
        let normalized = text.replace("\r\n", "\n").replace('\n', "\r");
        Ok(if mode.contains(TermMode::BRACKETED_PASTE) {
            format!("\u{1b}[200~{normalized}\u{1b}[201~")
        } else {
            normalized
        })
    }
    pub fn encode_meta(text: &str) -> String {
        format!("\u{1b}{text}")
    }
    pub fn encode_mouse(
        &self,
        button: u8,
        pressed: bool,
        row: usize,
        column: usize,
    ) -> Result<Option<String>> {
        let mode = *self
            .term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?
            .mode();
        if !mode.intersects(TermMode::MOUSE_MODE) {
            return Ok(None);
        }
        let code = if pressed { button } else { 3 };
        let column = column.saturating_add(1).min(9999);
        let row = row.saturating_add(1).min(9999);
        if mode.contains(TermMode::SGR_MOUSE) {
            Ok(Some(format!(
                "\u{1b}[<{code};{column};{row}{}",
                if pressed { 'M' } else { 'm' }
            )))
        } else {
            let cb = char::from_u32((code as u32 + 32).min(255)).unwrap_or(' ');
            let cx = char::from_u32((column as u32 + 32).min(255)).unwrap_or(' ');
            let cy = char::from_u32((row as u32 + 32).min(255)).unwrap_or(' ');
            Ok(Some(format!("\u{1b}[M{cb}{cx}{cy}")))
        }
    }
    pub fn drain_events(&self) -> Vec<TerminalEvent> {
        let Ok(mut events) = self.events.lock() else {
            self.set_failure("Terminal event queue unavailable");
            return Vec::new();
        };
        let drained = events.drain(..).collect::<Vec<_>>();
        drop(events);
        for event in &drained {
            match event {
                TerminalEvent::Title(title) => {
                    if let Ok(mut current) = self.title.lock() {
                        current.clone_from(title);
                    }
                }
                TerminalEvent::Exit => self.set_failure("Terminal requested exit"),
                TerminalEvent::ChildExit(status) => {
                    self.set_failure(&format!("Terminal process exited: {status}"))
                }
                TerminalEvent::Bell
                | TerminalEvent::ClipboardStore(..)
                | TerminalEvent::ClipboardLoad(..) => {}
            }
        }
        drained
    }

    pub fn snapshot(&self) -> Result<TerminalView> {
        let term = self
            .term
            .lock()
            .map_err(|_| anyhow!("terminal state lock poisoned"))?;
        let content = term.renderable_content();
        let columns = term.columns();
        let rows = term.screen_lines();
        let scrollback_lines = term.total_lines().saturating_sub(rows);
        let display_offset = content.display_offset;
        let modes = TerminalModes {
            alternate_screen: content.mode.contains(TermMode::ALT_SCREEN),
            bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
            mouse_mode: content.mode.intersects(TermMode::MOUSE_MODE),
            mouse_motion: content
                .mode
                .intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
            sgr_mouse: content.mode.contains(TermMode::SGR_MOUSE),
        };
        let cursor = point_to_viewport(display_offset, content.cursor.point).and_then(|point| {
            (point.line < rows && content.cursor.shape != CursorShape::Hidden).then_some(
                TerminalCursor {
                    row: point.line,
                    column: point.column.0,
                    shape: content.cursor.shape,
                },
            )
        });
        let selection = content.selection;
        let colors = *content.colors;
        let mut cells = vec![TerminalCell::default(); columns * rows];
        for indexed in content.display_iter {
            let Some(point) = point_to_viewport(display_offset, indexed.point) else {
                continue;
            };
            if point.line >= rows || point.column.0 >= columns {
                continue;
            }
            let flags = indexed.cell.flags;
            let mut foreground = resolve_color(indexed.cell.fg, &colors, true);
            let mut background = resolve_color(indexed.cell.bg, &colors, false);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            let spacer =
                flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let mut text = if spacer || flags.contains(Flags::HIDDEN) {
                " ".to_owned()
            } else {
                indexed.cell.c.to_string()
            };
            if let Some(extra) = indexed.cell.zerowidth() {
                text.extend(extra);
            }
            cells[point.line * columns + point.column.0] = TerminalCell {
                text,
                foreground,
                background,
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underlined: flags.intersects(Flags::ALL_UNDERLINES),
                selected: selection.is_some_and(|range| range.contains(indexed.point)),
                spacer,
                hyperlink: indexed.cell.hyperlink().map(|link| link.uri().to_owned()),
            };
        }
        Ok(TerminalView {
            columns,
            rows,
            cells,
            cursor,
            display_offset,
            scrollback_lines,
            modes,
            title: self
                .title
                .lock()
                .ok()
                .and_then(|title| title.clone())
                .map(|title| safe_terminal_title(&title)),
            failure: self.status.lock().ok().and_then(|status| status.clone()),
        })
    }
}

fn viewport_point<T>(term: &Term<T>, row: usize, column: usize) -> Point {
    Point::new(
        Line(
            row.min(term.screen_lines().saturating_sub(1)) as i32
                - term.grid().display_offset() as i32,
        ),
        Column(column.min(term.columns().saturating_sub(1))),
    )
}
fn resolve_color(
    color: Color,
    colors: &alacritty_terminal::term::color::Colors,
    foreground: bool,
) -> [u8; 3] {
    match color {
        Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
        Color::Indexed(index) => rgb_array(
            colors[index as usize].unwrap_or_else(|| default_color_for_index(index as usize)),
        ),
        Color::Named(name) => {
            rgb_array(colors[name].unwrap_or_else(|| named_color(name, foreground)))
        }
    }
}
fn rgb_array(rgb: Rgb) -> [u8; 3] {
    [rgb.r, rgb.g, rgb.b]
}
fn default_color_for_index(index: usize) -> Rgb {
    if index < 16 {
        return named_color(
            match index {
                0 => NamedColor::Black,
                1 => NamedColor::Red,
                2 => NamedColor::Green,
                3 => NamedColor::Yellow,
                4 => NamedColor::Blue,
                5 => NamedColor::Magenta,
                6 => NamedColor::Cyan,
                7 => NamedColor::White,
                8 => NamedColor::BrightBlack,
                9 => NamedColor::BrightRed,
                10 => NamedColor::BrightGreen,
                11 => NamedColor::BrightYellow,
                12 => NamedColor::BrightBlue,
                13 => NamedColor::BrightMagenta,
                14 => NamedColor::BrightCyan,
                _ => NamedColor::BrightWhite,
            },
            true,
        );
    }
    if index < 232 {
        let value = index - 16;
        let component = |part: usize| if part == 0 { 0 } else { 55 + part as u8 * 40 };
        return Rgb {
            r: component(value / 36),
            g: component((value / 6) % 6),
            b: component(value % 6),
        };
    }
    if index < 256 {
        let value = 8 + (index - 232) as u8 * 10;
        return Rgb {
            r: value,
            g: value,
            b: value,
        };
    }
    if index == NamedColor::Background as usize {
        Rgb {
            r: 12,
            g: 16,
            b: 24,
        }
    } else {
        Rgb {
            r: 226,
            g: 232,
            b: 240,
        }
    }
}
fn named_color(name: NamedColor, foreground: bool) -> Rgb {
    let palette = [
        [30, 34, 42],
        [239, 68, 68],
        [74, 222, 128],
        [250, 204, 21],
        [96, 165, 250],
        [216, 180, 254],
        [34, 211, 238],
        [226, 232, 240],
        [100, 116, 139],
        [248, 113, 113],
        [134, 239, 172],
        [253, 224, 71],
        [147, 197, 253],
        [233, 213, 255],
        [103, 232, 249],
        [248, 250, 252],
    ];
    let index = name as usize;
    if index < palette.len() {
        let [r, g, b] = palette[index];
        Rgb { r, g, b }
    } else if matches!(name, NamedColor::Background) || !foreground {
        Rgb {
            r: 12,
            g: 16,
            b: 24,
        }
    } else {
        Rgb {
            r: 226,
            g: 232,
            b: 240,
        }
    }
}

fn safe_terminal_title(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty()
        || trimmed.contains('@')
        || trimmed.contains("/Users/")
        || trimmed.contains("/private/")
    {
        "Terminal".to_owned()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parsed(bytes: &[u8]) -> TerminalModel {
        let terminal = TerminalModel::new(None, None);
        terminal.feed(&mut ansi::Processor::new(), bytes);
        terminal
    }
    #[test]
    fn deterministic_fixture_uses_grid_without_leaking_osc_or_csi() {
        let terminal = parsed(b"plain \x1b[31mred\x1b[0m \x1b]8;;https://example.com\x07link\x1b]8;;\x07 \xed\x95\x9c\xea\xb8\x80");
        let view = terminal.snapshot().unwrap();
        let text = view.visible_text();
        assert!(
            text.contains("plain red link 한글"),
            "visible grid was {text:?}"
        );
        assert!(!text.contains("]8;;"));
        assert!(
            view.cells
                .iter()
                .any(|cell| cell.foreground == [239, 68, 68])
        );
        assert!(
            view.cells
                .iter()
                .any(|cell| cell.hyperlink.as_deref() == Some("https://example.com"))
        );
    }
    #[test]
    fn alternate_screen_cursor_modes_and_primary_screen_restore() {
        let terminal = parsed(b"primary\x1b[?1049halt\x1b[2J\x1b[5;7Halt\x1b[?25l");
        let alternate = terminal.snapshot().unwrap();
        assert!(alternate.modes.alternate_screen);
        assert!(alternate.cursor.is_none());
        assert!(alternate.visible_text().contains("alt"));
        terminal.feed(&mut ansi::Processor::new(), b"\x1b[?25h\x1b[?1049l");
        let primary = terminal.snapshot().unwrap();
        assert!(!primary.modes.alternate_screen);
        assert!(primary.cursor.is_some());
        assert!(primary.visible_text().contains("primary"));
    }
    #[test]
    fn scrollback_selection_resize_and_input_modes_are_owned_by_term() {
        let terminal = TerminalModel::new(None, None);
        let mut parser = ansi::Processor::new();
        for line in 0..40 {
            terminal.feed(&mut parser, format!("line-{line:02}\r\n").as_bytes());
        }
        terminal.scroll(5).unwrap();
        let scrolled = terminal.snapshot().unwrap();
        assert!(scrolled.scrollback_lines >= 5);
        assert_eq!(scrolled.display_offset, 5);
        terminal.begin_selection(0, 0).unwrap();
        terminal.update_selection(0, 6).unwrap();
        assert!(
            terminal
                .selected_text()
                .unwrap()
                .is_some_and(|text| text.starts_with("line-"))
        );
        terminal
            .resize(TerminalDimensions {
                columns: 80,
                rows: 24,
                cell_width: 9,
                cell_height: 18,
            })
            .unwrap();
        assert_eq!(
            (
                terminal.snapshot().unwrap().columns,
                terminal.snapshot().unwrap().rows
            ),
            (80, 24)
        );
        terminal.feed(&mut parser, b"\x1b[?2004h\x1b[?1000h\x1b[?1006h");
        assert_eq!(
            terminal.encode_paste("a\nb").unwrap(),
            "\x1b[200~a\rb\x1b[201~"
        );
        assert_eq!(TerminalModel::encode_meta("f"), "\x1bf");
        assert_eq!(
            terminal.encode_mouse(0, true, 2, 3).unwrap().as_deref(),
            Some("\x1b[<0;4;3M")
        );
    }
    #[test]
    fn child_failure_is_retained_as_a_pane_visible_state() {
        let terminal = TerminalModel::new(None, None);
        terminal.set_failure("Pane attach conflict: already attached");
        assert_eq!(
            terminal.snapshot().unwrap().failure.as_deref(),
            Some("Pane attach conflict: already attached")
        );
    }
    #[test]
    fn account_and_hostname_are_not_projected_into_product_chrome() {
        assert_eq!(safe_terminal_title("person@host:/private/tmp"), "Terminal");
        assert_eq!(safe_terminal_title("vim"), "vim");
    }
    #[test]
    fn hostile_zerowidth_scrollback_is_bounded_by_the_pinned_upstream_cell_limit() {
        let terminal = TerminalModel::new(None, None);
        let mut parser = ansi::Processor::new();
        let hostile = format!("x{}\r\n", "\u{0301}".repeat(64));
        for _ in 0..10_100 {
            terminal.feed(&mut parser, hostile.as_bytes());
        }
        let term = terminal.term.lock().unwrap();
        assert!(term.total_lines() <= SCROLLBACK_LINES + DEFAULT_ROWS);
        let mut max_per_cell = 0;
        let mut retained = 0;
        for line in term.topmost_line().0..=term.bottommost_line().0 {
            for column in 0..term.columns() {
                let count = term.grid()[Line(line)][Column(column)]
                    .zerowidth()
                    .map_or(0, <[char]>::len);
                max_per_cell = max_per_cell.max(count);
                retained += count;
            }
        }
        assert_eq!(max_per_cell, 9);
        assert!(retained <= term.total_lines() * term.columns() * 9);
    }
}
