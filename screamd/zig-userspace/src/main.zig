const std = @import("std");
const Instant = std.time.Instant;

const ev = @cImport({
    @cInclude("libevdev-1.0/libevdev/libevdev.h");
});

const errno = @cImport({
    @cInclude("errno.h");
});

const c = @cImport({
    @cInclude("stdio.h");
    @cInclude("stdlib.h");
    @cInclude("string.h");
    @cInclude("errno.h");
    @cInclude("unistd.h");
    @cInclude("fcntl.h");
    @cInclude("sys/ioctl.h");
    @cInclude("linux/input.h");
    @cInclude("linux/uinput.h");
    @cInclude("sys/types.h");
    @cInclude("sys/stat.h");
    @cInclude("sys/syscall.h");
    @cInclude("sys/mman.h");
});

const bpf = @cImport({
    @cInclude("libbpf/src/bpf.h");
});

const MAError = error{ FailedToOpenBPFMap, UnableToSetFd, FileOpenError };

// return how many packets out of u32 max should be dropped
fn calculate_drop_amt(v: f32) u32 {
    // v is from 0 to 1 according to miniaudio
    // worst case should be 50% packet loss
    // We use a polynomial curve so that the impacts of making noise are more clear
    // f(x) = 10 * (1 - x)^3.15
    return @intFromFloat((std.math.maxInt(u32) / 2) * std.math.pow(f64, 1 - v, 3.15));
}

// Find the average sound level in an array
fn root_mean_square(buf: []f32) f32 {
    var sum: f32 = 0;
    for (buf) |v| {
        sum += v * v;
    }
    return std.math.sqrt(sum / @as(f32, @floatFromInt(buf.len)));
}

var ns: f64 = 0.0;

fn observe_delay() !void {
    const path = "/dev/input/mouse2";
    const bytes_per_read = 3;
    const f = std.fs.openFileAbsolute(path, .{ .mode = .read_only }) catch |err| {
        std.debug.print("Failed to open device file: {s}\n", .{@errorName(err)});
        return MAError.FileOpenError;
    };
    defer f.close();

    while (true) {
        const start = try Instant.now();
        var b: [bytes_per_read]u8 = undefined;
        _ = try f.read(&b);
        const end = try Instant.now();
        ns = @floatFromInt(end.since(start));
    }
}

var bpf_map_fd: c_int = -1;

pub fn main() !u8 {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    _ = try std.Thread.spawn(.{}, observe_delay, .{});

    while (true) {
        std.log.debug("Time elapsed is: {d:.3}ms\n", .{
            ns / std.time.ns_per_ms,
        });
        std.time.sleep(100 * std.time.ns_per_ms);
    }
    return 0;
}
