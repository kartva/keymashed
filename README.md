_Ya know, how sometimes your computer's internet is slow?_

_What if... you could motivate it. Make the internet itself flow a lil' quicker._

## Keymashed

An interactive installation set to debut at [Purdue Hacker's BURST](https://burst.purduehackers.com/). Since making the internet faster is a hard research problem, `keymashed` instead settles for slowing down the internet and then easing up on the impairment based on how many keys you mash.

Keymashed consists of the following components:
- an eBPF filter written in C that drops packets with some probability that it reads from a shared map. This eBPF filter is installed onto the a network interface using the `tc` utility.
- a typing test written with `ratatui` which communicates WPM results to the shared eBPF map
- an in-progress implementation of an RTP-like protocol to showcase the effects of packet loss
- an imaginary video codec that causes cool visual artifacts on packet loss (hopefully becomes real before the deadline)

Consult the READMEs in the directories for more details on each component.