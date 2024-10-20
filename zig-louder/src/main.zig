const std = @import("std");
const procmgr = @import("procmgr.zig");
const mic = @import("mic.zig");

pub fn main() !u8 {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const args = try std.process.argsAlloc(arena.allocator());
    if (args.len < 2) {
        std.debug.print("Usage: {s} program [program_arguments]\n", .{args[0]});
        return 1;
    }

    // Fork, and in the child process, mark the process as traceable by the parent process
    const pid = try std.posix.fork();

    if (pid < 0) {
        std.debug.print("Fork failed!\n", .{});
        return 1;
    } else if (pid == 0) {
        return procmgr.spawnTrace(&arena, args[1..], pid);
    }

    // If we're in the parent process, start intercepting syscalls and inserting delay
    try mic.init();
    var cm = procmgr.ChildManager{ .arena = &arena, .childPid = pid };
    try cm.childInterceptSyscalls();
    return 0;
}
