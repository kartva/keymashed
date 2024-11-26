#!/bin/bash

sudo tc qdisc add dev wlp3s0 ingress
sudo tc qdisc add dev wlp3s0 root handle 1: prio
sudo tc filter add dev wlp3s0 ingress bpf da obj bpf.o sec classifier
sudo tc filter add dev wlp3s0 protocol all parent 1: prio 1 bpf da obj bpf.o sec classifier