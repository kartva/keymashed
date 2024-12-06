This Rust project requires nightly Rust to compile.

Bump up the net send buffer when running this program, otherwise UDP packets tend to get dropped.

```
sudo sysctl -w net.core.rmem_default=8388608
```