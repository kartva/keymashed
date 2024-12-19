_Ya know, how sometimes your computer's internet is slow?_

_What if... you could motivate it. Make the internet itself flow a lil' quicker._

## Keymashed

An interactive installation at [Purdue Hacker's BURST](https://burst.purduehackers.com/). Since making the internet faster is a hard research problem, `keymashed` instead settles for slowing down the internet and then easing up on the impairment based on how many keys you mash. Observe the effects of your encouragement through a bad video protocol made for your enjoyment. Mash a variety of keys for best effect.

https://github.com/user-attachments/assets/f13cbadf-bcb7-433d-a5de-5e4c0cf470ff

<p float="left">
  <img src="media/BURST 2024 SebMur-6-resized.jpg" width="49%" /> 
  <img src="media/BURST 2024 SebMur-19-resized.jpg" width="49%" />
  <img src="media/BURST 2024 SebMur-82-cropped.jpg" width="49%" />
  <img src="media/BURST 2024 SebMur-66-cropped.jpg" width="49%" />
</p>

## The Exhibit

Keymashed as an exhibit consisted of:
- An IBM Model-M keyboard with exquisite mash-feel.
- An old square monitor.
- Two Dell Optiplexes (cheap desktop computers) that are connected to the monitor and webcam. They communicate with each other over the internet.

There are two effects at play:
- UDP packets are being lost on the livestream playing computer at the network interface level. The more keys you mash, the less packets are lost. At the threshold, packet loss stops occurring.
- Frames are being encoded lossily on the livestream sender computer. The more keys you mash, the lower the lossy compression. At the threshold, the image becomes clear without any color banding.

The livestream is delayed by 30 seconds, since it's more interesting to see a bit into the past rather than just looking at your own back.

All of this combines to create the
## ✨magic keymashed experience✨:
_You walk up to the exhibit. There's a keyboard in front of you. The pedestal says, "Mash the keyboard". There are indistinct splotches of grey on the screen that may or may not be people standing around. As you start mashing, the image changes color and gains quality. The edges of the screen glow a bright green to indicate you're close to the peak. The image resolves into... not you. In the screen, you see yourself starting to approach the exhibit._

The webcam is mounted on top of a wall along with an Optiplex with a wireless dongle. This is the sender computer. The receiver computer sits under the pedestal that holds the monitor.
<img src="media/BURST 2024 suspiciously-optiplex shaped box.jpg" />

## Technical Details (and repository map)

<TODO: This section is still under construction>

The repository consists of the following components:
- an eBPF filter written in C that drops packets with some probability that it reads from a shared map. This eBPF filter is installed onto the a network interface using the `tc` utility.
- a video codec which uses a JPEG-like scheme to lossily compress blocks of frames which are then reassembled and decompressed on the receiver. The quality of the JPEG encoding can vary per block.
- an RTP-like protocol for receiving packets over UDP.

Consult the READMEs in the directories for more details on each component.

<p align="center">
  <img src="https://github.com/user-attachments/assets/27412e69-7cbc-4a01-9383-3a5e2ed242dd" style="height: 40%; width: 40%;" />
  <br>
   Poster design by Rebecca Pine and pixel art by Jadden Picardal.
</p>
