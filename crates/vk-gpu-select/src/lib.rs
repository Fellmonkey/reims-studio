//! Fallback Vulkan layer that puts a chosen GPU at physical device index 0.
//!
//! The layer is used only when two GPUs share a VID:DID pair or the loader is
//! too old for `VK_LOADER_DEVICE_SELECT`. The launcher sets
//! `VK_ADD_LAYER_PATH` only when it runs without elevation, because the Vulkan
//! loader ignores that variable for elevated processes.
//!
//! # Quick start
//!
//! The target is selected with `REIMS_GPU_SELECT=vid:did`, for example
//! `REIMS_GPU_SELECT=10de:2484`. The loader loads this library through the
//! accompanying `VkLayer_reims_gpu_select.json` manifest.
//!
//! ```
//! use vk_gpu_select::{parse_device_select, selected_index};
//!
//! let target = parse_device_select("10de:2484").expect("device select");
//! let devices = [(0x8086, 0x9bc5), (0x10de, 0x2484)];
//! assert_eq!(selected_index(&devices, target), Some(1));
//! ```

// SAFETY scope: this crate is the Vulkan loader boundary. Every unsafe block
// below documents the invariant that makes the call valid.
#![allow(unsafe_code, reason = "Vulkan loader FFI boundary")]

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};

/// Vulkan result code for success.
pub const VK_SUCCESS: i32 = 0;
/// Vulkan result code for a failed initialization.
pub const VK_ERROR_INITIALIZATION_FAILED: i32 = -3;
/// Vulkan loader interface version implemented by this layer.
pub const LOADER_LAYER_INTERFACE_VERSION: u32 = 2;

const VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO: i32 = 42;
const VK_LAYER_FUNCTION_LINK: i32 = 1;

const CREATE_INSTANCE: &[u8] = b"vkCreateInstance\0";
const ENUMERATE_PHYSICAL_DEVICES: &[u8] = b"vkEnumeratePhysicalDevices\0";
const GET_PHYSICAL_DEVICE_PROPERTIES: &[u8] = b"vkGetPhysicalDeviceProperties\0";

type VkInstance = *mut c_void;
type VkPhysicalDevice = *mut c_void;
type PfnVoid = unsafe extern "system" fn();
type PfnGetInstanceProcAddr =
    unsafe extern "system" fn(VkInstance, *const c_char) -> Option<PfnVoid>;
type PfnCreateInstance =
    unsafe extern "system" fn(*const c_void, *const c_void, *mut VkInstance) -> i32;
type PfnEnumeratePhysicalDevices =
    unsafe extern "system" fn(VkInstance, *mut u32, *mut VkPhysicalDevice) -> i32;
type PfnGetPhysicalDeviceProperties = unsafe extern "system" fn(VkPhysicalDevice, *mut c_void);

static NEXT_GET_INSTANCE_PROC_ADDR: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct VkNegotiateLayerInterface {
    function: i32,
    next: *mut c_void,
    loader_layer_interface_version: u32,
    get_instance_proc_addr: Option<PfnGetInstanceProcAddr>,
    get_device_proc_addr: Option<PfnVoid>,
    get_physical_device_proc_addr: Option<PfnVoid>,
}

#[repr(C)]
struct VkInstanceCreateInfoPrefix {
    function: i32,
    next: *const c_void,
}

#[repr(C)]
union VkLayerInstanceLink {
    next_get_instance_proc_addr: Option<PfnGetInstanceProcAddr>,
    next_get_physical_device_proc_addr: Option<PfnVoid>,
}

#[repr(C)]
struct VkLayerInstanceCreateInfo {
    function_type: i32,
    next: *const c_void,
    function: i32,
    link: VkLayerInstanceLink,
}

#[repr(C)]
struct VkPhysicalDevicePropertiesPrefix {
    api_version: u32,
    driver_version: u32,
    vendor_id: u32,
    device_id: u32,
}

#[repr(C, align(16))]
struct PropertiesBuffer([u8; 2048]);

/// Parses a `vid:did` selector into numeric IDs.
///
/// # Errors
///
/// Returns `None` when either component is not four hexadecimal digits.
pub fn parse_device_select(value: &str) -> Option<(u32, u32)> {
    let (vendor, device) = value.split_once(':')?;
    let vendor = u32::from_str_radix(vendor.trim(), 16).ok()?;
    let device = u32::from_str_radix(device.trim(), 16).ok()?;
    if vendor > 0xffff || device > 0xffff {
        return None;
    }
    Some((vendor, device))
}

fn selected_ids() -> Option<(u32, u32)> {
    let value = std::env::var("REIMS_GPU_SELECT").ok()?;
    parse_device_select(&value)
}

/// Returns the index of the device matching `target`.
pub fn selected_index(devices: &[(u32, u32)], target: (u32, u32)) -> Option<usize> {
    devices.iter().position(|device| *device == target)
}

fn store_next_proc_addr(proc_addr: PfnGetInstanceProcAddr) {
    NEXT_GET_INSTANCE_PROC_ADDR.store(proc_addr as usize as u64, Ordering::Release);
}

fn next_proc_addr() -> Option<PfnGetInstanceProcAddr> {
    let raw = NEXT_GET_INSTANCE_PROC_ADDR.load(Ordering::Acquire);
    if raw == 0 {
        return None;
    }
    // SAFETY: the stored value was written from a live Vulkan function
    // pointer and Vulkan function pointers remain valid for the process.
    Some(unsafe { std::mem::transmute::<usize, PfnGetInstanceProcAddr>(raw as usize) })
}

unsafe fn delegate(instance: VkInstance, name: &[u8]) -> Option<PfnVoid> {
    let next = next_proc_addr()?;
    // SAFETY: `next` is the loader or lower layer GetInstanceProcAddr and
    // `name` is a static null-terminated Vulkan function name.
    unsafe { next(instance, name.as_ptr().cast::<c_char>()) }
}

#[unsafe(no_mangle)]
extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(
    interface: *mut VkNegotiateLayerInterface,
) -> i32 {
    if interface.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SAFETY: the loader passes a writable pointer to its interface structure.
    unsafe {
        let next = (*interface).get_instance_proc_addr;
        if let Some(next) = next {
            store_next_proc_addr(next);
        }
        (*interface).function = 0;
        (*interface).next = std::ptr::null_mut();
        (*interface).loader_layer_interface_version = LOADER_LAYER_INTERFACE_VERSION;
        (*interface).get_instance_proc_addr = Some(vkGetInstanceProcAddr);
        (*interface).get_device_proc_addr = None;
        (*interface).get_physical_device_proc_addr = None;
    }
    VK_SUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "system" fn vkGetInstanceProcAddr(
    instance: VkInstance,
    name: *const c_char,
) -> Option<PfnVoid> {
    if name.is_null() {
        return None;
    }
    // SAFETY: the loader passes a null-terminated function name.
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    match name {
        b"vkGetInstanceProcAddr\0" => {
            // SAFETY: both function pointer types have the system ABI.
            Some(unsafe {
                std::mem::transmute::<PfnGetInstanceProcAddr, PfnVoid>(vkGetInstanceProcAddr)
            })
        }
        b"vkCreateInstance\0" => {
            // SAFETY: both function pointer types have the system ABI.
            Some(unsafe { std::mem::transmute::<PfnCreateInstance, PfnVoid>(vkCreateInstance) })
        }
        b"vkEnumeratePhysicalDevices\0" => {
            // SAFETY: both function pointer types have the system ABI.
            Some(unsafe {
                std::mem::transmute::<PfnEnumeratePhysicalDevices, PfnVoid>(
                    vkEnumeratePhysicalDevices,
                )
            })
        }
        _ => unsafe { delegate(instance, name) },
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn vkCreateInstance(
    create_info: *const c_void,
    allocator: *const c_void,
    instance: *mut VkInstance,
) -> i32 {
    if !create_info.is_null() {
        // SAFETY: the loader passes a valid VkInstanceCreateInfo.
        let info = unsafe { &*create_info.cast::<VkInstanceCreateInfoPrefix>() };
        let mut cursor = info.next;
        while !cursor.is_null() {
            // SAFETY: every pNext node is a Vulkan structure with sType and
            // pNext as its first two fields.
            let node = unsafe { &*cursor.cast::<VkLayerInstanceCreateInfo>() };
            if node.function_type == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO
                && node.function == VK_LAYER_FUNCTION_LINK
            {
                // SAFETY: LINK nodes carry the lower layer GetInstanceProcAddr
                // in this union member.
                if let Some(next) = unsafe { node.link.next_get_instance_proc_addr } {
                    store_next_proc_addr(next);
                }
            }
            cursor = node.next;
        }
    }
    let raw = unsafe { delegate(std::ptr::null_mut(), CREATE_INSTANCE) };
    // SAFETY: Vulkan returns vkCreateInstance with this signature.
    let real: PfnCreateInstance = match raw {
        Some(function) => unsafe { std::mem::transmute::<PfnVoid, PfnCreateInstance>(function) },
        None => return VK_ERROR_INITIALIZATION_FAILED,
    };
    // SAFETY: arguments are passed through unchanged to the lower layer.
    unsafe { real(create_info, allocator, instance) }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn vkEnumeratePhysicalDevices(
    instance: VkInstance,
    count: *mut u32,
    devices: *mut VkPhysicalDevice,
) -> i32 {
    let raw = unsafe { delegate(instance, ENUMERATE_PHYSICAL_DEVICES) };
    // SAFETY: Vulkan returns vkEnumeratePhysicalDevices with this signature.
    let real: PfnEnumeratePhysicalDevices = match raw {
        Some(function) => unsafe {
            std::mem::transmute::<PfnVoid, PfnEnumeratePhysicalDevices>(function)
        },
        None => return VK_ERROR_INITIALIZATION_FAILED,
    };
    // SAFETY: arguments are passed through unchanged to the lower layer.
    let result = unsafe { real(instance, count, devices) };
    if result != VK_SUCCESS || count.is_null() || devices.is_null() {
        return result;
    }
    let Some(target) = selected_ids() else {
        return result;
    };
    let raw = unsafe { delegate(instance, GET_PHYSICAL_DEVICE_PROPERTIES) };
    // SAFETY: Vulkan returns vkGetPhysicalDeviceProperties with this signature.
    let get_properties: PfnGetPhysicalDeviceProperties = match raw {
        Some(function) => unsafe {
            std::mem::transmute::<PfnVoid, PfnGetPhysicalDeviceProperties>(function)
        },
        None => return result,
    };
    // SAFETY: the lower layer wrote `*count` device handles into `devices`.
    let handles = unsafe { std::slice::from_raw_parts_mut(devices, *count as usize) };
    let mut chosen = None;
    for (index, handle) in handles.iter().enumerate() {
        let mut buffer = PropertiesBuffer([0_u8; 2048]);
        // SAFETY: the buffer is 16-byte aligned and large enough for any
        // VkPhysicalDeviceProperties returned by a conforming driver.
        unsafe { get_properties(*handle, buffer.0.as_mut_ptr().cast::<c_void>()) };
        // SAFETY: the prefix matches the Vulkan structure layout and the
        // driver wrote the full structure into `buffer`.
        let properties = unsafe {
            buffer
                .0
                .as_ptr()
                .cast::<VkPhysicalDevicePropertiesPrefix>()
                .read()
        };
        if properties.vendor_id == target.0 && properties.device_id == target.1 {
            chosen = Some(index);
            break;
        }
    }
    if let Some(index) = chosen {
        handles.swap(0, index);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_selectors() {
        assert_eq!(parse_device_select("10de:2484"), Some((0x10de, 0x2484)));
        assert_eq!(parse_device_select(" 8086 : 9bc5 "), Some((0x8086, 0x9bc5)));
        assert_eq!(parse_device_select("invalid"), None);
        assert_eq!(parse_device_select("10000:1"), None);
    }

    #[test]
    fn finds_the_requested_device() {
        let devices = [(0x8086, 0x9bc5), (0x10de, 0x2484), (0x10de, 0x2484)];
        assert_eq!(selected_index(&devices, (0x10de, 0x2484)), Some(1));
        assert_eq!(selected_index(&devices, (0x1002, 0x73bf)), None);
    }

    #[test]
    fn interface_version_is_v2() {
        assert_eq!(LOADER_LAYER_INTERFACE_VERSION, 2);
    }
}
