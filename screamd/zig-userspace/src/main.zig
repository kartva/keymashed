const std = @import("std");

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

var bpf_map_fd: c_int = -1;

pub fn main() !u8 {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const map_path = "/sys/fs/bpf/tc/globals/map_scream";
    std.log.debug("opening ebpf map at {s}", .{map_path});
    bpf_map_fd = bpf.bpf_obj_get(map_path);
    if (bpf_map_fd < 0) {
        std.debug.print("Failed to get BPF map with err: {}\n", .{bpf_map_fd});
        return MAError.FailedToOpenBPFMap;
    }

    const device_path = "/dev/input/mouse1"; // Replace with your actual device file
    const fd = std.fs.openFileAbsolute(device_path, .{ .mode = .read_only }) catch |err| {
        std.debug.print("Failed to open device file: {s}\n", .{@errorName(err)});
        return MAError.FileOpenError;
    };
    defer fd.close();

    // Initialize libevdev
    const evdev: ?*ev.struct_libevdev = ev.libevdev_new();
    defer ev.libevdev_free(evdev);

    if (ev.libevdev_set_fd(evdev, fd.handle) != 0) {
        
        return MAError.UnableToSetFd;
    }

    std.debug.print("Device: {s}\n", .{ev.libevdev_get_name(evdev)});

    while (true) {
        var evv: ev.input_event = undefined;
        const res = ev.libevdev_next_event(evdev, ev.LIBEVDEV_READ_FLAG_NORMAL, &evv);

        if (res == c.EAGAIN) {
            continue; // No new event available, non-blocking
        } else if (res < 0) {
            std.debug.print("Error reading event\n", .{});
            break;
        }

        var drop_amt: u32 = 100000;
        std.debug.print("drop ratio: {}%  \r", .{(drop_amt / std.math.maxInt(@TypeOf(drop_amt))) * 100});

        const key: u32 = 0;

        const err = bpf.bpf_map_update_elem(bpf_map_fd, &key, &drop_amt, bpf.BPF_ANY);
        if (err != 0) {
            std.debug.print("Failed to update BPF map with err: {}\n", .{err});
        }
    }

    std.log.debug("started listening", .{});

    return 0;
}
