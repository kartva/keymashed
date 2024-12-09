_Ya know, how sometimes your computer's internet is slow?_

_What if... you could motivate it. Make the internet itself flow a lil' quicker._

## Keymashed

An interactive installation at [Purdue Hacker's BURST](https://burst.purduehackers.com/). Since making the internet faster is a hard research problem, `keymashed` instead settles for slowing down the internet and then easing up on the impairment based on how many keys you mash.

https://github.com/user-attachments/assets/f13cbadf-bcb7-433d-a5de-5e4c0cf470ff

Keymashed consists of the following components:
- an eBPF filter written in C that drops packets with some probability that it reads from a shared map. This eBPF filter is installed onto the a network interface using the `tc` utility.
- a video codec which uses a JPEG-like scheme to lossily compress blocks of frames which are then reassembled and decompressed on the receiver. The quality of the JPEG encoding can vary per block.
- an RTP-like protocol to showcase the effects of packet loss.

Consult the READMEs in the directories for more details on each component.
