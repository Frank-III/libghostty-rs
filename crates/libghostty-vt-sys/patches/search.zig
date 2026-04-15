const std = @import("std");
const Search = @import("../search.zig");
const Selection = @import("../Selection.zig");
const page = @import("../page.zig");
const point = @import("../point.zig");
const terminal_c = @import("terminal.zig");
const Result = @import("result.zig").Result;
const size = @import("../size.zig");

// This patch is copied into Ghostty's checkout at build time so downstream
// consumers can rely on the search/selection C hooks until upstream exports them from:
// - src/terminal/c/main.zig
// - src/lib_vt.zig

/// C: GhosttyTerminalSearchMatch
pub const SearchMatch = extern struct {
    start_x: size.CellCountInt,
    start_y: u32,
    end_x: size.CellCountInt,
    end_y: u32,
};

pub fn terminal_search_matches(
    terminal_: terminal_c.Terminal,
    needle_ptr: [*]const u8,
    needle_len: usize,
    out_buf: ?[*]SearchMatch,
    out_buf_len: usize,
    out_len: *usize,
) callconv(.c) Result {
    const terminal_handle = terminal_ orelse return .invalid_value;
    const terminal = if (comptime @hasField(@TypeOf(terminal_handle.*), "terminal"))
        terminal_handle.terminal
    else
        terminal_handle;
    if (needle_len == 0) {
        out_len.* = 0;
        return .invalid_value;
    }

    const alloc = terminal.gpa();
    var search: Search.Screen = Search.Screen.init(
        alloc,
        terminal.screens.active,
        needle_ptr[0..needle_len],
    ) catch return .out_of_memory;
    defer search.deinit();

    search.searchAll() catch return .out_of_memory;

    const flattened = search.matches(alloc) catch return .out_of_memory;
    defer alloc.free(flattened);

    var needed: usize = 0;
    for (flattened) |highlight| {
        const untracked = highlight.untracked();
        const start = terminal.screens.active.pages.pointFromPin(.screen, untracked.start) orelse continue;
        const end = terminal.screens.active.pages.pointFromPin(.screen, untracked.end) orelse continue;
        _ = start;
        _ = end;
        needed += 1;
    }

    out_len.* = needed;
    if (out_buf == null or out_buf_len < needed) return .out_of_space;

    var index: usize = 0;
    for (flattened) |highlight| {
        const untracked = highlight.untracked();
        const start = terminal.screens.active.pages.pointFromPin(.screen, untracked.start) orelse continue;
        const end = terminal.screens.active.pages.pointFromPin(.screen, untracked.end) orelse continue;

        switch (start) {
            .screen => |start_coord| switch (end) {
                .screen => |end_coord| {
                    out_buf.?[index] = .{
                        .start_x = start_coord.x,
                        .start_y = start_coord.y,
                        .end_x = end_coord.x,
                        .end_y = end_coord.y,
                    };
                    index += 1;
                },
                else => {},
            },
            else => {},
        }
    }

    out_len.* = index;
    return .success;
}

pub fn terminal_selection_string(
    terminal_: terminal_c.Terminal,
    start_active: bool,
    start_x: size.CellCountInt,
    start_y: u32,
    end_active: bool,
    end_x: size.CellCountInt,
    end_y: u32,
    rectangle: bool,
    trim: bool,
    out_buf: ?[*]u8,
    out_buf_len: usize,
    out_len: *usize,
) callconv(.c) Result {
    const terminal_handle = terminal_ orelse return .invalid_value;
    const terminal = if (comptime @hasField(@TypeOf(terminal_handle.*), "terminal"))
        terminal_handle.terminal
    else
        terminal_handle;
    const start_point: point.Point = if (start_active)
        .{ .active = .{ .x = start_x, .y = start_y } }
    else
        .{ .screen = .{ .x = start_x, .y = start_y } };
    const end_point: point.Point = if (end_active)
        .{ .active = .{ .x = end_x, .y = end_y } }
    else
        .{ .screen = .{ .x = end_x, .y = end_y } };

    const start_pin = terminal.screens.active.pages.pin(start_point) orelse return .invalid_value;
    const end_pin = terminal.screens.active.pages.pin(end_point) orelse return .invalid_value;

    const selection = Selection.init(start_pin, end_pin, rectangle);
    const alloc = terminal.gpa();
    const text = terminal.screens.active.selectionString(alloc, .{
        .sel = selection,
        .trim = trim,
    }) catch return .out_of_memory;
    defer alloc.free(text);

    out_len.* = text.len;
    if (out_buf == null or out_buf_len < text.len) return .out_of_space;

    @memcpy(out_buf.?[0..text.len], text[0..text.len]);
    return .success;
}
