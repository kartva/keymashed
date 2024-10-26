const std = @import("std");

const ma = @cImport({
    @cInclude("miniaudio/miniaudio.h");
});

const bpf = @cImport({
    @cInclude("bpf/bpf_api.h");
});

const MAError = error{
    FailedToInitializeCaptureDevice,
    FailedToStartDevice,
};

var device: ma.ma_device = undefined;
var deviceConfig: ma.ma_device_config = undefined;
pub var delay: usize = 0;

fn calculate_delay_ns(v: f32) usize {
    // v is from 0 to 1 according to miniaudio
    // Delay will be from 100ms (no sound) to 0ms (100% sound)
    // We use a polynomial curve so that the impacts of making noise are more clear
    // f(x) = 100_000_000 * (1 - x)^3.15
    return @intFromFloat(100_000_000 * std.math.pow(f32, 1 - v, 3.15));
}

// Find the average sound level in an array
fn root_mean_square(buf: []f32) f32 {
    var sum: f32 = 0;
    for (buf) |v| {
        sum += v * v;
    }
    return std.math.sqrt(sum / @as(f32, @floatFromInt(buf.len)));
}

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
    delay = calculate_delay_ns(root_mean_square(i[0..frameCount]));
    std.debug.print("delay: {}ms  \r", .{delay / 1_000_000});

    // update the bpf map
    const attrs = .{
        .pathname = "a",
        .bpf_fd = 0, // unused
        .file_flags = 2, // read write
        .path_fd = 0, // unused
    };
    bpf.sys_bpf(bpf.BPF_OBJ_GET, &attrs, @sizeOf(attrs));
}

pub fn init() MAError!void {
    deviceConfig = ma.ma_device_config_init(ma.ma_device_type_capture);
    deviceConfig.dataCallback = data_callback; // When the mic has data, it'll call this function
    deviceConfig.capture.format = ma.ma_format_f32; // Returns data in [0.0, 1.0] range
    deviceConfig.capture.channels = 2; // Very overkill
    deviceConfig.sampleRate = 48000; // Very overkill

    if (ma.ma_device_init(@as(?*ma.ma_context, null), &deviceConfig, &device) != ma.MA_SUCCESS) {
        return MAError.FailedToInitializeCaptureDevice;
    }
    errdefer ma.ma_device_uninit(&device);
    // We don't really need to actually clean up the device because it'll only ever get closed when the program stops
    // The best garbage collector

    if (ma.ma_device_start(&device) != ma.MA_SUCCESS) {
        return MAError.FailedToStartDevice;
    }
}
