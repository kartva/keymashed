#include "bpf_api.h"

/* Minimal, stand-alone toy map pinning example:
 *
 * clang -target bpf -O2 [...] -o bpf_shared.o -c bpf_shared.c
 * tc filter add dev foo parent 1: bpf obj bpf_shared.o sec egress
 * tc filter add dev foo parent ffff: bpf obj bpf_shared.o sec ingress
 *
 * Both classifier will share the very same map instance in this example,
 * so map content can be accessed from ingress *and* egress side!
 *
 * This example has a pinning of PIN_OBJECT_NS, so it's private and
 * thus shared among various program sections within the object.
 *
 * A setting of PIN_GLOBAL_NS would place it into a global namespace,
 * so that it can be shared among different object files. A setting
 * of PIN_NONE (= 0) means no sharing, so each tc invocation a new map
 * instance is being created.
 */

struct {
    // declare that the bpf map will be of type array, mapping uint32_t to uint32_t and have a maximum of one entry.
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(key_size, sizeof(uint32_t)); 
    __uint(value_size, sizeof(uint32_t));
    __uint(max_entries, 1);
    // PIN_BY_NAME ensures that the map is pinned in /sys/fs/bpf
    __uint(pinning, LIBBPF_PIN_BY_NAME);
    // synchronize the `map_keymash` name with the userspace program
} map_keymash __section(".maps");

__section("classifier")
int scream_bpf(struct __sk_buff *skb)
{
    uint32_t key = 0, *val = 0;

    val = map_lookup_elem(&map_keymash, &key);
    if (val && get_prandom_u32() < *val) {
        return TC_ACT_SHOT; // Drop packet
    }
    return TC_ACT_OK; // Pass packet
}

BPF_LICENSE("GPL");