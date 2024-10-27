const std = @import("std");

const ma = @cImport({
    @cInclude("miniaudio/miniaudio.h");
});

const bpf = @cImport({
    @cInclude("libbpf/src/bpf.h");
});

const MAError = error{
    FailedToInitializeContext,
    FailedToInitializeCaptureDevice,
    FailedToGetDevices,
    FailedToStartDevice,
    FailedToOpenBPFMap,
};

var device: ma.ma_device = undefined;
var ma_context: ma.ma_context = undefined;
var deviceConfig: ma.ma_device_config = undefined;
pub var drop_amt: u32 = 0;

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

fn data_callback(
    pDevice: ?*anyopaque,
    pOutput: ?*anyopaque,
    pInput: ?*const anyopaque,
    frameCount: ma.ma_uint32,
) callconv(.C) void {
    _ = pDevice;
    _ = pOutput;

    // The sound library we're using gives us back a void pointer
    // We're casting it from a const void pointer to a non-const pointer to an array of floats
    // We know it's an array of f32s because that's the setting we set for it in the initialization function
    const i = @as([*]f32, @constCast(@ptrCast(@alignCast(pInput.?))));
    drop_amt = calculate_drop_amt(root_mean_square(i[0..frameCount]));
    std.debug.print("drop ratio: {}%  \r", .{(drop_amt / std.math.maxInt(@TypeOf(drop_amt))) * 100});

    const key: u32 = 0;

    const err = bpf.bpf_map_update_elem(bpf_map_fd, &key, &drop_amt, bpf.BPF_ANY);
    if (err != 0) {
        std.debug.print("Failed to update BPF map with err: {}\n", .{err});
    }
}

pub fn init() MAError!void {
    const map_path = "/sys/fs/bpf/tc/globals/map_scream";
    std.log.debug("opening ebpf map at {s}", .{map_path});
    bpf_map_fd = bpf.bpf_obj_get(map_path);
    if (bpf_map_fd < 0) {
        std.debug.print("Failed to get BPF map with err: {}\n", .{bpf_map_fd});
        return MAError.FailedToOpenBPFMap;
    }

    if (ma.ma_context_init(null, 0, null, &ma_context) != ma.MA_SUCCESS) {
        return MAError.FailedToInitializeContext;
    }

    var pCaptureInfos: [*c]ma.ma_device_info = undefined;
    var captureCount: ma.ma_uint32 = undefined;

    if (ma.ma_context_get_devices(&ma_context, null, null, &pCaptureInfos, &captureCount) != ma.MA_SUCCESS) {
        return MAError.FailedToGetDevices;
    }

    for (0..captureCount) |iDevice| {
        std.debug.print("{} - {s}\n", .{ iDevice, pCaptureInfos[iDevice].name });
    }

    deviceConfig = ma.ma_device_config_init(ma.ma_device_type_capture);
    deviceConfig.dataCallback = data_callback; // When the mic has data, it'll call this function
    deviceConfig.capture.format = ma.ma_format_f32; // Returns data in [0.0, 1.0] range
    deviceConfig.capture.channels = 2; // Very overkill
    deviceConfig.sampleRate = 44100; // Very overkill
    deviceConfig.playback.pDeviceID = &pCaptureInfos[10].id; // replace 10 with the device you want to use

    std.log.debug("trying to init mic", .{});

    if (ma.ma_device_init(@as(?*ma.ma_context, null), &deviceConfig, &device) != ma.MA_SUCCESS) {
        return MAError.FailedToInitializeCaptureDevice;
    }

    errdefer ma.ma_device_uninit(&device);
    errdefer _ = ma.ma_context_uninit(&ma_context);
    // We don't really need to actually clean up the device because it'll only ever get closed when the program stops
    // The best garbage collector

    std.log.debug("trying to start mic", .{});

    if (ma.ma_device_start(&device) != ma.MA_SUCCESS) {
        return MAError.FailedToStartDevice;
    }

    std.log.debug("started mic", .{});
}
