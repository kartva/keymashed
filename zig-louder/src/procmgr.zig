// Adapted from https://notes.eatonphil.com/2023-10-01-intercepting-and-modifying-linux-system-calls-with-ptrace.html

const std = @import("std");
const mic = @import("mic.zig");
const c = @cImport({
    @cInclude("sys/ptrace.h");
    @cInclude("sys/user.h");
    @cInclude("sys/wait.h");
    @cInclude("errno.h");
});

const cNullPtr: ?*anyopaque = null;

pub fn spawnTrace(arena: *std.heap.ArenaAllocator, args: []const []const u8, childPid: std.posix.pid_t) std.process.ExecvError {
    _ = c.ptrace(c.PTRACE_TRACEME, childPid, cNullPtr, cNullPtr);
    return std.process.execv(arena.allocator(), args);
}

const ProcError = error{
    Ptrace,
};

pub const ChildManager = struct {
    arena: *std.heap.ArenaAllocator,
    childPid: std.posix.pid_t,

    fn getSyscall(cm: ChildManager) !c_ulonglong {
        var regs: c.user_regs_struct = .{};
        if (c.ptrace(c.PTRACE_GETREGS, cm.childPid, cNullPtr, &regs) == -1) {
            return ProcError.Ptrace;
        }
        return regs.orig_rax;
    }

    fn childWaitForSyscall(cm: ChildManager) !i32 {
        var status: i32 = 0;
        if (c.ptrace(c.PTRACE_SYSCALL, cm.childPid, cNullPtr, cNullPtr) == -1) {
            return ProcError.Ptrace;
        }
        _ = c.waitpid(cm.childPid, &status, 0);
        return status;
    }

    const hooks = &[_]struct {
        syscall: c_ulonglong,
        hook: *const fn (*ChildManager) anyerror!void,
    }{
        .{
            .syscall = @intFromEnum(std.os.linux.syscalls.X64.poll),
            .hook = pollHandler,
        },
        .{
            .syscall = @intFromEnum(std.os.linux.syscalls.X64.epoll_wait),
            .hook = pollHandler,
        },
        // Add more syscalls to intercept here...
    };

    pub fn childInterceptSyscalls(
        cm: *ChildManager,
    ) !void {
        while (true) {
            // Handle syscall entrance
            const status = try cm.childWaitForSyscall();
            // Checking status so that our loop stops if the child exits
            if (status == 0) {
                break;
            }

            const address = try cm.getSyscall();

            for (hooks) |hook| {
                if (address == hook.syscall) {
                    try hook.hook(cm);
                }
            }
        }
    }

    fn pollHandler(cm: *ChildManager) anyerror!void {
        std.time.sleep(mic.delay);
        _ = try cm.childWaitForSyscall();
    }
};
