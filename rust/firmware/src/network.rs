//! Wi-Fi setup recovery, captive DNS, and Japan time without process-global TZ.

use std::time::{SystemTime, UNIX_EPOCH};

pub const SETUP_SSID: &str = "ChibiStackChan-Setup";
pub const SETUP_PASSWORD: &str = "stackchan";
pub const SETUP_IP: [u8; 4] = [192, 168, 4, 1];
pub const HOSTNAME: &str = "chibi-chan";
const WEEKDAYS: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];

/// Answer a single ordinary DNS question. Compressed question names are
/// rejected, so an untrusted packet cannot create pointer loops or recursion.
/// AAAA and other record types receive a successful response with no records.
pub fn dns_response(query: &[u8], address: [u8; 4]) -> Option<Vec<u8>> {
    if query.len() < 17 || query.len() > 512
        || query[2] & 0xf8 != 0 // Queries only, standard opcode.
        || query[3] & 0x0f != 0
        || query[4..6] != [0, 1]
        || query[6..10] != [0, 0, 0, 0]
    {
        return None;
    }
    let mut end = 12;
    loop {
        let len = usize::from(*query.get(end)?);
        end += 1;
        if len == 0 {
            break;
        }
        if len > 63 || end.checked_add(len)? > query.len() {
            return None;
        }
        end += len;
        if end - 12 > 254 {
            return None;
        }
    }
    let question_end = end.checked_add(4)?;
    let question = query.get(end..question_end)?;
    let record_type = u16::from_be_bytes([question[0], question[1]]);
    if question[2..4] != [0, 1] {
        return None;
    }
    let answer_a = matches!(record_type, 1 | 255);
    let mut response = Vec::with_capacity(question_end + if answer_a { 16 } else { 0 });
    response.extend_from_slice(&query[..question_end]);
    response[2] = 0x84 | (query[2] & 1); // Authoritative answer, preserve RD.
    response[3] = 0;
    response[6..8].copy_from_slice(&[0, u8::from(answer_a)]);
    response[8..12].fill(0); // No authority/additional records copied from query.
    if answer_a {
        response.extend_from_slice(&[
            0xc0, 0x0c, // Name pointer to question at offset 12.
            0, 1, 0, 1, // A, IN.
            0, 0, 0, 30, // Short TTL, so setup does not poison later lookups.
            0, 4,
        ]);
        response.extend_from_slice(&address);
    }
    Some(response)
}

pub fn now_japan() -> String {
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return String::new();
    };
    synchronized_japan_timestamp(duration.as_secs())
}

fn synchronized_japan_timestamp(unix_seconds: u64) -> String {
    // Do not present the power-on epoch as a real date before SNTP completes.
    if unix_seconds < 1_577_836_800 {
        return String::new();
    }
    format_japan_timestamp(unix_seconds).unwrap_or_default()
}

#[cfg(target_os = "espidf")]
fn format_japan_timestamp(unix_seconds: u64) -> Option<String> {
    use esp_idf_svc::sys;
    let shifted: sys::time_t = unix_seconds.checked_add(9 * 3600)?.try_into().ok()?;
    let mut tm: sys::tm = unsafe { core::mem::zeroed() };
    // gmtime_r avoids setenv/TZ/tzset and is safe alongside SNTP and HTTP tasks.
    if unsafe { sys::gmtime_r(&shifted, &mut tm) }.is_null() {
        return None;
    }
    let weekday = WEEKDAYS.get(usize::try_from(tm.tm_wday).ok()?)?;
    Some(format!(
        "{:04}年{:02}月{:02}日（{weekday}曜日） {:02}:{:02}:{:02}（日本時間）",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}

#[cfg(not(target_os = "espidf"))]
fn format_japan_timestamp(unix_seconds: u64) -> Option<String> {
    // Gregorian civil-date conversion for the portable host preview/tests.
    let shifted = unix_seconds.checked_add(9 * 3600)?;
    let days = i64::try_from(shifted / 86400).ok()?;
    let z = days.checked_add(719468)?;
    let era = z.div_euclid(146097);
    let day_of_era = z - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    if !(1970..=9999).contains(&year) {
        return None;
    }
    let seconds = shifted % 86400;
    let weekday = WEEKDAYS.get(usize::try_from((days + 4).rem_euclid(7)).ok()?)?;
    Some(format!(
        "{year:04}年{month:02}月{day:02}日（{weekday}曜日） {:02}:{:02}:{:02}（日本時間）",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    ))
}

#[cfg(target_os = "espidf")]
pub use esp::Network;

#[cfg(target_os = "espidf")]
mod esp {
    use super::*;
    use crate::config::Config;
    use anyhow::Context;
    use esp_idf_svc::{
        eventloop::EspSystemEventLoop,
        hal::modem::Modem,
        ipv4,
        netif::{EspNetif, NetifConfiguration},
        nvs::EspDefaultNvsPartition,
        sntp::EspSntp,
        wifi::{
            AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration, EspWifi,
            WifiDriver,
        },
    };
    use std::{
        net::{Ipv4Addr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
    const RETRY_INTERVAL: Duration = Duration::from_secs(15);
    const PORTAL_AFTER_OUTAGE: Duration = Duration::from_secs(120);

    pub struct Network {
        pub portal: bool,
        dns: Option<CaptiveDns>,
        #[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
        mdns: Option<esp_idf_svc::mdns::EspMdns>,
        sntp: Option<EspSntp<'static>>,
        wifi: EspWifi<'static>,
        _sys_loop: EspSystemEventLoop,
        last_connected: Instant,
        last_retry: Instant,
        last_service_retry: Instant,
    }

    impl Network {
        pub fn new(
            modem: Modem<'static>,
            nvs: EspDefaultNvsPartition,
            config: &Config,
            force_portal: bool,
        ) -> anyhow::Result<Self> {
            let sys_loop = EspSystemEventLoop::take()?;
            let driver = WifiDriver::new(modem, sys_loop.clone(), Some(nvs))?;
            // Config persistence belongs to Storage; prevent the Wi-Fi driver
            // from saving a second copy of the station password in SDK NVS.
            esp_idf_svc::sys::esp!(unsafe {
                esp_idf_svc::sys::esp_wifi_set_storage(
                    esp_idf_svc::sys::wifi_storage_t_WIFI_STORAGE_RAM,
                )
            })?;
            let mut station = NetifConfiguration::wifi_default_client();
            station.ip_configuration = Some(ipv4::Configuration::Client(
                ipv4::ClientConfiguration::DHCP(ipv4::DHCPClientSettings {
                    hostname: Some(
                        HOSTNAME
                            .try_into()
                            .map_err(|_| anyhow::anyhow!("hostname too long"))?,
                    ),
                }),
            ));
            let address = ipv4::Ipv4Addr::from(SETUP_IP);
            let mut access_point = NetifConfiguration::wifi_default_router();
            access_point.ip_configuration =
                Some(ipv4::Configuration::Router(ipv4::RouterConfiguration {
                    subnet: ipv4::Subnet {
                        gateway: address,
                        mask: ipv4::Mask(24),
                    },
                    dhcp_enabled: true,
                    dns: Some(address),
                    secondary_dns: None,
                }));
            let wifi = EspWifi::wrap_all(
                driver,
                EspNetif::new_with_conf(&station)?,
                EspNetif::new_with_conf(&access_point)?,
            )?;
            let now = Instant::now();
            let mut network = Self {
                portal: false,
                dns: None,
                wifi,
                _sys_loop: sys_loop,
                sntp: None,
                #[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
                mdns: None,
                last_connected: now,
                last_retry: now,
                last_service_retry: now,
            };
            if !force_portal && !config.wifi_ssid.is_empty() {
                match network.connect_station(config) {
                    Ok(true) => {
                        network.last_connected = Instant::now();
                        network.start_services();
                        log::info!("Wi-Fi connected: http://{}/", network.ip());
                        return Ok(network);
                    }
                    Ok(false) => log::warn!("Wi-Fi connection timed out; entering setup mode"),
                    Err(error) => log::warn!("Wi-Fi unavailable; entering setup mode: {error}"),
                }
            }
            network.enter_portal()?;
            Ok(network)
        }

        fn connect_station(&mut self, config: &Config) -> anyhow::Result<bool> {
            let client = ClientConfiguration {
                ssid: config
                    .wifi_ssid
                    .as_str()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("SSID exceeds 32 bytes"))?,
                password: config
                    .wifi_pass
                    .as_str()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("Wi-Fi password exceeds 64 bytes"))?,
                auth_method: if config.wifi_pass.is_empty() {
                    AuthMethod::None
                } else {
                    AuthMethod::WPA2Personal
                },
                ..Default::default()
            };
            self.wifi
                .set_configuration(&Configuration::Client(client))?;
            self.wifi.start()?;
            let started = Instant::now();
            let mut next_attempt = started;
            while started.elapsed() < CONNECT_TIMEOUT {
                if self.connected() {
                    return Ok(true);
                }
                if Instant::now() >= next_attempt && self.wifi.is_started()? {
                    let _ = self.wifi.disconnect();
                    // connect() on EspWifi requests connection without waiting.
                    let _ = self.wifi.connect();
                    next_attempt = Instant::now() + Duration::from_secs(5);
                }
                thread::sleep(Duration::from_millis(50));
            }
            Ok(false)
        }

        pub fn ip(&self) -> String {
            let netif = if self.portal {
                self.wifi.ap_netif()
            } else {
                self.wifi.sta_netif()
            };
            netif
                .get_ip_info()
                .map(|info| info.ip.to_string())
                .unwrap_or_else(|_| "0.0.0.0".into())
        }

        /// True only for a station with a usable address, not for the setup AP.
        pub fn connected(&self) -> bool {
            !self.portal
                && self.wifi.is_connected().unwrap_or(false)
                && self.wifi.sta_netif().is_up().unwrap_or(false)
        }

        pub fn enter_portal(&mut self) -> anyhow::Result<()> {
            if self.portal {
                return Ok(());
            }
            self.sntp = None;
            #[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
            {
                self.mdns = None;
            }
            let _ = self.wifi.disconnect();
            if self.wifi.is_started()? {
                self.wifi.stop()?;
            }
            self.wifi
                .set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
                    ssid: SETUP_SSID
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("setup SSID too long"))?,
                    password: SETUP_PASSWORD
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("setup password too long"))?,
                    auth_method: AuthMethod::WPA2Personal,
                    max_connections: 4,
                    channel: 1,
                    ..Default::default()
                }))?;
            self.wifi.start()?;
            self.portal = true;
            self.start_dns();
            self.last_retry = Instant::now();
            log::info!("Setup Wi-Fi ready: {SETUP_SSID}, http://192.168.4.1/");
            Ok(())
        }

        /// Keep the main audio/UI loop responsive: reconnection is requested
        /// asynchronously and no sleep or connection wait occurs here.
        pub fn maintain(&mut self) -> anyhow::Result<()> {
            let now = Instant::now();
            if self.portal {
                if self.dns.as_ref().is_some_and(|dns| dns.finished()) {
                    self.dns = None;
                }
                if self.dns.is_none() && now.duration_since(self.last_retry) >= RETRY_INTERVAL {
                    self.last_retry = now;
                    self.start_dns();
                }
                return Ok(());
            }
            if self.connected() {
                self.last_connected = now;
                if now.duration_since(self.last_service_retry) >= RETRY_INTERVAL {
                    self.start_services();
                }
                return Ok(());
            }
            if now.duration_since(self.last_connected) >= PORTAL_AFTER_OUTAGE {
                log::warn!("Wi-Fi outage exceeded two minutes; enabling setup recovery");
                return self.enter_portal();
            }
            if now.duration_since(self.last_retry) >= RETRY_INTERVAL {
                self.last_retry = now;
                let _ = self.wifi.disconnect();
                if let Err(error) = self.wifi.connect() {
                    log::warn!("Wi-Fi reconnect request failed: {error}");
                }
            }
            Ok(())
        }

        fn start_dns(&mut self) {
            if self.dns.is_some() {
                return;
            }
            match CaptiveDns::start() {
                Ok(dns) => self.dns = Some(dns),
                Err(error) => {
                    log::warn!("Captive DNS unavailable; use http://192.168.4.1/: {error}")
                }
            }
        }

        fn start_services(&mut self) {
            self.last_service_retry = Instant::now();
            if self.sntp.is_none() {
                match EspSntp::new_default() {
                    Ok(sntp) => self.sntp = Some(sntp),
                    Err(error) => log::warn!("Time synchronization unavailable: {error}"),
                }
            }
            #[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
            if self.mdns.is_none() {
                match Self::start_mdns() {
                    Ok(mdns) => self.mdns = Some(mdns),
                    Err(error) => {
                        log::warn!("mDNS unavailable; use the device IP address: {error}")
                    }
                }
            }
        }

        #[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
        fn start_mdns() -> anyhow::Result<esp_idf_svc::mdns::EspMdns> {
            let mut mdns = esp_idf_svc::mdns::EspMdns::take()?;
            mdns.set_hostname(HOSTNAME)?;
            mdns.set_instance_name("Chibi Stackchan")?;
            mdns.add_service(None, "_http", "_tcp", 80, &[])?;
            Ok(mdns)
        }
    }

    struct CaptiveDns {
        stop: Arc<AtomicBool>,
        task: Option<JoinHandle<()>>,
    }

    impl CaptiveDns {
        fn start() -> anyhow::Result<Self> {
            let socket =
                UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 53)).context("bind DNS UDP port")?;
            socket.set_read_timeout(Some(Duration::from_millis(250)))?;
            socket.set_write_timeout(Some(Duration::from_millis(250)))?;
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let task = thread::Builder::new()
                .name("captive-dns".into())
                .stack_size(8192)
                .spawn(move || {
                    let mut packet = [0u8; 512];
                    while !worker_stop.load(Ordering::Relaxed) {
                        match socket.recv_from(&mut packet) {
                            Ok((len, peer)) => {
                                if let Some(response) = dns_response(&packet[..len], SETUP_IP) {
                                    let _ = socket.send_to(&response, peer);
                                }
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                        | std::io::ErrorKind::Interrupted
                                ) => {}
                            Err(error) => {
                                log::warn!("Captive DNS receiver stopped: {error}");
                                break;
                            }
                        }
                    }
                })?;
            Ok(Self {
                stop,
                task: Some(task),
            })
        }

        fn finished(&self) -> bool {
            self.task.as_ref().is_none_or(JoinHandle::is_finished)
        }
    }

    impl Drop for CaptiveDns {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(task) = self.task.take() {
                let _ = task.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(record_type: u16) -> Vec<u8> {
        let mut bytes = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(b"\x07example\x03com\0");
        bytes.extend_from_slice(&record_type.to_be_bytes());
        bytes.extend_from_slice(&[0, 1]);
        bytes
    }

    #[test]
    fn dns_answers_ipv4_and_preserves_transaction_and_question() {
        let request = query(1);
        let response = dns_response(&request, SETUP_IP).unwrap();
        assert_eq!(&response[..2], &[0x12, 0x34]);
        assert_eq!(response[2], 0x85);
        assert_eq!(&response[4..12], &[0, 1, 0, 1, 0, 0, 0, 0]);
        assert_eq!(&response[12..request.len()], &request[12..]);
        assert_eq!(&response[response.len() - 4..], &SETUP_IP);
        assert_eq!(response.len(), request.len() + 16);
    }

    #[test]
    fn ipv6_and_unknown_records_have_no_invented_address() {
        for record_type in [28, 16, 65] {
            let request = query(record_type);
            let response = dns_response(&request, SETUP_IP).unwrap();
            assert_eq!(&response[6..8], &[0, 0]);
            assert_eq!(response.len(), request.len());
        }
        assert_eq!(&dns_response(&query(255), SETUP_IP).unwrap()[6..8], &[0, 1]);
    }

    #[test]
    fn dns_rejects_truncation_pointers_and_non_queries() {
        let request = query(1);
        for end in 0..request.len() {
            assert!(dns_response(&request[..end], SETUP_IP).is_none());
        }
        for (index, byte) in [(2, 0x81), (2, 0x09), (5, 2), (12, 0xc0), (12, 64)] {
            let mut malformed = request.clone();
            malformed[index] = byte;
            assert!(dns_response(&malformed, SETUP_IP).is_none());
        }
        let mut wrong_class = request;
        *wrong_class.last_mut().unwrap() = 3;
        assert!(dns_response(&wrong_class, SETUP_IP).is_none());
        assert!(dns_response(&[0; 513], SETUP_IP).is_none());
    }

    #[test]
    fn japan_time_crosses_utc_day_and_leap_year_boundaries() {
        assert_eq!(
            format_japan_timestamp(0).as_deref(),
            Some("1970年01月01日（木曜日） 09:00:00（日本時間）")
        );
        // 2024-02-28 15:00 UTC is leap day at midnight in Japan.
        assert_eq!(
            format_japan_timestamp(1_709_132_400).as_deref(),
            Some("2024年02月29日（木曜日） 00:00:00（日本時間）")
        );
        assert_eq!(
            format_japan_timestamp(1_735_657_200).as_deref(),
            Some("2025年01月01日（水曜日） 00:00:00（日本時間）")
        );
        assert!(format_japan_timestamp(u64::MAX).is_none());
    }

    #[test]
    fn japan_time_is_empty_until_synced_and_rolls_weekday_at_jst_midnight() {
        assert!(synchronized_japan_timestamp(0).is_empty());
        assert!(synchronized_japan_timestamp(1_577_836_799).is_empty());
        assert!(synchronized_japan_timestamp(u64::MAX).is_empty());
        let midnight = 1_735_657_200;
        assert_eq!(
            synchronized_japan_timestamp(midnight - 1),
            "2024年12月31日（火曜日） 23:59:59（日本時間）"
        );
        for (day, name) in ["水", "木", "金", "土", "日", "月", "火"]
            .iter()
            .enumerate()
        {
            assert!(synchronized_japan_timestamp(midnight + day as u64 * 86400)
                .contains(&format!("（{name}曜日）")));
        }
    }
}
