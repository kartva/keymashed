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

        exe.linkLibC();
        exe.addIncludePath(b.path("src/include"));

        exe.addLibraryPath(b.path("src/include/libbpf/src/"));
        exe.linkSystemLibrary("bpf");
        exe.linkSystemLibrary("evdev");

        b.installArtifact(exe);
    }
}
