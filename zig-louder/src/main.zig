const std = @import("std");
const procmgr = @import("procmgr.zig");
const mic = @import("mic.zig");

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const args = try std.process.argsAlloc(arena.allocator());
    std.debug.assert(args.len >= 2);

    const pid = try std.posix.fork();

    if (pid < 0) {
        std.debug.print("Fork failed!\n", .{});
        return;
    } else if (pid == 0) {
        return procmgr.spawnTrace(&arena, args[1..], pid);
    }

        try mic.init();

    var cm = procmgr.ChildManager{ .arena = &arena, .childPid = pid };
    try cm.childInterceptSyscalls();
}
