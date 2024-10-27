Add libbpf libraries:

```
cd screamd/zig-userspace/src/include
git clone https://github.com/libbpf/libbpf.git
cd libbpf
make -j`nproc`
```

[Run `tc` first](screamd/bpf/README.md)

Run userspace program:

```
zig build
sudo ./zig-out/bin/syscall sleep 10
```