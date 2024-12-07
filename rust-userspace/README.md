This Rust project requires nightly Rust to compile.

Bump up the net send buffer when running this program, otherwise UDP packets tend to get dropped.

```bash
sudo sysctl -w net.core.rmem_default=8388608
# bumping up the stack size limit may also be helpful if the program crashes
ulimit -s 8388608
```