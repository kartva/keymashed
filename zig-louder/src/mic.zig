const std = @import("std");

const ma = @cImport({
    @cDefine("MINIAUDIO_IMPLEMENTATION", {});
    @cInclude("miniaudio/miniaudio.h");
});

const MAError = error{
    FailedToInitializeCaptureDevice,
    FailedToStartDevice,
};

var device: ma.ma_device = undefined;
var deviceConfig: ma.ma_device_config = undefined;
pub var delay: usize = 0;

fn calculate_delay_ns(v: f32) usize {
    // v is from 0 to 1
    return @intFromFloat(1_00_000_000 * std.math.pow(f32, 1 - v, 5));
}

fn root_mean_square(buf: []f32) f32 {
    var sum: f32 = 0;
    for (buf) |v| {
        sum += v * v;
    }
    return std.math.sqrt(sum / @as(f32, @floatFromInt(buf.len)));
}

fn data_callback(pDevice: ?*anyopaque, pOutput: ?*anyopaque, pInput: ?*const anyopaque, frameCount: ma.ma_uint32) callconv(.C) void {
    _ = pDevice;
    _ = pOutput;
    const in = pInput.?;
    const i = @as([*]f32, @constCast(@ptrCast(@alignCast(in))));
    delay = calculate_delay_ns(root_mean_square(i[0..frameCount]));
    std.debug.print("delay: {}\n", .{delay});
}

pub fn init() MAError!void {
    deviceConfig = ma.ma_device_config_init(ma.ma_device_type_capture);
    deviceConfig.dataCallback = data_callback;
    deviceConfig.capture.format = ma.ma_format_f32;
    deviceConfig.capture.channels = 2;
    deviceConfig.sampleRate = 48000;

    if (ma.ma_device_init(@as(?*ma.ma_context, null), &deviceConfig, &device) != ma.MA_SUCCESS) {
        return MAError.FailedToInitializeCaptureDevice;
    }
    errdefer ma.ma_device_uninit(&device);

    if (ma.ma_device_start(&device) != ma.MA_SUCCESS) {
        return MAError.FailedToStartDevice;
    }
}
