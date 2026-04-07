#![warn(clippy::pedantic)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(clippy::allow_attributes)]
#![warn(clippy::allow_attributes_without_reason)]

//! Thin Zed-specific adapter helpers built on top of `libghostty-vt`.

use std::{cell::RefCell, rc::Rc};

use anyhow::{Context as _, Result as AnyResult};
use libghostty_vt::{
    RenderState as WrapperRenderState, Terminal as WrapperTerminal,
    error::Result,
    focus, key, mouse,
    render::{CellIterator, Dirty, RowIteration, RowIterator, Snapshot},
    screen::{
        Cell as WrapperCell, CellWide as WrapperCellWide, GridRef, Row as WrapperRow,
        RowSemanticPrompt as WrapperRowSemanticPrompt,
    },
    style::{RgbColor as WrapperRgbColor, Style as WrapperStyle, StyleColor as WrapperStyleColor},
    terminal::{
        ColorScheme, ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType,
        PrimaryDeviceAttributes, SecondaryDeviceAttributes, SizeReportSize,
        TertiaryDeviceAttributes,
    },
};

/// Runtime effects collected from terminal callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEffect {
    /// Bell sequence was received.
    Bell,
    /// Terminal title changed, including being cleared.
    TitleChanged(Option<String>),
}

/// Options for installing Zed callback behavior on a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallOptions {
    /// XTVERSION response string to report.
    pub xtversion: &'static str,
}

/// Dirty state for a render snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDirty {
    /// The frame is clean.
    Clean,
    /// Some rows have changed.
    Partial,
    /// The whole frame should be treated as dirty.
    Full,
}

/// Width classification for a rendered cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellWide {
    /// A normal single-column cell.
    Narrow,
    /// The leading half of a wide character.
    Wide,
    /// The trailing spacer half of a wide character.
    SpacerTail,
    /// A leading spacer head marker.
    SpacerHead,
}

/// RGB color in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor {
    /// Red channel.
    pub red: u8,
    /// Green channel.
    pub green: u8,
    /// Blue channel.
    pub blue: u8,
}

/// Color reference used by Zed compatibility cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleColor {
    /// No explicit color override.
    None,
    /// Palette-indexed color.
    Palette(u8),
    /// Direct RGB color.
    Rgb(RgbColor),
}

/// Cell styling in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    /// Foreground color.
    pub foreground: StyleColor,
    /// Background color.
    pub background: StyleColor,
    /// Underline color.
    pub underline_color: StyleColor,
    /// Bold flag.
    pub bold: bool,
    /// Italic flag.
    pub italic: bool,
    /// Faint flag.
    pub faint: bool,
    /// Blink flag.
    pub blink: bool,
    /// Inverse flag.
    pub inverse: bool,
    /// Invisible flag.
    pub invisible: bool,
    /// Strikethrough flag.
    pub strikethrough: bool,
    /// Overline flag.
    pub overline: bool,
    /// Raw underline style value.
    pub underline: i32,
}

/// Shared screen identity used by compatibility summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Primary terminal screen.
    Primary,
    /// Alternate screen buffer.
    Alternate,
}

/// Cursor-related terminal state in the compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// Cursor column position.
    pub column: u16,
    /// Cursor row position.
    pub row: u16,
    /// Whether the next printable character wraps first.
    pub pending_wrap: bool,
    /// Whether the cursor is visible.
    pub visible: bool,
    /// Active screen buffer.
    pub active_screen: Screen,
    /// Active Kitty keyboard flags as a raw bitset.
    pub kitty_flags: u8,
}

/// Scrollbar state in the compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarState {
    /// Total scrollable rows.
    pub total: u64,
    /// Current scroll offset.
    pub offset: u64,
    /// Visible scrollbar length.
    pub len: u64,
}

/// Prompt classification for a grid row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowSemanticPrompt {
    /// No prompt cells in the row.
    None,
    /// Prompt cells exist in the row.
    Prompt,
    /// Prompt continuation cells exist in the row.
    PromptContinuation,
}

/// Grid cell data in the compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    /// Primary codepoint for the cell, if any.
    pub codepoint: Option<u32>,
    /// Width classification.
    pub wide: CellWide,
    /// Cell style metadata.
    pub style: CellStyle,
}

/// Grid row data in the compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRow {
    /// Whether the row is soft-wrapped.
    pub wrapped: bool,
    /// Whether the row continues a wrapped line.
    pub wrap_continuation: bool,
    /// Whether any cells contain grapheme clusters.
    pub has_grapheme_cluster: bool,
    /// Whether any cells are styled.
    pub is_styled: bool,
    /// Whether any cells have hyperlinks.
    pub has_hyperlink: bool,
    /// Prompt metadata for the row.
    pub semantic_prompt: RowSemanticPrompt,
    /// Whether the row contains Kitty placeholder cells.
    pub has_kitty_virtual_placeholder: bool,
    /// Whether the row is dirty.
    pub is_dirty: bool,
}

/// Rendered cell data in the Zed compatibility shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCell {
    /// Temporary raw cell handle preserved for compatibility.
    pub raw: u64,
    /// Primary codepoint for the cell, if any.
    pub codepoint: Option<u32>,
    /// Width classification for the cell.
    pub wide: CellWide,
    /// Resolved style metadata.
    pub style: CellStyle,
    /// Resolved background color, if available.
    pub resolved_background: Option<RgbColor>,
    /// Resolved foreground color, if available.
    pub resolved_foreground: Option<RgbColor>,
    /// Whether the cell has a hyperlink.
    pub has_hyperlink: bool,
    /// Full grapheme cluster codepoints for the cell.
    pub graphemes: Vec<u32>,
}

/// Rendered row data in the Zed compatibility shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRow {
    /// Whether the row was dirty before snapshot adaptation cleared it.
    pub dirty: bool,
    /// Whether the row is soft-wrapped.
    pub wrapped: bool,
    /// Whether the row continues a wrapped line.
    pub wrap_continuation: bool,
    /// Whether the row contains a Kitty placeholder.
    pub has_kitty_virtual_placeholder: bool,
    /// Cells in the row.
    pub cells: Vec<RenderCell>,
}

/// Cursor information for a render snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCursor {
    /// Whether the cursor is visible.
    pub visible: bool,
    /// Whether the cursor is blinking.
    pub blinking: bool,
    /// Whether the cursor lies within the viewport.
    pub in_viewport: bool,
    /// Viewport column of the cursor, if visible in the viewport.
    pub viewport_column: Option<u16>,
    /// Viewport row of the cursor, if visible in the viewport.
    pub viewport_row: Option<u16>,
}

/// Color palette data for a render snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderColors {
    /// Default background color.
    pub background: RgbColor,
    /// Default foreground color.
    pub foreground: RgbColor,
    /// Cursor color override, if present.
    pub cursor: Option<RgbColor>,
    /// Resolved palette colors.
    pub palette: Vec<RgbColor>,
}

/// Eager render snapshot in the Zed compatibility shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSnapshot {
    /// Number of columns in the viewport.
    pub columns: u16,
    /// Number of rows in the viewport.
    pub rows: u16,
    /// Dirty state of the snapshot before it was cleared.
    pub dirty: RenderDirty,
    /// Cursor metadata.
    pub cursor: RenderCursor,
    /// Color metadata.
    pub colors: RenderColors,
    /// Materialized row data.
    pub rows_data: Vec<RenderRow>,
}

impl RenderSnapshot {
    /// Return rows as plain text, replacing empty cells with spaces.
    #[must_use]
    pub fn plain_text_rows(&self) -> Vec<String> {
        self.rows_data
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.codepoint.and_then(char::from_u32).unwrap_or(' '))
                    .collect::<String>()
            })
            .collect()
    }

    /// Borrow the snapshot rows.
    #[must_use]
    pub fn rows(&self) -> &[RenderRow] {
        &self.rows_data
    }
}

/// Shared terminal input options derived from terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalInputOptions {
    /// DEC mode 1: cursor key application mode.
    pub cursor_key_application: bool,
    /// DEC mode 66: keypad key application mode.
    pub keypad_key_application: bool,
    /// DEC mode 1036: alt sends escape prefix.
    pub alt_esc_prefix: bool,
    /// Active Kitty keyboard protocol flags.
    pub kitty_flags: u8,
    /// Active mouse tracking mode.
    pub mouse_tracking_mode: MouseTrackingMode,
    /// Active mouse reporting format.
    pub mouse_format: MouseFormat,
}

/// Terminal construction options in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalOptions {
    /// Terminal columns.
    pub cols: u16,
    /// Terminal rows.
    pub rows: u16,
    /// Maximum scrollback lines.
    pub max_scrollback: usize,
}

/// Point namespace for grid and selection lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointTag {
    /// Coordinates in the active screen history-aware space.
    Active,
    /// Coordinates relative to the viewport.
    Viewport,
    /// Coordinates relative to the visible screen.
    Screen,
    /// Coordinates in scrollback history.
    History,
}

/// Viewport scrolling operation in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollViewport {
    /// Jump to the top of scrollback.
    Top,
    /// Jump to the active bottom.
    Bottom,
    /// Scroll by a delta, where negative moves up.
    Delta(isize),
}

/// Formatter output format in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Plain text formatting.
    Plain,
}

impl From<Format> for libghostty_vt::fmt::Format {
    fn from(value: Format) -> Self {
        match value {
            Format::Plain => Self::Plain,
        }
    }
}

/// Formatter options in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatterOptions {
    /// Output format.
    pub format: Format,
    /// Trim trailing whitespace.
    pub trim: bool,
    /// Unwrap soft-wrapped lines.
    pub unwrap: bool,
}

impl FormatterOptions {
    /// Plain-text formatting options.
    #[must_use]
    pub fn plain(trim: bool, unwrap: bool) -> Self {
        Self {
            format: Format::Plain,
            trim,
            unwrap,
        }
    }

    fn into_wrapper(self) -> libghostty_vt::fmt::FormatterOptions {
        libghostty_vt::fmt::FormatterOptions {
            format: self.format.into(),
            trim: self.trim,
            unwrap: self.unwrap,
        }
    }
}

/// Zed compatibility terminal wrapper built on top of `libghostty-vt`.
#[derive(Debug)]
pub struct Terminal {
    inner: RefCell<Box<WrapperTerminal<'static, 'static>>>,
    callbacks: TerminalCallbacks,
}

impl Terminal {
    /// Create a terminal using the default Zed callback install options.
    pub fn new(options: TerminalOptions) -> AnyResult<Self> {
        Self::new_with_install_options(options, InstallOptions { xtversion: "Zed" })
    }

    /// Create a terminal with explicit callback install options.
    pub fn new_with_install_options(
        options: TerminalOptions,
        install_options: InstallOptions,
    ) -> AnyResult<Self> {
        if options.cols == 0 || options.rows == 0 {
            anyhow::bail!("terminal dimensions must be non-zero")
        }

        let columns = options.cols;
        let rows = options.rows;
        let mut inner = Box::new(
            WrapperTerminal::new(terminal_options(options))
                .context("failed to create ghostty terminal")?,
        );
        let callbacks = TerminalCallbacks::new(columns, rows);
        callbacks
            .install(&mut inner, install_options)
            .context("failed to install ghostty terminal callbacks")?;
        Ok(Self {
            inner: RefCell::new(inner),
            callbacks,
        })
    }

    /// Create a terminal from raw dimensions.
    pub fn new_with_dimensions(columns: u16, rows: u16, max_scrollback: usize) -> AnyResult<Self> {
        Self::new(TerminalOptions {
            cols: columns,
            rows,
            max_scrollback,
        })
    }

    /// Resize the terminal and update the callback-reported size.
    pub fn resize(
        &self,
        columns: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> AnyResult<()> {
        if columns == 0 || rows == 0 || cell_width_px == 0 || cell_height_px == 0 {
            anyhow::bail!("terminal dimensions and cell sizes must be non-zero")
        }

        self.with_inner_mut(|terminal| {
            terminal
                .resize(columns, rows, cell_width_px, cell_height_px)
                .context("failed to resize ghostty terminal")
        })
        .inspect(|_| {
            self.callbacks
                .update_size(columns, rows, cell_width_px, cell_height_px);
        })
    }

    /// Reset the terminal state.
    pub fn reset(&self) {
        let _ = self.with_inner_mut(|terminal| {
            terminal.reset();
            Ok(())
        });
    }

    /// Write VT bytes into the terminal parser.
    pub fn vt_write(&self, data: &[u8]) {
        let _ = self.with_inner_mut(|terminal| {
            terminal.vt_write(data);
            Ok(())
        });
    }

    /// Alias for [`Self::vt_write`].
    pub fn write_vt(&self, data: &[u8]) {
        self.vt_write(data);
    }

    /// Drain queued PTY writes.
    #[must_use]
    pub fn drain_pty_writes(&self) -> Vec<Vec<u8>> {
        self.callbacks.drain_pty_writes()
    }

    /// Drain queued runtime effects.
    #[must_use]
    pub fn drain_effects(&self) -> Vec<RuntimeEffect> {
        self.callbacks.drain_effects()
    }

    /// Scroll the viewport.
    pub fn scroll_viewport(&self, scroll: ScrollViewport) {
        let _ = self.with_inner_mut(|terminal| {
            terminal.scroll_viewport(crate::scroll_viewport(scroll));
            Ok(())
        });
    }

    /// Scroll to the top of the viewport history.
    pub fn scroll_viewport_top(&self) {
        self.scroll_viewport(ScrollViewport::Top);
    }

    /// Scroll to the live bottom.
    pub fn scroll_viewport_bottom(&self) {
        self.scroll_viewport(ScrollViewport::Bottom);
    }

    /// Scroll the viewport by a delta.
    pub fn scroll_viewport_delta(&self, delta: isize) {
        self.scroll_viewport(ScrollViewport::Delta(delta));
    }

    /// Read a terminal mode.
    pub fn mode(&self, mode: u16) -> AnyResult<bool> {
        self.with_inner(|terminal| {
            terminal
                .mode(mode_from_raw(mode))
                .context("failed to read ghostty terminal mode")
        })
    }

    /// Alias for [`Self::mode`].
    pub fn mode_enabled(&self, mode: u16) -> AnyResult<bool> {
        self.mode(mode)
    }

    /// Set a terminal mode.
    pub fn set_mode(&self, mode: u16, enabled: bool) -> AnyResult<()> {
        self.with_inner_mut(|terminal| {
            terminal
                .set_mode(mode_from_raw(mode), enabled)
                .context("failed to set ghostty terminal mode")
        })
    }

    /// Read current terminal dimensions.
    pub fn dimensions(&self) -> AnyResult<(u16, u16)> {
        Ok((self.cols()?, self.rows()?))
    }

    /// Read terminal columns.
    pub fn cols(&self) -> AnyResult<u16> {
        self.with_inner(|terminal| {
            terminal
                .cols()
                .context("failed to read ghostty terminal columns")
        })
    }

    /// Read terminal rows.
    pub fn rows(&self) -> AnyResult<u16> {
        self.with_inner(|terminal| {
            terminal
                .rows()
                .context("failed to read ghostty terminal rows")
        })
    }

    /// Read cursor column.
    pub fn cursor_x(&self) -> AnyResult<u16> {
        self.with_inner(|terminal| {
            terminal
                .cursor_x()
                .context("failed to read ghostty cursor column")
        })
    }

    /// Read cursor row.
    pub fn cursor_y(&self) -> AnyResult<u16> {
        self.with_inner(|terminal| {
            terminal
                .cursor_y()
                .context("failed to read ghostty cursor row")
        })
    }

    /// Read whether the cursor is pending wrap.
    pub fn is_cursor_pending_wrap(&self) -> AnyResult<bool> {
        self.with_inner(|terminal| {
            terminal
                .is_cursor_pending_wrap()
                .context("failed to read ghostty cursor wrap state")
        })
    }

    /// Read cursor visibility.
    pub fn is_cursor_visible(&self) -> AnyResult<bool> {
        self.with_inner(|terminal| {
            terminal
                .is_cursor_visible()
                .context("failed to read ghostty cursor visibility")
        })
    }

    /// Read Kitty keyboard protocol flags as raw bits.
    pub fn kitty_keyboard_flags(&self) -> AnyResult<u8> {
        self.with_inner(|terminal| {
            terminal
                .kitty_keyboard_flags()
                .map(|flags| flags.bits())
                .context("failed to read ghostty kitty keyboard flags")
        })
    }

    /// Read a compatibility cursor summary.
    pub fn cursor_state(&self) -> AnyResult<CursorState> {
        self.with_inner(|terminal| {
            crate::cursor_state(terminal).context("failed to read ghostty cursor state")
        })
    }

    /// Read a compatibility mode summary.
    pub fn mode_state(&self) -> AnyResult<libghostty_vt::terminal::ModeState> {
        self.with_inner(|terminal| {
            terminal
                .mode_state()
                .context("failed to read ghostty terminal mode state")
        })
    }

    /// Read shared input options derived from terminal state.
    pub fn input_options(&self) -> AnyResult<TerminalInputOptions> {
        self.with_inner(|terminal| {
            terminal_input_options(terminal)
                .context("failed to read ghostty terminal input options")
        })
    }

    /// Read the active screen buffer.
    pub fn active_screen(&self) -> AnyResult<Screen> {
        self.cursor_state().map(|cursor| cursor.active_screen)
    }

    /// Read whether mouse tracking is enabled.
    pub fn mouse_tracking_enabled(&self) -> AnyResult<bool> {
        self.with_inner(|terminal| {
            terminal
                .is_mouse_tracking()
                .context("failed to read ghostty mouse tracking state")
        })
    }

    /// Alias for [`Self::mouse_tracking_enabled`].
    pub fn is_mouse_tracking(&self) -> AnyResult<bool> {
        self.mouse_tracking_enabled()
    }

    /// Read the scrollbar state.
    pub fn scrollbar(&self) -> AnyResult<ScrollbarState> {
        self.with_inner(|terminal| {
            crate::scrollbar_state(terminal).context("failed to read ghostty scrollbar state")
        })
    }

    /// Read total terminal rows.
    pub fn total_rows(&self) -> AnyResult<usize> {
        self.with_inner(|terminal| {
            terminal
                .total_rows()
                .context("failed to read ghostty total row count")
        })
    }

    /// Read scrollback rows.
    pub fn scrollback_rows(&self) -> AnyResult<usize> {
        self.with_inner(|terminal| {
            terminal
                .scrollback_rows()
                .context("failed to read ghostty scrollback row count")
        })
    }

    /// Read the terminal foreground color.
    pub fn foreground_color(&self) -> AnyResult<Option<RgbColor>> {
        self.with_inner(|terminal| {
            terminal
                .fg_color()
                .map(|color| color.map(rgb_color_from_wrapper))
                .context("failed to read ghostty foreground color")
        })
    }

    /// Alias for [`Self::foreground_color`].
    pub fn fg_color(&self) -> AnyResult<Option<RgbColor>> {
        self.foreground_color()
    }

    /// Read the terminal background color.
    pub fn background_color(&self) -> AnyResult<Option<RgbColor>> {
        self.with_inner(|terminal| {
            terminal
                .bg_color()
                .map(|color| color.map(rgb_color_from_wrapper))
                .context("failed to read ghostty background color")
        })
    }

    /// Alias for [`Self::background_color`].
    pub fn bg_color(&self) -> AnyResult<Option<RgbColor>> {
        self.background_color()
    }

    /// Read the terminal cursor color.
    pub fn cursor_color(&self) -> AnyResult<Option<RgbColor>> {
        self.with_inner(|terminal| {
            terminal
                .cursor_color()
                .map(|color| color.map(rgb_color_from_wrapper))
                .context("failed to read ghostty cursor color")
        })
    }

    /// Read a palette color override by index.
    pub fn palette_color(&self, index: usize) -> AnyResult<Option<RgbColor>> {
        self.get_terminal_palette_color(index)
    }

    /// Read the full terminal color palette.
    pub fn color_palette(&self) -> AnyResult<[RgbColor; 256]> {
        self.with_inner(|terminal| {
            terminal
                .color_palette()
                .map(|colors| colors.map(rgb_color_from_wrapper))
                .context("failed to read ghostty color palette")
        })
    }

    /// Read an effective color by compatibility color index.
    pub fn effective_color_for_index(&self, index: usize) -> AnyResult<Option<RgbColor>> {
        match index {
            0..=255 => self.palette_color(index),
            256 => self.foreground_color(),
            257 => self.background_color(),
            258 => self.cursor_color(),
            _ => Ok(None),
        }
    }

    /// Read grid row metadata at a compatibility point.
    pub fn grid_row(&self, tag: PointTag, x: u16, y: u32) -> AnyResult<Option<GridRow>> {
        self.with_inner(|terminal| {
            let grid_ref = match terminal.grid_ref(crate::point(tag, x, y)) {
                Ok(grid_ref) => grid_ref,
                Err(libghostty_vt::error::Error::InvalidValue) => return Ok(None),
                Err(error) => {
                    return Err(anyhow::Error::new(error))
                        .context("failed to resolve ghostty grid ref");
                }
            };

            crate::grid_row(&grid_ref)
                .map(Some)
                .context("failed to read ghostty grid ref row")
        })
    }

    /// Read grid cell metadata at a compatibility point.
    pub fn grid_cell(&self, tag: PointTag, x: u16, y: u32) -> AnyResult<Option<GridCell>> {
        self.with_inner(|terminal| {
            let grid_ref = match terminal.grid_ref(crate::point(tag, x, y)) {
                Ok(grid_ref) => grid_ref,
                Err(libghostty_vt::error::Error::InvalidValue) => return Ok(None),
                Err(error) => {
                    return Err(anyhow::Error::new(error))
                        .context("failed to resolve ghostty grid ref");
                }
            };

            crate::grid_cell(&grid_ref)
                .map(Some)
                .context("failed to read ghostty grid ref cell")
        })
    }

    /// Read a history cell.
    pub fn history_cell(&self, x: u16, y: u32) -> AnyResult<Option<GridCell>> {
        self.grid_cell(PointTag::History, x, y)
    }

    /// Format terminal contents.
    pub fn format(&self, options: FormatterOptions) -> AnyResult<String> {
        self.with_inner(|terminal| {
            crate::format_terminal(terminal, options)
                .context("failed to format ghostty terminal output")
        })
    }

    /// Format terminal contents as plain text.
    pub fn format_plain_text(&self, trim: bool, unwrap: bool) -> AnyResult<String> {
        self.format(FormatterOptions::plain(trim, unwrap))
    }

    /// Search terminal contents.
    pub fn search_matches(
        &self,
        needle: &str,
    ) -> AnyResult<Vec<libghostty_vt::terminal::SearchMatch>> {
        self.with_inner(|terminal| {
            terminal
                .search_matches(needle)
                .context("failed to search ghostty terminal")
        })
    }

    /// Read selection text from raw compatibility endpoints.
    #[expect(clippy::too_many_arguments, reason = "compatibility API shape")]
    pub fn selection_string(
        &self,
        start_active: bool,
        start_x: u16,
        start_y: u32,
        end_active: bool,
        end_x: u16,
        end_y: u32,
        rectangle: bool,
        trim: bool,
    ) -> AnyResult<String> {
        self.with_inner(|terminal| {
            terminal
                .selection_string(
                    crate::selection_point(start_active, start_x, start_y),
                    crate::selection_point(end_active, end_x, end_y),
                    rectangle,
                    trim,
                )
                .context("failed to read ghostty selection text")
        })
    }

    /// Read selection text from typed points.
    pub fn selection_string_points(
        &self,
        start: libghostty_vt::terminal::SelectionPoint,
        end: libghostty_vt::terminal::SelectionPoint,
        rectangle: bool,
        trim: bool,
    ) -> AnyResult<String> {
        self.with_inner(|terminal| {
            terminal
                .selection_string(start, end, rectangle, trim)
                .context("failed to read ghostty selection text")
        })
    }

    /// Read a hyperlink URI at screen coordinates.
    pub fn hyperlink_uri_at(&self, x: u16, y: u32) -> AnyResult<Option<String>> {
        self.with_inner(|terminal| {
            terminal
                .hyperlink_uri_at_screen(libghostty_vt::terminal::PointCoordinate::new(x, y))
                .context("failed to read ghostty hyperlink URI")
        })
    }

    /// Read a hyperlink URI at a typed screen point.
    pub fn hyperlink_uri_at_screen(
        &self,
        point: libghostty_vt::terminal::PointCoordinate,
    ) -> AnyResult<Option<String>> {
        self.hyperlink_uri_at(point.x, point.y)
    }

    fn get_terminal_palette_color(&self, index: usize) -> AnyResult<Option<RgbColor>> {
        if index > 255 {
            return Ok(None);
        }

        Ok(Some(self.color_palette()?[index]))
    }

    fn with_inner<T>(
        &self,
        operation: impl FnOnce(&WrapperTerminal<'static, 'static>) -> AnyResult<T>,
    ) -> AnyResult<T> {
        let terminal = self.inner.borrow();
        operation(&terminal)
    }

    fn with_inner_mut<T>(
        &self,
        operation: impl FnOnce(&mut WrapperTerminal<'static, 'static>) -> AnyResult<T>,
    ) -> AnyResult<T> {
        let mut terminal = self.inner.borrow_mut();
        operation(&mut terminal)
    }
}

/// Build shared terminal input options from a wrapper terminal.
pub fn terminal_input_options(terminal: &WrapperTerminal<'_, '_>) -> Result<TerminalInputOptions> {
    let mode_state = terminal.mode_state()?;

    Ok(TerminalInputOptions {
        cursor_key_application: mode_state.app_cursor,
        keypad_key_application: mode_state.app_keypad,
        alt_esc_prefix: terminal.mode(libghostty_vt::terminal::Mode::ALT_ESC_PREFIX)?,
        kitty_flags: terminal.kitty_keyboard_flags()?.bits(),
        mouse_tracking_mode: mouse_tracking_mode_for_terminal(terminal)?,
        mouse_format: mouse_format_for_terminal(terminal)?,
    })
}

/// Convert compatibility terminal options into wrapper options.
#[must_use]
pub fn terminal_options(options: TerminalOptions) -> libghostty_vt::TerminalOptions {
    libghostty_vt::TerminalOptions {
        cols: options.cols,
        rows: options.rows,
        max_scrollback: options.max_scrollback,
    }
}

/// Convert a compatibility point tag and coordinates into a wrapper point.
#[must_use]
pub fn point(tag: PointTag, x: u16, y: u32) -> libghostty_vt::terminal::Point {
    let coordinate = libghostty_vt::terminal::PointCoordinate::new(x, y);
    match tag {
        PointTag::Active => libghostty_vt::terminal::Point::Active(coordinate),
        PointTag::Viewport => libghostty_vt::terminal::Point::Viewport(coordinate),
        PointTag::Screen => libghostty_vt::terminal::Point::Screen(coordinate),
        PointTag::History => libghostty_vt::terminal::Point::History(coordinate),
    }
}

/// Convert a compatibility viewport scroll command into the wrapper shape.
#[must_use]
pub fn scroll_viewport(value: ScrollViewport) -> libghostty_vt::terminal::ScrollViewport {
    match value {
        ScrollViewport::Top => libghostty_vt::terminal::ScrollViewport::Top,
        ScrollViewport::Bottom => libghostty_vt::terminal::ScrollViewport::Bottom,
        ScrollViewport::Delta(delta) => libghostty_vt::terminal::ScrollViewport::Delta(delta),
    }
}

/// Convert raw selection endpoint fields into a wrapper selection point.
#[must_use]
pub fn selection_point(active: bool, x: u16, y: u32) -> libghostty_vt::terminal::SelectionPoint {
    let coordinate = libghostty_vt::terminal::PointCoordinate::new(x, y);
    if active {
        libghostty_vt::terminal::SelectionPoint::Active(coordinate)
    } else {
        libghostty_vt::terminal::SelectionPoint::Screen(coordinate)
    }
}

/// Format terminal contents with compatibility formatter options.
pub fn format_terminal(
    terminal: &WrapperTerminal<'_, '_>,
    options: FormatterOptions,
) -> Result<String> {
    let mut formatter = libghostty_vt::fmt::Formatter::new(terminal, options.into_wrapper())?;
    let required = formatter.format_len()?;

    if required == 0 {
        return Ok(String::new());
    }

    let mut bytes = vec![0_u8; required];
    let written = formatter.format_buf(&mut bytes)?;
    bytes.truncate(written);
    String::from_utf8(bytes).map_err(|_| libghostty_vt::error::Error::InvalidValue)
}

/// Format terminal contents as plain text.
pub fn format_plain_text(
    terminal: &WrapperTerminal<'_, '_>,
    trim: bool,
    unwrap: bool,
) -> Result<String> {
    format_terminal(terminal, FormatterOptions::plain(trim, unwrap))
}

/// Ghostty key code for an unidentified key.
pub const KEY_UNIDENTIFIED: i32 = key::Key::Unidentified as i32;
/// Ghostty key code for the backquote key.
pub const KEY_BACKQUOTE: i32 = key::Key::Backquote as i32;
/// Ghostty key code for the backslash key.
pub const KEY_BACKSLASH: i32 = key::Key::Backslash as i32;
/// Ghostty key code for the left bracket key.
pub const KEY_BRACKET_LEFT: i32 = key::Key::BracketLeft as i32;
/// Ghostty key code for the right bracket key.
pub const KEY_BRACKET_RIGHT: i32 = key::Key::BracketRight as i32;
/// Ghostty key code for the comma key.
pub const KEY_COMMA: i32 = key::Key::Comma as i32;
/// Ghostty key code for the `0` digit key.
pub const KEY_DIGIT_0: i32 = key::Key::Digit0 as i32;
/// Ghostty key code for the equal key.
pub const KEY_EQUAL: i32 = key::Key::Equal as i32;
/// Ghostty key code for the `A` key.
pub const KEY_A: i32 = key::Key::A as i32;
/// Ghostty key code for the minus key.
pub const KEY_MINUS: i32 = key::Key::Minus as i32;
/// Ghostty key code for the period key.
pub const KEY_PERIOD: i32 = key::Key::Period as i32;
/// Ghostty key code for the quote key.
pub const KEY_QUOTE: i32 = key::Key::Quote as i32;
/// Ghostty key code for the semicolon key.
pub const KEY_SEMICOLON: i32 = key::Key::Semicolon as i32;
/// Ghostty key code for the slash key.
pub const KEY_SLASH: i32 = key::Key::Slash as i32;
/// Ghostty key code for the backspace key.
pub const KEY_BACKSPACE: i32 = key::Key::Backspace as i32;
/// Ghostty key code for the enter key.
pub const KEY_ENTER: i32 = key::Key::Enter as i32;
/// Ghostty key code for the space key.
pub const KEY_SPACE: i32 = key::Key::Space as i32;
/// Ghostty key code for the tab key.
pub const KEY_TAB: i32 = key::Key::Tab as i32;
/// Ghostty key code for the delete key.
pub const KEY_DELETE: i32 = key::Key::Delete as i32;
/// Ghostty key code for the end key.
pub const KEY_END: i32 = key::Key::End as i32;
/// Ghostty key code for the home key.
pub const KEY_HOME: i32 = key::Key::Home as i32;
/// Ghostty key code for the insert key.
pub const KEY_INSERT: i32 = key::Key::Insert as i32;
/// Ghostty key code for the page down key.
pub const KEY_PAGE_DOWN: i32 = key::Key::PageDown as i32;
/// Ghostty key code for the page up key.
pub const KEY_PAGE_UP: i32 = key::Key::PageUp as i32;
/// Ghostty key code for the down arrow key.
pub const KEY_ARROW_DOWN: i32 = key::Key::ArrowDown as i32;
/// Ghostty key code for the left arrow key.
pub const KEY_ARROW_LEFT: i32 = key::Key::ArrowLeft as i32;
/// Ghostty key code for the right arrow key.
pub const KEY_ARROW_RIGHT: i32 = key::Key::ArrowRight as i32;
/// Ghostty key code for the up arrow key.
pub const KEY_ARROW_UP: i32 = key::Key::ArrowUp as i32;
/// Ghostty key code for the escape key.
pub const KEY_ESCAPE: i32 = key::Key::Escape as i32;
/// Ghostty key code for the first function key.
pub const KEY_F1: i32 = key::Key::F1 as i32;
/// Ghostty key code for the `C` key.
pub const KEY_C: i32 = key::Key::C as i32;

/// Shift modifier bit.
pub const MODIFIER_SHIFT: u16 = key::Mods::SHIFT.bits();
/// Control modifier bit.
pub const MODIFIER_CONTROL: u16 = key::Mods::CTRL.bits();
/// Alt modifier bit.
pub const MODIFIER_ALT: u16 = key::Mods::ALT.bits();
/// Super/Command/Windows modifier bit.
pub const MODIFIER_SUPER: u16 = key::Mods::SUPER.bits();

/// Resolve a Ghostty key code from a textual key name.
#[must_use]
pub fn key_from_name(name: &str) -> Option<i32> {
    match name {
        "tab" => Some(KEY_TAB),
        "escape" => Some(KEY_ESCAPE),
        "enter" => Some(KEY_ENTER),
        "backspace" | "back" => Some(KEY_BACKSPACE),
        "space" => Some(KEY_SPACE),
        "home" => Some(KEY_HOME),
        "end" => Some(KEY_END),
        "pageup" => Some(KEY_PAGE_UP),
        "pagedown" => Some(KEY_PAGE_DOWN),
        "up" => Some(KEY_ARROW_UP),
        "down" => Some(KEY_ARROW_DOWN),
        "left" => Some(KEY_ARROW_LEFT),
        "right" => Some(KEY_ARROW_RIGHT),
        "insert" => Some(KEY_INSERT),
        "delete" => Some(KEY_DELETE),
        _ => {
            if let Some(function_number) = name
                .strip_prefix('f')
                .and_then(|suffix| suffix.parse::<i32>().ok())
                && (1..=20).contains(&function_number)
            {
                return Some(KEY_F1 + function_number - 1);
            }

            let mut characters = name.chars();
            let character = match (characters.next(), characters.next()) {
                (Some(character), None) => character,
                _ => return None,
            };

            match character {
                'a'..='z' => Some(KEY_A + (character as i32 - 'a' as i32)),
                'A'..='Z' => Some(KEY_A + (character as i32 - 'A' as i32)),
                '0'..='9' => Some(KEY_DIGIT_0 + (character as i32 - '0' as i32)),
                '!' => Some(KEY_DIGIT_0 + 1),
                '@' => Some(KEY_DIGIT_0 + 2),
                '#' => Some(KEY_DIGIT_0 + 3),
                '$' => Some(KEY_DIGIT_0 + 4),
                '%' => Some(KEY_DIGIT_0 + 5),
                '^' => Some(KEY_DIGIT_0 + 6),
                '&' => Some(KEY_DIGIT_0 + 7),
                '*' => Some(KEY_DIGIT_0 + 8),
                '(' => Some(KEY_DIGIT_0 + 9),
                ')' => Some(KEY_DIGIT_0),
                '`' | '~' => Some(KEY_BACKQUOTE),
                '-' | '_' => Some(KEY_MINUS),
                '=' | '+' => Some(KEY_EQUAL),
                '[' | '{' => Some(KEY_BRACKET_LEFT),
                ']' | '}' => Some(KEY_BRACKET_RIGHT),
                '\\' | '|' => Some(KEY_BACKSLASH),
                ';' | ':' => Some(KEY_SEMICOLON),
                '\'' | '"' => Some(KEY_QUOTE),
                ',' | '<' => Some(KEY_COMMA),
                '.' | '>' => Some(KEY_PERIOD),
                '/' | '?' => Some(KEY_SLASH),
                ' ' => Some(KEY_SPACE),
                _ => None,
            }
        }
    }
}

/// Key event action type in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Key release.
    Release,
    /// Key press.
    Press,
    /// Key repeat.
    Repeat,
}

impl KeyAction {
    fn into_wrapper(self) -> key::Action {
        match self {
            Self::Release => key::Action::Release,
            Self::Press => key::Action::Press,
            Self::Repeat => key::Action::Repeat,
        }
    }
}

/// Compatibility key event wrapper.
#[derive(Debug)]
pub struct KeyEvent {
    inner: key::Event<'static>,
}

impl KeyEvent {
    /// Create a key event.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: key::Event::new()?,
        })
    }

    /// Set the action.
    pub fn set_action(&mut self, action: KeyAction) {
        self.inner.set_action(action.into_wrapper());
    }

    /// Set the raw key code.
    pub fn set_key(&mut self, key_code: i32) {
        self.inner.set_key(key_from_raw(key_code));
    }

    /// Set modifier bits.
    pub fn set_mods(&mut self, modifiers: u16) {
        self.inner.set_mods(key::Mods::from_bits_retain(modifiers));
    }

    /// Alias for [`Self::set_mods`].
    pub fn set_modifiers(&mut self, modifiers: u16) {
        self.set_mods(modifiers);
    }

    /// Set consumed modifier bits.
    pub fn set_consumed_mods(&mut self, modifiers: u16) {
        self.inner
            .set_consumed_mods(key::Mods::from_bits_retain(modifiers));
    }

    /// Alias for [`Self::set_consumed_mods`].
    pub fn set_consumed_modifiers(&mut self, modifiers: u16) {
        self.set_consumed_mods(modifiers);
    }

    /// Set composition state.
    pub fn set_composing(&mut self, composing: bool) {
        self.inner.set_composing(composing);
    }

    /// Set the unshifted Unicode codepoint.
    pub fn set_unshifted_codepoint(&mut self, codepoint: u32) {
        if let Some(codepoint) = char::from_u32(codepoint) {
            self.inner.set_unshifted_codepoint(codepoint);
        }
    }

    /// Set UTF-8 text for the event.
    pub fn set_utf8(&mut self, text: &str) {
        self.inner.set_utf8(Some(text));
    }
}

/// Compatibility key encoder wrapper.
#[derive(Debug)]
pub struct KeyEncoder {
    inner: key::Encoder<'static>,
}

impl KeyEncoder {
    /// Create a key encoder.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: key::Encoder::new()?,
        })
    }

    /// Apply terminal-derived input options.
    pub fn set_options(&mut self, options: &TerminalInputOptions) {
        self.inner
            .set_cursor_key_application(options.cursor_key_application)
            .set_keypad_key_application(options.keypad_key_application)
            .set_alt_esc_prefix(options.alt_esc_prefix)
            .set_kitty_flags(key::KittyKeyFlags::from_bits_retain(options.kitty_flags))
            .set_macos_option_as_alt(key::OptionAsAlt::False);

        // `modifyOtherKeys` is not represented in the shared input summary yet.
        self.inner.set_modify_other_keys_state_2(false);
    }

    /// Set Kitty keyboard protocol flags.
    pub fn set_kitty_flags(&mut self, flags: u8) {
        self.inner
            .set_kitty_flags(key::KittyKeyFlags::from_bits_retain(flags));
    }

    /// Set cursor key application mode.
    pub fn set_cursor_key_application(&mut self, enabled: bool) {
        self.inner.set_cursor_key_application(enabled);
    }

    /// Set keypad key application mode.
    pub fn set_keypad_key_application(&mut self, enabled: bool) {
        self.inner.set_keypad_key_application(enabled);
    }

    /// Set alt-escape-prefix mode.
    pub fn set_alt_esc_prefix(&mut self, enabled: bool) {
        self.inner.set_alt_esc_prefix(enabled);
    }

    /// Set modifyOtherKeys mode 2.
    pub fn set_modify_other_keys_state_2(&mut self, enabled: bool) {
        self.inner.set_modify_other_keys_state_2(enabled);
    }

    /// Set macOS option-as-alt behavior.
    pub fn set_macos_option_as_alt(&mut self, enabled: bool) {
        let option = if enabled {
            key::OptionAsAlt::True
        } else {
            key::OptionAsAlt::False
        };
        self.inner.set_macos_option_as_alt(option);
    }

    /// Encode a key event into terminal bytes.
    pub fn encode(&mut self, event: &KeyEvent) -> Result<Vec<u8>> {
        let mut encoded = Vec::new();
        self.inner.encode_to_vec(&event.inner, &mut encoded)?;
        Ok(encoded)
    }
}

/// Mouse event action in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// Mouse press.
    Press,
    /// Mouse release.
    Release,
    /// Mouse motion.
    Motion,
}

impl MouseAction {
    fn into_wrapper(self) -> mouse::Action {
        match self {
            Self::Press => mouse::Action::Press,
            Self::Release => mouse::Action::Release,
            Self::Motion => mouse::Action::Motion,
        }
    }
}

/// Mouse button in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Unknown button.
    Unknown,
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
    /// Fourth auxiliary button.
    Four,
    /// Fifth auxiliary button.
    Five,
}

impl MouseButton {
    fn into_wrapper(self) -> mouse::Button {
        match self {
            Self::Unknown => mouse::Button::Unknown,
            Self::Left => mouse::Button::Left,
            Self::Right => mouse::Button::Right,
            Self::Middle => mouse::Button::Middle,
            Self::Four => mouse::Button::Four,
            Self::Five => mouse::Button::Five,
        }
    }
}

/// Mouse tracking mode in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTrackingMode {
    /// Mouse reporting disabled.
    None,
    /// X10 reporting.
    X10,
    /// Normal click reporting.
    Normal,
    /// Button tracking.
    Button,
    /// Motion tracking.
    Any,
}

impl MouseTrackingMode {
    fn into_wrapper(self) -> mouse::TrackingMode {
        match self {
            Self::None => mouse::TrackingMode::None,
            Self::X10 => mouse::TrackingMode::X10,
            Self::Normal => mouse::TrackingMode::Normal,
            Self::Button => mouse::TrackingMode::Button,
            Self::Any => mouse::TrackingMode::Any,
        }
    }
}

/// Mouse format in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseFormat {
    /// X10 format.
    X10,
    /// UTF-8 format.
    Utf8,
    /// SGR format.
    Sgr,
    /// URXVT format.
    Urxvt,
    /// SGR pixels format.
    SgrPixels,
}

impl MouseFormat {
    fn into_wrapper(self) -> mouse::Format {
        match self {
            Self::X10 => mouse::Format::X10,
            Self::Utf8 => mouse::Format::Utf8,
            Self::Sgr => mouse::Format::Sgr,
            Self::Urxvt => mouse::Format::Urxvt,
            Self::SgrPixels => mouse::Format::SgrPixels,
        }
    }
}

/// Mouse encoder geometry in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEncoderSize {
    /// Full screen width in pixels.
    pub screen_width: u32,
    /// Full screen height in pixels.
    pub screen_height: u32,
    /// Cell width in pixels.
    pub cell_width: u32,
    /// Cell height in pixels.
    pub cell_height: u32,
    /// Top padding in pixels.
    pub padding_top: u32,
    /// Bottom padding in pixels.
    pub padding_bottom: u32,
    /// Right padding in pixels.
    pub padding_right: u32,
    /// Left padding in pixels.
    pub padding_left: u32,
}

impl From<MouseEncoderSize> for mouse::EncoderSize {
    fn from(value: MouseEncoderSize) -> Self {
        Self {
            screen_width: value.screen_width,
            screen_height: value.screen_height,
            cell_width: value.cell_width,
            cell_height: value.cell_height,
            padding_top: value.padding_top,
            padding_bottom: value.padding_bottom,
            padding_right: value.padding_right,
            padding_left: value.padding_left,
        }
    }
}

/// Compatibility mouse event wrapper.
#[derive(Debug)]
pub struct MouseEvent {
    inner: mouse::Event<'static>,
}

impl MouseEvent {
    /// Create a mouse event.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: mouse::Event::new()?,
        })
    }

    /// Set the action.
    pub fn set_action(&mut self, action: MouseAction) {
        self.inner.set_action(action.into_wrapper());
    }

    /// Set the button.
    pub fn set_button(&mut self, button: Option<MouseButton>) {
        self.inner.set_button(button.map(MouseButton::into_wrapper));
    }

    /// Clear the current button.
    pub fn clear_button(&mut self) {
        self.set_button(None);
    }

    /// Set modifier bits.
    pub fn set_mods(&mut self, modifiers: u16) {
        self.inner.set_mods(key::Mods::from_bits_retain(modifiers));
    }

    /// Alias for [`Self::set_mods`].
    pub fn set_modifiers(&mut self, modifiers: u16) {
        self.set_mods(modifiers);
    }

    /// Set surface-space mouse position.
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.inner.set_position(mouse::Position { x, y });
    }
}

/// Compatibility mouse encoder wrapper.
#[derive(Debug)]
pub struct MouseEncoder {
    inner: mouse::Encoder<'static>,
}

impl MouseEncoder {
    /// Create a mouse encoder.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: mouse::Encoder::new()?,
        })
    }

    /// Apply terminal-derived input options.
    pub fn set_options(&mut self, options: &TerminalInputOptions) {
        self.inner
            .set_tracking_mode(options.mouse_tracking_mode.into_wrapper())
            .set_format(options.mouse_format.into_wrapper());
    }

    /// Set tracking mode.
    pub fn set_tracking_mode(&mut self, tracking_mode: MouseTrackingMode) {
        self.inner.set_tracking_mode(tracking_mode.into_wrapper());
    }

    /// Set output format.
    pub fn set_format(&mut self, format: MouseFormat) {
        self.inner.set_format(format.into_wrapper());
    }

    /// Set encoder geometry.
    pub fn set_size(&mut self, size: MouseEncoderSize) {
        self.inner.set_size(size.into());
    }

    /// Encode a mouse event into terminal bytes.
    pub fn encode(&mut self, event: &MouseEvent) -> Result<Vec<u8>> {
        let mut encoded = Vec::new();
        self.inner.encode_to_vec(&event.inner, &mut encoded)?;
        Ok(encoded)
    }
}

/// Focus event in the Zed compatibility shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEvent {
    /// Focus gained.
    Gained,
    /// Focus lost.
    Lost,
}

impl FocusEvent {
    fn into_wrapper(self) -> focus::Event {
        match self {
            Self::Gained => focus::Event::Gained,
            Self::Lost => focus::Event::Lost,
        }
    }
}

/// Encode a focus event into terminal bytes.
pub fn encode_focus(event: FocusEvent) -> Result<Vec<u8>> {
    let mut encoded = [0u8; 16];
    let written = event.into_wrapper().encode(&mut encoded)?;
    Ok(encoded[..written].to_vec())
}

fn mode_from_raw(mode: u16) -> libghostty_vt::terminal::Mode {
    let is_ansi = (mode & 0x8000) != 0;
    let raw_mode = mode & 0x7fff;
    libghostty_vt::terminal::Mode::new(
        raw_mode,
        if is_ansi {
            libghostty_vt::terminal::ModeKind::Ansi
        } else {
            libghostty_vt::terminal::ModeKind::Dec
        },
    )
}

/// Build a compatibility cursor summary from a wrapper terminal.
pub fn cursor_state(terminal: &WrapperTerminal<'_, '_>) -> Result<CursorState> {
    let cursor = terminal.cursor_state()?;
    Ok(CursorState {
        column: cursor.column,
        row: cursor.row,
        pending_wrap: cursor.pending_wrap,
        visible: cursor.visible,
        active_screen: screen_from_raw(cursor.active_screen as i32),
        kitty_flags: cursor.kitty_keyboard_flags.bits(),
    })
}

/// Build a compatibility scrollbar summary from a wrapper terminal.
pub fn scrollbar_state(terminal: &WrapperTerminal<'_, '_>) -> Result<ScrollbarState> {
    terminal.scrollbar().map(|scrollbar| ScrollbarState {
        total: scrollbar.total,
        offset: scrollbar.offset,
        len: scrollbar.len,
    })
}

/// Convert a wrapper grid row into the compatibility shape.
pub fn grid_row_from_wrapper(row: WrapperRow) -> Result<GridRow> {
    Ok(GridRow {
        wrapped: row.is_wrapped()?,
        wrap_continuation: row.is_wrap_continuation()?,
        has_grapheme_cluster: row.has_grapheme_cluster()?,
        is_styled: row.is_styled()?,
        has_hyperlink: row.has_hyperlink()?,
        semantic_prompt: row_semantic_prompt_from_wrapper(row.semantic_prompt()?),
        has_kitty_virtual_placeholder: row.has_kitty_virtual_placeholder()?,
        is_dirty: row.is_dirty()?,
    })
}

/// Convert a wrapper grid cell and style into the compatibility shape.
pub fn grid_cell_from_wrapper(cell: WrapperCell, style: WrapperStyle) -> Result<GridCell> {
    let codepoint = if cell.has_text()? {
        Some(cell.codepoint()?)
    } else {
        None
    };

    Ok(GridCell {
        codepoint,
        wide: cell_wide_from_wrapper(cell.wide()?),
        style: cell_style_from_wrapper(style),
    })
}

/// Resolve a compatibility grid row from a wrapper grid reference.
pub fn grid_row(grid_ref: &GridRef<'_>) -> Result<GridRow> {
    grid_row_from_wrapper(grid_ref.row()?)
}

/// Resolve a compatibility grid cell from a wrapper grid reference.
pub fn grid_cell(grid_ref: &GridRef<'_>) -> Result<GridCell> {
    let style = grid_ref.style()?;
    let cell = grid_ref.cell()?;
    grid_cell_from_wrapper(cell, style)
}

/// Eager render-state adapter for Zed compatibility consumers.
#[derive(Debug)]
pub struct RenderState {
    inner: WrapperRenderState<'static>,
}

impl RenderState {
    /// Create a render state.
    pub fn new() -> Result<Self> {
        let inner = WrapperRenderState::new()?;
        Ok(Self { inner })
    }

    /// Update the render state from a terminal and return an eager snapshot.
    pub fn update(&mut self, terminal: &Terminal) -> AnyResult<RenderSnapshot> {
        terminal.with_inner(|terminal| {
            let snapshot = self.inner.update(terminal)?;
            Self::adapt_snapshot(&snapshot).map_err(anyhow::Error::new)
        })
    }

    /// Alias for [`Self::update`].
    pub fn snapshot(&mut self, terminal: &Terminal) -> AnyResult<RenderSnapshot> {
        self.update(terminal)
    }

    fn adapt_snapshot(snapshot: &Snapshot<'static, '_>) -> Result<RenderSnapshot> {
        let columns = snapshot.cols()?;
        let rows = snapshot.rows()?;
        let dirty = render_dirty_from_wrapper(snapshot.dirty()?);
        let cursor = render_cursor_from_wrapper(snapshot)?;
        let colors = render_colors_from_wrapper(snapshot.colors()?);

        let mut row_iterator = RowIterator::new()?;
        let mut row_iteration = row_iterator.update(snapshot)?;
        let mut row_cells = CellIterator::new()?;
        let mut rows_data = Vec::with_capacity(rows as usize);

        while let Some(row) = row_iteration.next() {
            rows_data.push(render_row_from_wrapper(row, &mut row_cells, columns)?);
        }

        snapshot.set_dirty(Dirty::Clean)?;

        Ok(RenderSnapshot {
            columns,
            rows,
            dirty,
            cursor,
            colors,
            rows_data,
        })
    }
}

fn render_dirty_from_wrapper(dirty: Dirty) -> RenderDirty {
    match dirty {
        Dirty::Clean => RenderDirty::Clean,
        Dirty::Partial => RenderDirty::Partial,
        Dirty::Full => RenderDirty::Full,
    }
}

fn screen_from_raw(raw: i32) -> Screen {
    if raw == 1 {
        Screen::Alternate
    } else {
        Screen::Primary
    }
}

fn row_semantic_prompt_from_wrapper(prompt: WrapperRowSemanticPrompt) -> RowSemanticPrompt {
    match prompt {
        WrapperRowSemanticPrompt::None => RowSemanticPrompt::None,
        WrapperRowSemanticPrompt::Prompt => RowSemanticPrompt::Prompt,
        WrapperRowSemanticPrompt::Continuation => RowSemanticPrompt::PromptContinuation,
    }
}

fn render_cursor_from_wrapper(snapshot: &Snapshot<'static, '_>) -> Result<RenderCursor> {
    let viewport = snapshot.cursor_viewport()?;
    Ok(RenderCursor {
        visible: snapshot.cursor_visible()?,
        blinking: snapshot.cursor_blinking()?,
        in_viewport: viewport.is_some(),
        viewport_column: viewport.map(|cursor| cursor.x),
        viewport_row: viewport.map(|cursor| cursor.y),
    })
}

fn render_colors_from_wrapper(colors: libghostty_vt::render::Colors) -> RenderColors {
    RenderColors {
        background: rgb_color_from_wrapper(colors.background),
        foreground: rgb_color_from_wrapper(colors.foreground),
        cursor: colors.cursor.map(rgb_color_from_wrapper),
        palette: colors
            .palette
            .into_iter()
            .map(rgb_color_from_wrapper)
            .collect(),
    }
}

fn render_cell_from_wrapper(
    cell: &libghostty_vt::render::CellIteration<'static, '_>,
) -> Result<RenderCell> {
    let raw_cell = cell.raw_cell()?;
    let graphemes = cell.graphemes()?.into_iter().map(u32::from).collect();

    Ok(RenderCell {
        raw: raw_cell.as_raw(),
        codepoint: Some(raw_cell.codepoint()?).filter(|codepoint| *codepoint != 0),
        wide: cell_wide_from_wrapper(raw_cell.wide()?),
        style: cell_style_from_wrapper(cell.style()?),
        resolved_background: cell.bg_color()?.map(rgb_color_from_wrapper),
        resolved_foreground: cell.fg_color()?.map(rgb_color_from_wrapper),
        has_hyperlink: raw_cell.has_hyperlink()?,
        graphemes,
    })
}

fn render_row_from_wrapper(
    row: &RowIteration<'static, '_>,
    row_cells: &mut CellIterator<'static>,
    columns: u16,
) -> Result<RenderRow> {
    let raw_row = row.raw_row()?;
    let mut cell_iteration = row_cells.update(row)?;
    let mut cells = Vec::with_capacity(columns as usize);

    while let Some(cell) = cell_iteration.next() {
        cells.push(render_cell_from_wrapper(cell)?);
    }

    let render_row = RenderRow {
        dirty: row.dirty()?,
        wrapped: raw_row.is_wrapped()?,
        wrap_continuation: raw_row.is_wrap_continuation()?,
        has_kitty_virtual_placeholder: raw_row.has_kitty_virtual_placeholder()?,
        cells,
    };

    row.set_dirty(false)?;

    Ok(render_row)
}

fn cell_wide_from_wrapper(wide: WrapperCellWide) -> CellWide {
    match wide {
        WrapperCellWide::Narrow => CellWide::Narrow,
        WrapperCellWide::Wide => CellWide::Wide,
        WrapperCellWide::SpacerTail => CellWide::SpacerTail,
        WrapperCellWide::SpacerHead => CellWide::SpacerHead,
    }
}

fn rgb_color_from_wrapper(color: WrapperRgbColor) -> RgbColor {
    RgbColor {
        red: color.r,
        green: color.g,
        blue: color.b,
    }
}

fn style_color_from_wrapper(color: WrapperStyleColor) -> StyleColor {
    match color {
        WrapperStyleColor::None => StyleColor::None,
        WrapperStyleColor::Palette(index) => StyleColor::Palette(index.0),
        WrapperStyleColor::Rgb(color) => StyleColor::Rgb(rgb_color_from_wrapper(color)),
    }
}

fn cell_style_from_wrapper(style: WrapperStyle) -> CellStyle {
    CellStyle {
        foreground: style_color_from_wrapper(style.fg_color),
        background: style_color_from_wrapper(style.bg_color),
        underline_color: style_color_from_wrapper(style.underline_color),
        bold: style.bold,
        italic: style.italic,
        faint: style.faint,
        blink: style.blink,
        inverse: style.inverse,
        invisible: style.invisible,
        strikethrough: style.strikethrough,
        overline: style.overline,
        underline: style.underline as i32,
    }
}

fn key_from_raw(key_code: i32) -> key::Key {
    u32::try_from(key_code)
        .ok()
        .and_then(|key_code| key::Key::try_from(key_code).ok())
        .unwrap_or(key::Key::Unidentified)
}

fn mouse_tracking_mode_for_terminal(
    terminal: &WrapperTerminal<'_, '_>,
) -> Result<MouseTrackingMode> {
    if terminal.mode(libghostty_vt::terminal::Mode::ANY_MOUSE)? {
        Ok(MouseTrackingMode::Any)
    } else if terminal.mode(libghostty_vt::terminal::Mode::BUTTON_MOUSE)? {
        Ok(MouseTrackingMode::Button)
    } else if terminal.mode(libghostty_vt::terminal::Mode::NORMAL_MOUSE)? {
        Ok(MouseTrackingMode::Normal)
    } else if terminal.mode(libghostty_vt::terminal::Mode::X10_MOUSE)? {
        Ok(MouseTrackingMode::X10)
    } else {
        Ok(MouseTrackingMode::None)
    }
}

fn mouse_format_for_terminal(terminal: &WrapperTerminal<'_, '_>) -> Result<MouseFormat> {
    if terminal.mode(libghostty_vt::terminal::Mode::SGR_PIXELS_MOUSE)? {
        Ok(MouseFormat::SgrPixels)
    } else if terminal.mode(libghostty_vt::terminal::Mode::SGR_MOUSE)? {
        Ok(MouseFormat::Sgr)
    } else if terminal.mode(libghostty_vt::terminal::Mode::URXVT_MOUSE)? {
        Ok(MouseFormat::Urxvt)
    } else if terminal.mode(libghostty_vt::terminal::Mode::UTF8_MOUSE)? {
        Ok(MouseFormat::Utf8)
    } else {
        Ok(MouseFormat::X10)
    }
}

#[derive(Debug, Default)]
struct CallbackState {
    pending_pty_writes: Vec<Vec<u8>>,
    pending_effects: Vec<RuntimeEffect>,
    columns: u16,
    rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
}

/// Shared callback queue and size state for a terminal instance.
#[derive(Debug, Clone)]
pub struct TerminalCallbacks {
    state: Rc<RefCell<CallbackState>>,
}

impl TerminalCallbacks {
    /// Create a callback queue for a terminal of the given size.
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            state: Rc::new(RefCell::new(CallbackState {
                columns,
                rows,
                cell_width_px: 1,
                cell_height_px: 1,
                ..CallbackState::default()
            })),
        }
    }

    /// Install Zed callback behavior on a terminal.
    ///
    /// Call this after the terminal has reached its final storage location.
    /// The underlying callback registration keeps terminal-owned callback
    /// vtables alive by address, so moving the terminal afterward can
    /// invalidate the callback userdata.
    pub fn install(
        &self,
        terminal: &mut WrapperTerminal<'static, 'static>,
        options: InstallOptions,
    ) -> Result<()> {
        {
            let callbacks = self.clone();
            terminal.on_pty_write(move |_terminal, data| callbacks.push_pty_write(data))?;
        }
        {
            let callbacks = self.clone();
            terminal.on_bell(move |_terminal| callbacks.push_effect(RuntimeEffect::Bell))?;
        }
        terminal.on_enquiry(|_terminal| None)?;
        terminal.on_xtversion(move |_terminal| Some(options.xtversion))?;
        {
            let callbacks = self.clone();
            terminal.on_title_changed(move |terminal| {
                let title = terminal
                    .title()
                    .ok()
                    .and_then(|title| (!title.is_empty()).then(|| title.to_owned()));
                callbacks.push_effect(RuntimeEffect::TitleChanged(title));
            })?;
        }
        terminal.on_color_scheme(|terminal| color_scheme_for_terminal(terminal))?;
        {
            let callbacks = self.clone();
            terminal.on_size(move |_terminal| {
                let state = callbacks.state.borrow();
                Some(SizeReportSize {
                    rows: state.rows,
                    columns: state.columns,
                    cell_width: state.cell_width_px,
                    cell_height: state.cell_height_px,
                })
            })?;
        }
        terminal.on_device_attributes(|_terminal| Some(default_device_attributes()))?;
        Ok(())
    }

    /// Update the terminal size reported by the size callback.
    pub fn update_size(&self, columns: u16, rows: u16, cell_width_px: u32, cell_height_px: u32) {
        let mut state = self.state.borrow_mut();
        state.columns = columns;
        state.rows = rows;
        state.cell_width_px = cell_width_px.max(1);
        state.cell_height_px = cell_height_px.max(1);
    }

    /// Drain queued PTY writes.
    pub fn drain_pty_writes(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.state.borrow_mut().pending_pty_writes)
    }

    /// Drain queued runtime effects.
    pub fn drain_effects(&self) -> Vec<RuntimeEffect> {
        std::mem::take(&mut self.state.borrow_mut().pending_effects)
    }

    fn push_pty_write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        self.state
            .borrow_mut()
            .pending_pty_writes
            .push(bytes.to_vec());
    }

    fn push_effect(&self, effect: RuntimeEffect) {
        self.state.borrow_mut().pending_effects.push(effect);
    }
}

fn color_scheme_for_terminal(terminal: &WrapperTerminal<'_, '_>) -> Option<ColorScheme> {
    let mut render_state = WrapperRenderState::new().ok()?;
    let snapshot = render_state.update(terminal).ok()?;
    let background = snapshot.colors().ok()?.background;
    Some(if rgb_is_light(rgb_color_from_wrapper(background)) {
        ColorScheme::Light
    } else {
        ColorScheme::Dark
    })
}

fn rgb_is_light(color: RgbColor) -> bool {
    let luminance = (0.299 * f32::from(color.red))
        + (0.587 * f32::from(color.green))
        + (0.114 * f32::from(color.blue));
    luminance >= 128.0
}

fn default_device_attributes() -> DeviceAttributes {
    DeviceAttributes {
        primary: PrimaryDeviceAttributes::new(
            ConformanceLevel(62),
            [
                DeviceAttributeFeature(1),
                DeviceAttributeFeature(6),
                DeviceAttributeFeature(22),
            ],
        ),
        secondary: SecondaryDeviceAttributes {
            device_type: DeviceType(1),
            firmware_version: 1,
            rom_cartridge: 0,
        },
        tertiary: TertiaryDeviceAttributes { unit_id: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FocusEvent, InstallOptions, KEY_A, KEY_ARROW_UP, KEY_C, KEY_DIGIT_0, KEY_MINUS, KEY_SLASH,
        KEY_UNIDENTIFIED, KeyAction, KeyEncoder, KeyEvent, MODIFIER_CONTROL, MouseAction,
        MouseButton, MouseEncoder, MouseEncoderSize, MouseEvent, MouseFormat, MouseTrackingMode,
        PointTag, RenderDirty, RenderState, RowSemanticPrompt, RuntimeEffect, Screen, Terminal,
        TerminalOptions, encode_focus, key_from_name,
    };
    fn install_callbacks() -> Terminal {
        Terminal::new_with_install_options(
            TerminalOptions {
                cols: 20,
                rows: 4,
                max_scrollback: 128,
            },
            InstallOptions { xtversion: "Zed" },
        )
        .expect("create terminal")
    }

    #[test]
    fn callbacks_queue_runtime_effects() {
        let terminal = install_callbacks();

        terminal.vt_write(b"\x07");
        assert_eq!(terminal.drain_effects(), vec![RuntimeEffect::Bell]);
        assert!(terminal.drain_effects().is_empty());

        terminal.vt_write(b"\x1b]2;zed title\x07");
        assert_eq!(
            terminal.drain_effects(),
            vec![RuntimeEffect::TitleChanged(Some("zed title".to_owned()))]
        );

        terminal.vt_write(b"\x1b]2;\x07");
        assert_eq!(
            terminal.drain_effects(),
            vec![RuntimeEffect::TitleChanged(None)]
        );
    }

    #[test]
    fn callbacks_queue_pty_replies() {
        let terminal = install_callbacks();

        terminal.vt_write(b"\x1b[>q");
        let replies = terminal.drain_pty_writes();
        assert_eq!(replies.len(), 1, "expected one XTVERSION reply");
        let reply = String::from_utf8(replies.into_iter().next().expect("reply"))
            .expect("XTVERSION reply should be valid UTF-8");
        assert!(
            reply.contains("Zed"),
            "expected XTVERSION reply to contain Zed, got {reply:?}"
        );

        terminal.vt_write(b"\x05");
        assert!(
            terminal.drain_pty_writes().is_empty(),
            "expected ENQ callback to stay silent"
        );
    }

    #[test]
    fn eager_render_snapshot_includes_text() {
        let terminal = install_callbacks();
        terminal.vt_write(b"abc\r\ndef\r\n");

        let mut render_state = RenderState::new().expect("create render state");
        let snapshot = render_state
            .snapshot(&terminal)
            .expect("snapshot render state");

        assert_eq!(snapshot.columns, 20);
        assert_eq!(snapshot.rows, 4);
        assert!(matches!(
            snapshot.dirty,
            RenderDirty::Partial | RenderDirty::Full
        ));
        let combined = snapshot.plain_text_rows().join("\n");
        assert!(combined.contains("abc"));
        assert!(combined.contains("def"));
    }

    #[test]
    fn cursor_and_scrollbar_summaries_match_terminal_state() {
        let terminal = install_callbacks();

        let cursor = terminal.cursor_state().expect("cursor state");
        let scrollbar = terminal.scrollbar().expect("scrollbar state");

        assert_eq!(cursor.column, 0);
        assert_eq!(cursor.row, 0);
        assert!(cursor.visible);
        assert_eq!(cursor.active_screen, Screen::Primary);
        assert_eq!(scrollbar.offset, 0);
        assert!(scrollbar.total >= scrollbar.len);
    }

    #[test]
    fn grid_summaries_match_wrapped_prompt_content() {
        let terminal = install_callbacks();
        terminal.vt_write(b"prompt\r\n");

        let row = terminal
            .grid_row(PointTag::Active, 0, 0)
            .expect("grid row")
            .expect("row");
        let cell = terminal
            .grid_cell(PointTag::Active, 0, 0)
            .expect("grid cell")
            .expect("cell");

        assert!(!row.wrap_continuation);
        assert_eq!(row.semantic_prompt, RowSemanticPrompt::None);
        assert_eq!(cell.codepoint, Some(u32::from('p')));
    }

    #[test]
    fn terminal_input_options_follow_terminal_modes() {
        let terminal = install_callbacks();
        terminal
            .set_mode(libghostty_vt::terminal::Mode::DECCKM.into(), true)
            .expect("enable app cursor mode");
        terminal
            .set_mode(libghostty_vt::terminal::Mode::SGR_MOUSE.into(), true)
            .expect("enable sgr mouse mode");
        terminal
            .set_mode(libghostty_vt::terminal::Mode::NORMAL_MOUSE.into(), true)
            .expect("enable normal mouse mode");
        terminal
            .set_mode(libghostty_vt::terminal::Mode::ALT_ESC_PREFIX.into(), true)
            .expect("enable alt esc prefix mode");

        let options = terminal.input_options().expect("terminal input options");
        assert!(options.cursor_key_application);
        assert!(options.alt_esc_prefix);
        assert_eq!(options.mouse_tracking_mode, MouseTrackingMode::Normal);
        assert_eq!(options.mouse_format, MouseFormat::Sgr);
    }

    #[test]
    fn key_mouse_and_focus_encoding_return_bytes() {
        let mut key_encoder = KeyEncoder::new().expect("create key encoder");
        key_encoder.set_kitty_flags(0x1f);

        let mut key_event = KeyEvent::new().expect("create key event");
        key_event.set_action(KeyAction::Press);
        key_event.set_key(KEY_C);
        key_event.set_mods(MODIFIER_CONTROL);
        key_encoder.encode(&key_event).expect("encode key event");

        let mut mouse_encoder = MouseEncoder::new().expect("create mouse encoder");
        mouse_encoder.set_tracking_mode(MouseTrackingMode::Normal);
        mouse_encoder.set_format(MouseFormat::Sgr);
        mouse_encoder.set_size(MouseEncoderSize {
            screen_width: 800,
            screen_height: 600,
            cell_width: 10,
            cell_height: 20,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        });

        let mut mouse_event = MouseEvent::new().expect("create mouse event");
        mouse_event.set_action(MouseAction::Press);
        mouse_event.set_button(Some(MouseButton::Left));
        mouse_event.set_position(50.0, 40.0);
        let mouse_bytes = mouse_encoder
            .encode(&mouse_event)
            .expect("encode mouse event");
        assert!(!mouse_bytes.is_empty());

        let focus_in = encode_focus(FocusEvent::Gained).expect("encode focus in");
        let focus_out = encode_focus(FocusEvent::Lost).expect("encode focus out");
        assert_eq!(focus_in, b"\x1b[I".to_vec());
        assert_eq!(focus_out, b"\x1b[O".to_vec());
    }

    #[test]
    fn key_from_name_handles_shifted_ascii_variants() {
        assert_eq!(key_from_name("A"), Some(KEY_A));
        assert_eq!(key_from_name("@"), Some(KEY_DIGIT_0 + 2));
        assert_eq!(key_from_name("_"), Some(KEY_MINUS));
        assert_eq!(key_from_name("?"), Some(KEY_SLASH));
        assert_eq!(key_from_name("~"), Some(super::KEY_BACKQUOTE));
        assert_eq!(key_from_name("up"), Some(KEY_ARROW_UP));
        assert_eq!(key_from_name("bogus-key"), None);
        assert_eq!(key_from_name("é"), None);
        assert_eq!(KEY_UNIDENTIFIED, 0);
    }
}
