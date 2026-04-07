#![warn(clippy::pedantic)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(clippy::allow_attributes)]
#![warn(clippy::allow_attributes_without_reason)]

//! Thin Zed-specific adapter helpers built on top of `libghostty-vt`.

use std::{cell::RefCell, rc::Rc};

use libghostty_vt::{
    RenderState as WrapperRenderState, Terminal,
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

/// Build shared terminal input options from a wrapper terminal.
pub fn terminal_input_options(terminal: &Terminal<'_, '_>) -> Result<TerminalInputOptions> {
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

/// Build a compatibility cursor summary from a wrapper terminal.
pub fn cursor_state(terminal: &Terminal<'_, '_>) -> Result<CursorState> {
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
pub fn scrollbar_state(terminal: &Terminal<'_, '_>) -> Result<ScrollbarState> {
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
    pub fn update(&mut self, terminal: &Terminal<'static, '_>) -> Result<RenderSnapshot> {
        let snapshot = self.inner.update(terminal)?;
        Self::adapt_snapshot(&snapshot)
    }

    /// Alias for [`Self::update`].
    pub fn snapshot(&mut self, terminal: &Terminal<'static, '_>) -> Result<RenderSnapshot> {
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

fn mouse_tracking_mode_for_terminal(terminal: &Terminal<'_, '_>) -> Result<MouseTrackingMode> {
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

fn mouse_format_for_terminal(terminal: &Terminal<'_, '_>) -> Result<MouseFormat> {
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
        terminal: &mut Terminal<'static, 'static>,
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

fn color_scheme_for_terminal(terminal: &Terminal<'_, '_>) -> Option<ColorScheme> {
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
        RenderDirty, RenderState, RowSemanticPrompt, RuntimeEffect, Screen, TerminalCallbacks,
        cursor_state, encode_focus, grid_cell, grid_row, key_from_name, scrollbar_state,
        terminal_input_options,
    };
    use libghostty_vt::{
        Terminal, TerminalOptions,
        terminal::{Point, PointCoordinate},
    };

    fn install_callbacks() -> (Box<Terminal<'static, 'static>>, TerminalCallbacks) {
        let mut terminal = Box::new(
            Terminal::new(TerminalOptions {
                cols: 20,
                rows: 4,
                max_scrollback: 128,
            })
            .expect("create terminal"),
        );
        let callbacks = TerminalCallbacks::new(20, 4);
        callbacks
            .install(&mut terminal, InstallOptions { xtversion: "Zed" })
            .expect("install callbacks");
        (terminal, callbacks)
    }

    #[test]
    fn callbacks_queue_runtime_effects() {
        let (mut terminal, callbacks) = install_callbacks();

        terminal.vt_write(b"\x07");
        assert_eq!(callbacks.drain_effects(), vec![RuntimeEffect::Bell]);
        assert!(callbacks.drain_effects().is_empty());

        terminal.vt_write(b"\x1b]2;zed title\x07");
        assert_eq!(
            callbacks.drain_effects(),
            vec![RuntimeEffect::TitleChanged(Some("zed title".to_owned()))]
        );

        terminal.vt_write(b"\x1b]2;\x07");
        assert_eq!(
            callbacks.drain_effects(),
            vec![RuntimeEffect::TitleChanged(None)]
        );
    }

    #[test]
    fn callbacks_queue_pty_replies() {
        let (mut terminal, callbacks) = install_callbacks();

        terminal.vt_write(b"\x1b[>q");
        let replies = callbacks.drain_pty_writes();
        assert_eq!(replies.len(), 1, "expected one XTVERSION reply");
        let reply = String::from_utf8(replies.into_iter().next().expect("reply"))
            .expect("XTVERSION reply should be valid UTF-8");
        assert!(
            reply.contains("Zed"),
            "expected XTVERSION reply to contain Zed, got {reply:?}"
        );

        terminal.vt_write(b"\x05");
        assert!(
            callbacks.drain_pty_writes().is_empty(),
            "expected ENQ callback to stay silent"
        );
    }

    #[test]
    fn eager_render_snapshot_includes_text() {
        let (mut terminal, _) = install_callbacks();
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
        let (terminal, _) = install_callbacks();

        let cursor = cursor_state(&terminal).expect("cursor state");
        let scrollbar = scrollbar_state(&terminal).expect("scrollbar state");

        assert_eq!(cursor.column, 0);
        assert_eq!(cursor.row, 0);
        assert!(cursor.visible);
        assert_eq!(cursor.active_screen, Screen::Primary);
        assert_eq!(scrollbar.offset, 0);
        assert!(scrollbar.total >= scrollbar.len);
    }

    #[test]
    fn grid_summaries_match_wrapped_prompt_content() {
        let (mut terminal, _) = install_callbacks();
        terminal.vt_write(b"prompt\r\n");

        let grid_ref = terminal
            .grid_ref(Point::Active(PointCoordinate::new(0, 0)))
            .expect("grid ref");
        let row = grid_row(&grid_ref).expect("grid row");
        let cell = grid_cell(&grid_ref).expect("grid cell");

        assert!(!row.wrap_continuation);
        assert_eq!(row.semantic_prompt, RowSemanticPrompt::None);
        assert_eq!(cell.codepoint, Some(u32::from('p')));
    }

    #[test]
    fn terminal_input_options_follow_terminal_modes() {
        let (mut terminal, _) = install_callbacks();
        terminal
            .set_mode(libghostty_vt::terminal::Mode::DECCKM, true)
            .expect("enable app cursor mode");
        terminal
            .set_mode(libghostty_vt::terminal::Mode::SGR_MOUSE, true)
            .expect("enable sgr mouse mode");
        terminal
            .set_mode(libghostty_vt::terminal::Mode::NORMAL_MOUSE, true)
            .expect("enable normal mouse mode");
        terminal
            .set_mode(libghostty_vt::terminal::Mode::ALT_ESC_PREFIX, true)
            .expect("enable alt esc prefix mode");

        let options = terminal_input_options(&terminal).expect("terminal input options");
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
