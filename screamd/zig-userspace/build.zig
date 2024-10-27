const std = @import("std");
const Build = std.Build;

pub fn build(b: *Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    {
        const exe = b.addExecutable(.{
            .name = "syscall",
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        });
        exe.addIncludePath(b.path("src/include"));

        exe.root_module.addCMacro("MA_ENABLE_ONLY_SPECIFIC_BACKENDS", "");
        exe.root_module.addCMacro("MA_ENABLE_ALSA", "");
        exe.addCSourceFile(.{ .file = b.path("src/include/miniaudio/miniaudio.c") });
        exe.addLibraryPath(b.path("src/include/libbpf/src/"));
        exe.linkSystemLibrary("bpf");

        exe.linkLibC();
        exe.linkSystemLibrary("libpipewire-0.3");
        exe.linkSystemLibrary("alsa");
        // exe.linkSystemLibrary("libpulse");

        b.installArtifact(exe);
    }
}
