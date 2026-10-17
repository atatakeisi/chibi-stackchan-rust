#[cfg(target_os = "espidf")]
fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    stackchan_firmware::diagnostics::heap("startup");
    if let Err(error) = stackchan_firmware::device::run() {
        log::error!("firmware stopped: {error:#}");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}

#[cfg(not(target_os = "espidf"))]
fn main() {
    println!("ATOMS3R firmware: build with --target xtensa-esp32s3-espidf; cargo test runs portable logic tests.");
}
