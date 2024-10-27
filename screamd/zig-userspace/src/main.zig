const std = @import("std");
// const procmgr = @import("procmgr.zig");
const mic = @import("mic.zig");

pub fn main() !u8 {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const args = try std.process.argsAlloc(arena.allocator());
    if (args.len < 2) {
        std.debug.print("Usage: {s} program [program_arguments]\n", .{args[0]});
        return 1;
    }

    try mic.init();

    _ = std.os.linux.pause();
    return 0;
}
