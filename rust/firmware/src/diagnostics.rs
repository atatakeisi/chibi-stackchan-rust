//! Runtime memory and stack headroom, without audio or credential contents.
use esp_idf_svc::sys;

pub fn heap(stage: &str) {
    unsafe {
        log::info!(
            "heap {stage}: free8={} largest8={} internal={} psram={} stack_free={}",
            sys::heap_caps_get_free_size(sys::MALLOC_CAP_8BIT),
            sys::heap_caps_get_largest_free_block(sys::MALLOC_CAP_8BIT),
            sys::heap_caps_get_free_size(sys::MALLOC_CAP_INTERNAL | sys::MALLOC_CAP_8BIT),
            sys::heap_caps_get_free_size(sys::MALLOC_CAP_SPIRAM | sys::MALLOC_CAP_8BIT),
            sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut())
        );
    }
}
