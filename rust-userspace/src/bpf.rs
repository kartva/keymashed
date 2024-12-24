use std::{
    ffi::CStr,
    os::raw::{c_int, c_void},
};

use libbpf_sys::{bpf_obj_get, BPF_ANY};

const BPF_MAP_NAME: &CStr = c"/sys/fs/bpf/tc/globals/map_keymash";

#[derive(Debug)]
pub struct BpfHandle {
    map_fd: c_int,
}

#[derive(Debug, Clone, Copy)]
pub enum BpfError {
    LoadMap(c_int),
    MapWrite(c_int),
}

/// Opens the eBPF map.
pub unsafe fn init() -> Result<BpfHandle, BpfError> {
    let res = bpf_obj_get(BPF_MAP_NAME.as_ptr());
    if res < 0 {
        log::error!("Failed to load BPF map {BPF_MAP_NAME:?}: {}", res);
        return Err(BpfError::LoadMap(res));
    }
    Ok(BpfHandle { map_fd: res })
}

impl BpfHandle {
    /// Write a key-value pair to the eBPF map.
    pub fn write_to_map(&self, key: u32, value: u32) -> Result<(), BpfError> {
        unsafe {
            let res = libbpf_sys::bpf_map_update_elem(
                self.map_fd,
                &key as *const u32 as *const c_void,
                &value as *const u32 as *const c_void,
                BPF_ANY.into(),
            );
            if res != 0 {
                log::error!("Failed to write to BPF map: {}", res);
                return Err(BpfError::MapWrite(res));
            }
        }
        Ok(())
    }
}

impl Drop for BpfHandle {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.map_fd);
        }
    }
}
