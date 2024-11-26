#include <libbpf.h>

// #define __uint(name, val) int (*name)[val] // int (*name)[4] -> 16 bytes
// #define __type(name, val) typeof(val) *name
// #define __array(name, val) typeof(val) *name[]

struct {
	__uint(type, BPF_MAP_TYPE_ARRAY);
	__uint(key_size, sizeof(uint32_t));
	__uint(value_size, sizeof(uint32_t));
	__uint(max_entries, 1);
	__uint(pinning, LIBBPF_PIN_BY_NAME);	/* or LIBBPF_PIN_NONE */ // PIN_BY_NAME ensures that the map is pinned in /sys/fs/bpf
} map_scream __section(".maps"); // synchronize this map name with userspace program

__section("classifier")
int scream_bpf(struct __sk_buff *skb)
{
    uint32_t key = 0, *val;

	val = map_lookup_elem(&map_scream, &key);
    int prob_frac = 0;
	if (val)
		prob_frac = *val;

    // Implement probability check here (e.g., a simple random function)
    if (get_prandom_u32() < prob_frac) { // 10% probability
        return TC_ACT_SHOT; // Drop packet
    }
    return TC_ACT_OK; // Pass packet
}

BPF_LICENSE("GPL");