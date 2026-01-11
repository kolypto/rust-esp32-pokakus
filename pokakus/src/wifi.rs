use defmt;

use core::str::FromStr;

use esp_hal::{
    rng::Rng,
};
use esp_radio::wifi;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embassy_net::{DhcpConfig};

use crate::mk_static;


// anyhow: return errors
use anyhow::{Context, Result};


// Load WiFi credential from environment variables
// Variables will be read *at compile time*
const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");

// Name yourself
const DHCP_HOSTNAME: Option<&str> = option_env!("DHCP_HOSTNAME");

// The number of sockets to allocate enough space for.
const N_SOCKETS: usize = 7;


// Start WiFi, spawn net tasks, return net stack
pub async fn start_wifi(
    spawner: &Spawner,
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
) -> Result<embassy_net::Stack<'static>> {
    // Init controller
    let radio: &esp_radio::Controller<'static> = mk_static!(esp_radio::Controller, esp_radio::init().context("Init radio")?);
    let (mut wifi_controller, interfaces) =
        wifi::new(&radio, wifi_peripheral, Default::default())
            .context("Failed to initialize Wi-Fi controller")?;
    let wifi_interface = interfaces.sta;

    // WiFi power saving.
    // We only send occasional HTTP requests, so MAX should be fine.
    // Otherwise the chip gets really hot.
    wifi_controller.set_power_saving(wifi::PowerSaveMode::Maximum)?;

    // Network config: DHCP
    let net_config = embassy_net::Config::dhcpv4({
        let mut c = DhcpConfig::default();
        c.hostname = match DHCP_HOSTNAME {  // feature="dhcpv4-hostname"
            None => None,
            Some(v) => heapless_0_8::String::from_str(v).ok()
        };
        defmt::info!("DHCP_HOSTNAME: {}", c.hostname);
        c
    });


    // Network stack.
    // It also needs a random number: for TLS and networking.
    // The net stack wants a u64, so we join two u32-s.
    let rng = Rng::new();
    let net_seed = rng.random() as u64 | ((rng.random() as u64) << 32);
    let (stack, runner) = embassy_net::new(
        wifi_interface, net_config,
        mk_static!(embassy_net::StackResources::<N_SOCKETS>, embassy_net::StackResources::<N_SOCKETS>::new()),
        net_seed,
    );

    // Start background tasks:
    // - the connection_task will maintain the Wi-Fi connection
    // - the net_task will run the network stack and handle network events.
    // - report WiFi state to the LED
    spawner.spawn(task_keep_wifi_client_up(wifi_controller)).ok();
    spawner.spawn(task_network(runner)).ok();
    // NOTE: `stack` is `Copy`, so just clone it :)
    spawner.spawn(task_report_network_state(stack)).ok();

    // Wait until the connection is up
    // wait_for_connection(stack).await;

    // Done
    Ok(stack)
}


// Task: run the network stack
#[embassy_executor::task]
async fn task_network(mut runner: embassy_net::Runner<'static, wifi::WifiDevice<'static>>) {
    runner.run().await
}


// Task: manage WiFi connection by continuously checking the status, configuring the Wi-Fi controller,
// and attempting to reconnect if the connection is lost or not started.
#[embassy_executor::task]
async fn task_keep_wifi_client_up(mut controller: wifi::WifiController<'static>) {
    loop {
        // Set LED state
        crate::led::set_led_state({
            match wifi::sta_state() {
                wifi::WifiStaState::Connected => crate::led::LedState::PresenceBlink,
                _ => crate::led::LedState::PatientBlink,
            }
        });

        // 1. Check WiFi state
        // If it is in StaConnected, we wait until it gets disconnected.
        if wifi::sta_state() == wifi::WifiStaState::Connected {
            // wait until we're no longer connected, then a bit more -- and reconnect
            controller.wait_for_event(wifi::WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_secs(5)).await;
        }

        // 2. Check if the WiFi controller is started.
        // If not, we initialize the WiFi client configuration.
        if !matches!(controller.is_started(), Ok(true)) {
            // Init client. Use SSID.
            let client_config = wifi::ModeConfig::Client(
                wifi::ClientConfig::default()
                    .with_ssid(SSID.into())
                    .with_password(PASSWORD.into())
                    .with_auth_method(wifi::AuthMethod::Wpa2Personal),  // TODO: configurable?
            );
            controller.set_config(&client_config).unwrap();
            defmt::debug!("WiFi: starting...");

            // Wifi start.
            controller.start_async().await.unwrap();
        }

        // Wait until connected
        defmt::debug!("WiFi: connecting...");
        match controller.connect_async().await {
            // NOTE: This is only WiFi.
            // The network stack (smoltcp) will need to use its DHCP client now.
            Ok(_) => {
                let rssi = controller.rssi().unwrap_or(-999);
                defmt::info!("WiFi: connected! rssi={}", rssi);
            }
            Err(e) => {
                defmt::warn!("WiFi: failed to connect: {:?}", e);

                // Sleep before trying again
                Timer::after(Duration::from_secs(5)).await
            }
        }
    }
}

// Task: wait for the Wi-Fi link to be up, then obtain the IP address.
#[embassy_executor::task]
async fn task_report_network_state(stack: embassy_net::Stack<'static>) {
    loop {
        // Wait up
        defmt::info!("Network: connecting...");
        crate::led::set_led_state(crate::led::LedState::PatientBlink);
        stack.wait_config_up().await;

        // Get config
        if let Some(config) = stack.config_v4() {
            defmt::info!("Network: UP! IP: {} MAC: {}", config.address, stack.hardware_address());
            crate::led::set_led_state(crate::led::LedState::PresenceBlink);
        } else {
            continue;
        }

        // Wait down
        stack.wait_config_down().await;
    }
}
