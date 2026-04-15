use crate::{Result, Terminal};

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
        terminal: Terminal,
        needle: *const u8,
        needle_len: usize,
        out_matches: *mut GhosttyTerminalSearchMatch,
        out_matches_len: usize,
        out_len: *mut usize,
    ) -> Result::Type;

    pub fn ghostty_terminal_selection_string(
        terminal: Terminal,
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
    ) -> Result::Type;
}
