/* Runtime-probed Apple Silicon thermal sensors. */

#![cfg(target_arch = "aarch64")]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_long, c_void};

#[repr(C)]
struct __IOHIDEventSystemClient;
#[repr(C)]
struct __IOHIDServiceClient;
#[repr(C)]
struct __IOHIDEvent;
type Client = *mut __IOHIDEventSystemClient;
type Service = *mut __IOHIDServiceClient;
type Event = *mut __IOHIDEvent;
type CFType = *const c_void;
type CFAllocator = c_void;
type CFDictionary = c_void;
type CFArray = c_void;
type CFString = c_void;
type CFNumber = c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: *const CFAllocator;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFStringCreateWithCString(a: *const CFAllocator, s: *const c_char, enc: u32) -> *mut CFString;
    fn CFNumberCreate(a: *const CFAllocator, ty: c_int, value: *const c_void) -> *mut CFNumber;
    fn CFDictionaryCreate(a: *const CFAllocator, keys: *const *const c_void, values: *const *const c_void,
        count: c_long, key_cb: *const c_void, value_cb: *const c_void) -> *mut CFDictionary;
    fn CFArrayGetCount(a: *const CFArray) -> c_long;
    fn CFArrayGetValueAtIndex(a: *const CFArray, index: c_long) -> CFType;
    fn CFStringGetCString(s: *const CFString, buf: *mut c_char, size: c_long, enc: u32) -> bool;
    fn CFRelease(value: CFType);
}

const RTLD_LAZY: c_int = 1;
const RTLD_LOCAL: c_int = 4;
const K_CF_NUMBER_SINT32: c_int = 3;
const K_CF_STRING_ENCODING_ASCII: u32 = 0x0600;
const TEMPERATURE_EVENT: c_int = 15;

#[allow(non_camel_case_types)]
type Create = unsafe extern "C" fn(*const CFAllocator) -> Client;
type SetMatching = unsafe extern "C" fn(Client, *const CFDictionary);
type CopyServices = unsafe extern "C" fn(Client) -> *mut CFArray;
type CopyProperty = unsafe extern "C" fn(Service, *const CFString) -> *mut CFString;
type CopyEvent = unsafe extern "C" fn(Service, c_int, c_int, i64) -> Event;
type EventValue = unsafe extern "C" fn(Event, c_int) -> c_double;

struct HidApi {
    handle: *mut c_void,
    create: Create,
    set_matching: SetMatching,
    copy_services: CopyServices,
    copy_property: CopyProperty,
    copy_event: CopyEvent,
    event_value: EventValue,
}

impl Drop for HidApi {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.handle); }
    }
}

impl HidApi {
    unsafe fn load() -> Option<Self> {
        let path = CString::new("/System/Library/Frameworks/IOKit.framework/IOKit").ok()?;
        let handle = libc::dlopen(path.as_ptr(), RTLD_LAZY | RTLD_LOCAL);
        if handle.is_null() { return None; }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let name = concat!($name, "\0");
                let ptr = libc::dlsym(handle, name.as_ptr() as *const c_char);
                if ptr.is_null() { libc::dlclose(handle); return None; }
                std::mem::transmute::<*mut c_void, $ty>(ptr)
            }};
        }
        Some(Self {
            handle,
            create: symbol!("IOHIDEventSystemClientCreate", Create),
            set_matching: symbol!("IOHIDEventSystemClientSetMatching", SetMatching),
            copy_services: symbol!("IOHIDEventSystemClientCopyServices", CopyServices),
            copy_property: symbol!("IOHIDServiceClientCopyProperty", CopyProperty),
            copy_event: symbol!("IOHIDServiceClientCopyEvent", CopyEvent),
            event_value: symbol!("IOHIDEventGetFloatValue", EventValue),
        })
    }
}

pub(crate) fn apple_silicon_temperature() -> Option<f32> {
    unsafe { collect() }
}

unsafe fn collect() -> Option<f32> {
    let api = HidApi::load()?;
    let page: i32 = 0xff00;
    let usage: i32 = 5;
    let key_page = CFStringCreateWithCString(kCFAllocatorDefault, b"PrimaryUsagePage\0".as_ptr() as *const c_char, K_CF_STRING_ENCODING_ASCII);
    let key_usage = CFStringCreateWithCString(kCFAllocatorDefault, b"PrimaryUsage\0".as_ptr() as *const c_char, K_CF_STRING_ENCODING_ASCII);
    if key_page.is_null() || key_usage.is_null() { if !key_page.is_null() { CFRelease(key_page as CFType); } if !key_usage.is_null() { CFRelease(key_usage as CFType); } return None; }
    let number_page = CFNumberCreate(kCFAllocatorDefault, K_CF_NUMBER_SINT32, &page as *const _ as *const c_void);
    let number_usage = CFNumberCreate(kCFAllocatorDefault, K_CF_NUMBER_SINT32, &usage as *const _ as *const c_void);
    if number_page.is_null() || number_usage.is_null() {
        CFRelease(key_page as CFType);
        CFRelease(key_usage as CFType);
        if !number_page.is_null() { CFRelease(number_page as CFType); }
        if !number_usage.is_null() { CFRelease(number_usage as CFType); }
        return None;
    }
    let keys = [key_page as *const c_void, key_usage as *const c_void];
    let values = [number_page as *const c_void, number_usage as *const c_void];
    let matching = CFDictionaryCreate(kCFAllocatorDefault, keys.as_ptr(), values.as_ptr(), 2, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFRelease(key_page as CFType); CFRelease(key_usage as CFType); CFRelease(number_page as CFType); CFRelease(number_usage as CFType);
    if matching.is_null() { return None; }
    let client = (api.create)(kCFAllocatorDefault);
    if client.is_null() { CFRelease(matching as CFType); return None; }
    (api.set_matching)(client, matching);
    let services = (api.copy_services)(client);
    let product = CFStringCreateWithCString(kCFAllocatorDefault, b"Product\0".as_ptr() as *const c_char, K_CF_STRING_ENCODING_ASCII);
    let mut sums = [0.0; 3];
    let mut counts = [0u32; 3];
    if !services.is_null() && !product.is_null() {
        for i in 0..CFArrayGetCount(services as *const CFArray) {
            let service = CFArrayGetValueAtIndex(services as *const CFArray, i) as Service;
            if service.is_null() { continue; }
            let name = (api.copy_property)(service, product);
            if name.is_null() { continue; }
            let mut buf = [0i8; 128];
            let valid_name = CFStringGetCString(name, buf.as_mut_ptr(), buf.len() as c_long, K_CF_STRING_ENCODING_ASCII);
            if valid_name {
                let sensor = CStr::from_ptr(buf.as_ptr()).to_bytes();
                let group = if sensor.starts_with(b"eACC") || sensor.starts_with(b"pACC") {
                    Some(0)
                } else if sensor.starts_with(b"PMU tdie") {
                    Some(1)
                } else if sensor.starts_with(b"SOC MTR") {
                    Some(2)
                } else {
                    None
                };
                if let Some(group) = group {
                    let event = (api.copy_event)(service, TEMPERATURE_EVENT, 0, 0);
                    if !event.is_null() {
                        let value = (api.event_value)(event, TEMPERATURE_EVENT << 16);
                        CFRelease(event as CFType);
                        if value.is_finite() && value > 0.0 && value < 150.0 {
                            sums[group] += value;
                            counts[group] += 1;
                        }
                    }
                }
            }
            CFRelease(name as CFType);
        }
    }
    if !product.is_null() { CFRelease(product as CFType); }
    if !services.is_null() { CFRelease(services as CFType); }
    CFRelease(client as CFType);
    CFRelease(matching as CFType);
    counts.iter().position(|&count| count > 0).map(|group| (sums[group] / counts[group] as f64) as f32)
}
