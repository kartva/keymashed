const std = @import("std");

pub fn main() !u8 {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const args = try std.process.argsAlloc(arena.allocator());
    if (args.len < 2) {
        std.debug.print("Usage: {s} program [program_arguments]\n", .{args[0]});
        return 1;
    }

    _ = std.os.linux.pause();
    return 0;
}
