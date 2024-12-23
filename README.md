_Ya know, how sometimes your computer's internet is slow?_

_What if... you could motivate it. Make the internet itself flow a lil' quicker._

# Keymashed

  <img align="right" src="https://github.com/user-attachments/assets/27412e69-7cbc-4a01-9383-3a5e2ed242dd" style="width:125px;">
  An interactive installation at <a href="https://burst.purduehackers.com/">Purdue Hackers' BURST</a>. Since making the internet faster is a hard research problem, <code>keymashed</code> instead settles for worsening the internet and then easing up on the impairment based on how fast you mash the keyboard. Observe the effects of your encouragement through a bad video protocol made for your enjoyment. Mash a variety of keys for best effect.

## Gallery

<p float="left">
  <img src="media/BURST 2024 SebMur-6-resized.jpg" width="49%" /> 
  <img src="media/BURST 2024 SebMur-19-resized.jpg" width="49%" />
  <img src="media/BURST 2024 SebMur-82-cropped.jpg" width="49%" />
  <img src="media/BURST 2024 SebMur-66-cropped.jpg" width="49%" />
</p>

https://github.com/user-attachments/assets/f13cbadf-bcb7-433d-a5de-5e4c0cf470ff

## The Exhibit

Keymashed as an exhibit consisted of:
- An IBM Model-M keyboard with exquisite mash-feel.
- An old square monitor.
- Two Dell Optiplexes (cheap desktop computers) that are connected to the monitor and webcam. They communicate with each other over the internet.

There are two effects at play:
- UDP packets are being dropped on the livestream playing computer at the network interface level. The more keys you mash, the less packets are lost. At the threshold, packet loss stops occurring.
- Frames are being encoded lossily on the livestream sender computer. The more keys you mash, the lower the lossy compression. At the threshold, the image becomes clear without any color banding.

The livestream is delayed by 30 seconds, since it's more interesting to see a bit into the past rather than just looking at your own back.

All of this combines to create the:

## ✨the keymashed experience✨:
_You walk up to the exhibit. There's a keyboard in front of you. The pedestal says, "Mash the keyboard". There are indistinct splotches of grey on the screen that may or may not be people standing around. As you start mashing, the image gains quality and smoothness. The edges of the screen glow a bright green to indicate you're close to the peak. The image resolves into a birds-eye view of the pedestal. In the screen, you see yourself starting to approach the exhibit._

The webcam is mounted on top of a wall along with an Optiplex with a wireless dongle. This is the sender computer. The receiver computer sits under the pedestal that holds the monitor.
<img src="media/BURST 2024 suspiciously-optiplex shaped box.jpg" />

## Technical Details (and repository map)

<TODO: This section is still under construction>

The repository consists of the following components:
- an eBPF filter written in C that drops packets with some probability that it reads from a shared map. This eBPF filter is installed onto the a network interface using the `tc` utility.
- a video codec which uses a JPEG-like scheme to lossily compress blocks of frames which are then reassembled and decompressed on the receiver. The quality of the JPEG encoding can vary per block.
- an RTP-like protocol for receiving packets over UDP.

### eBPF component

[eBPF](https://ebpf.io/) is a relatively recent feature in the Linux kernel which allows running sandboxed user-provided code in the kernel inside a virtual machine. It is used in [many kernel subsystems which deal with security, tracing and networking](https://docs.ebpf.io/linux/program-type/).

We create an eBPF filter in [bpf.c](bpf/bpf.c) which reads the drop probability from a file which user programs can write to and then decides whether to drop the current packet or not. This eBPF filter is installed at a network interface using the `tc` (traffic control) utility.

```c
struct {
  // declare that the bpf map will be of type array, mapping uint32_t to uint32_t and have a maximum of one entry.
  __uint(type, BPF_MAP_TYPE_ARRAY);
  __uint(key_size, sizeof(uint32_t)); 
  __uint(value_size, sizeof(uint32_t));
  __uint(max_entries, 1);
  // PIN_BY_NAME ensures that the map is pinned in /sys/fs/bpf
  __uint(pinning, LIBBPF_PIN_BY_NAME);
  // synchronize the `map_keymash` name with the userspace program
} map_mash __section(".maps");

__section("classifier")
int scream_bpf(struct __sk_buff *skb)
{
  uint32_t key = 0, *val = 0;

  val = map_lookup_elem(&map_mash, &key);
  if (val && get_prandom_u32() < *val) {
    return TC_ACT_SHOT; // Drop packet
  }
  return TC_ACT_OK; // Pass packet
}
```

The userspace code interacts with the eBPF filter using the `bpf_obj_get` and `bpf_map_update_elem` functions from `libbpf`.

### Real-time UDP streaming
I decided to re-invent the Real-time protocol from scratch, with a focus on reduci90ng copies as much as possible. It makes heavy use of the `zerocopy` crate. The result is some rather complex Rust code that I'm quite happy with.

### Video Codec

The webcam transmits video in the `YUV422` format. The [`YUV`](https://en.wikipedia.org/wiki/YCbCr) format is an alternative to the more well-known `RGB` format; it encodes the luminance (`Y`), blue-difference chroma (`Cb`/`U`) and red-difference chroma (`Cr`/`V`).

![A group of pixels 2 tall and 4 wide.](media/YUV444.drawio.svg)

The `422` refers the [chroma subsamping](https://en.wikipedia.org/wiki/Chroma_subsampling), explained below.

![](media/YUV422.drawio.svg)

After receiving the video from the webcam, the video sender further subsamples the colors into 4:2:0.

![](media/YUV420.drawio.svg)

The subsampled frame is then broken into _macroblocks_ of 16 x 16 pixels which contain six _blocks_ of 8 x 8 values: four for luminance, one for red-difference and one for blue-difference. (Note that a group of four pixels has six associated values).

![](media/Macroblock.drawio.svg)

The macroblock.

![](media/MacroblockExpanded.drawio.svg)

The macroblock, decomposed into its six constituent blocks.

Each block is encoded using the [DCT transform](https://en.wikipedia.org/wiki/Discrete_cosine_transform).

After the transformation, the values are divided element-wise by the _quantization matrix_, which is specially chosen to minimize perceptual quality loss.

Finally, the quantized block is run-length encoded in a zig-zag pattern. This causes zero values to end up at the end, which makes our naive encoding quite efficient on its own.

![](media/Zigzag.drawio.svg)

Encoded macroblocks are inserted into a packet with the following metadata and then sent over the network.
```
|---------------|
|  Frame no.    |
|---------------|
| Block 1       |
| x, y, quality |
| RLE data      |
|---------------|
| Block 2       |
| x, y, quality |
| RLE data      |
|---------------|
|      ...      |
```

### User-level application
The application itself uses `SDL2` for handling key input and rendering the video.

## Project Evolution

"what if you could scream at your computer to make it run faster?" was the original question I asked. We (@kartva and @9p4) wrote `run-louder`/`screamd` (we went through many names) which would spawn a child process, say Google Chrome, and intercept all syscalls made by it using `ptrace` (the same syscall that `gdb` uses). After intercepting a syscall, the parent would sleep for some time (proportional to scream intensity) before resuming the child.

We demoed it and have a shaky video of:
- trying to open Chrome but it's stuck loading
- coming up to the laptop and yelling at it
- Chrome immediately loads

As an extension to this idea, I started working on affecting the network as well by dropping packets. At this point, I decided to present `run-louder`/`screamd` at BURST, which necessitated changing screaming to key-mashing (out of respect for the art gallery setting). Additionally, while `ping` works fine as a method of demoing packet loss, I wanted something more visual and thus ended up writing the video codec.

# About the author / hire me!
_I'm looking for Summer 2025 internships._ Read more about my work at [my Github profile](https://github.com/kartva/).

# Credits

@9p4 helped a lot with initial ideation and prototyping.
Poster design by Rebecca Pine and pixel art by Jadden Picardal.
Most photos by Sebastian Murariu.
