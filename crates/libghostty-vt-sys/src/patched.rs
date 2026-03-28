use crate::{GhosttyResult, GhosttyTerminal_ptr};

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GhosttyTerminalSearchMatch {
    pub start_x: u16,
    pub start_y: u32,
    pub end_x: u16,
    pub end_y: u32,
}

unsafe extern "C" {
    pub fn ghostty_terminal_search_matches(
        terminal: GhosttyTerminal_ptr,
        needle: *const u8,
        needle_len: usize,
        out_matches: *mut GhosttyTerminalSearchMatch,
        out_matches_len: usize,
        out_len: *mut usize,
    ) -> GhosttyResult;

    pub fn ghostty_terminal_selection_string(
        terminal: GhosttyTerminal_ptr,
        start_active: bool,
        start_x: u16,
        start_y: u32,
        end_active: bool,
        end_x: u16,
        end_y: u32,
        rectangle: bool,
        trim: bool,
        out_buf: *mut u8,
        out_buf_len: usize,
        out_len: *mut usize,
    ) -> GhosttyResult;

    pub fn ghostty_terminal_hyperlink_uri_at(
        terminal: GhosttyTerminal_ptr,
        x: u16,
        y: u32,
        out_buf: *mut u8,
        out_buf_len: usize,
        out_len: *mut usize,
    ) -> GhosttyResult;
}
