#![no_std]
#![no_main]

mod config;
mod error;

use config::*;
use error::RequestError;

use esp_backtrace as _;

use core::str::{self, FromStr};
use esp_println::println;
use heapless::String;
use log::{error, info};
use serde::Deserialize;

use cu40026::{interface::SerialInterface, CursorType, Display};
use embassy_executor::Spawner;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    DhcpConfig, Stack, StackResources,
};
use embassy_time::{Duration, Timer};
use esp_hal::{
    gpio::Io,
    prelude::*,
    rng::Rng,
    timer::{
        systimer::{SystemTimer, Target},
        timg::TimerGroup,
    },
    uart::{self, UartTx},
    Blocking,
};
use esp_wifi::{
    wifi::{
        ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice,
        WifiState,
    },
    EspWifiInitFor,
};
use reqwless::{
    client::{HttpClient, TlsConfig},
    request::{Method, RequestBuilder},
};

extern crate alloc;
use esp_alloc as _;

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_hal::macros::main]
async fn main(spawner: Spawner) {
    esp_alloc::heap_allocator!(72 * 1024);

    esp_println::logger::init_logger_from_env();

    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });

    let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);

    let systimer = SystemTimer::new(peripherals.SYSTIMER).split::<Target>();

    let mut rng = Rng::new(peripherals.RNG);
    let net_seed = rng.random() as u64 | ((rng.random() as u64) << 32);
    let tls_seed = rng.random() as u64 | ((rng.random() as u64) << 32);

    esp_hal_embassy::init(systimer.alarm0);

    let uart_config = uart::config::Config::default()
        .baudrate(19200)
        .parity_even();
    let tx = UartTx::new_with_config(peripherals.UART1, uart_config, io.pins.gpio4).unwrap();
    let serial_interface = SerialInterface::new(tx);
    let display = cu40026::Display::new(serial_interface);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let wifi_init = esp_wifi::init(
        EspWifiInitFor::Wifi,
        timg0.timer0,
        rng,
        peripherals.RADIO_CLK,
    )
    .unwrap();
    let wifi = peripherals.WIFI;

    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&wifi_init, wifi, WifiStaDevice).unwrap();

    let mut dhcp_config = DhcpConfig::default();
    dhcp_config.hostname = Some(String::from_str("VFD-HA-Dashboard").unwrap());

    let net_config = embassy_net::Config::dhcpv4(dhcp_config);

    let stack = &*mk_static!(
        Stack<WifiDevice<'_, WifiStaDevice>>,
        Stack::new(
            wifi_interface,
            net_config,
            mk_static!(StackResources<3>, StackResources::<3>::new()),
            net_seed
        )
    );

    spawner.spawn(connection_task(controller, stack)).ok();
    spawner.spawn(net_task(stack)).ok();
    spawner.spawn(display_task(stack, display, tls_seed)).ok();
}

#[embassy_executor::task]
async fn connection_task(
    mut controller: WifiController<'static>,
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
) {
    loop {
        if esp_wifi::wifi::get_wifi_state() == WifiState::StaConnected {
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASS.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            println!("Starting Wi-Fi");
            controller.start().await.unwrap();
        }
        println!("Connecting...");

        match controller.connect().await {
            Ok(_) => {
                println!("Wi-Fi connected!");

                stack.wait_config_up().await;
                println!(
                    "Got IP: {:?}",
                    stack.config_v4().unwrap().address.address().0
                );
            }
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
    stack.run().await
}

#[derive(Deserialize)]
struct EntityState<'a> {
    state: &'a str,
}

struct Entity {
    display_name: &'static str,
    display_unit: &'static [u8],
    entity_name: &'static str,
    position: u8,
}

async fn get_entity_state<'a>(
    client: &mut HttpClient<
        '_,
        TcpClient<'_, WifiDevice<'static, WifiStaDevice>, 1, 4096, 4096>,
        DnsSocket<'_, WifiDevice<'static, WifiStaDevice>>,
    >,
    buf: &'a mut [u8],
    url: &str,
) -> Result<&'a str, RequestError> {
    info!("Requesting {}", url);
    let mut request = client
        .request(Method::GET, url)
        .await?
        .headers(&[("Authorization", TOKEN)]);
    let response = request.send(buf).await?;

    info!("Got response");
    let res = response.body().read_to_end().await?;

    let str = str::from_utf8(res).map_err(RequestError::Utf8Error)?;
    println!("{}", str);

    let (entity_state, _) =
        serde_json_core::from_str::<EntityState>(str).map_err(RequestError::JsonErr)?;

    Ok(entity_state.state)
}

#[embassy_executor::task]
async fn display_task(
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
    mut display: Display<SerialInterface<UartTx<'static, esp_hal::peripherals::UART1, Blocking>>>,
    tls_seed: u64,
) {
    display.initialise().unwrap();
    Timer::after(Duration::from_millis(10)).await;
    display
        .set_cursor_type(CursorType::InvisibleCursor)
        .unwrap();
    display.set_luminance(DEFAULT_LUMINANCE).unwrap();

    stack.wait_config_up().await;

    let mut entity_text_lengths: [usize; NUM_ENTITIES] = [0; NUM_ENTITIES];

    let dns = DnsSocket::new(&stack);
    let tcp_state = TcpClientState::<1, 4096, 4096>::new();
    let tcp = TcpClient::new(stack, &tcp_state);

    let mut tls_rx_buffer = [0u8; 4096];
    let mut tls_tx_buffer = [0u8; 4096];
    let tls = TlsConfig::new(
        tls_seed,
        &mut tls_rx_buffer,
        &mut tls_tx_buffer,
        reqwless::client::TlsVerify::None,
    );

    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);

    let mut buf = [0u8; 4096];
    loop {
        let mut url: String<512> = String::new();

        // Update luminance
        if let Some(luminance_entity) = LUMINANCE_ENTITY {
            url.push_str(HA_API_URL).expect("HA API URL too long");
            url.push_str(luminance_entity).expect("HA API URL too long");

            match get_entity_state(&mut client, &mut buf, url.as_str()).await {
                Ok(entity_state) => {
                    let luminance: u8 = entity_state
                        .parse::<f32>()
                        .expect("Could not parse luminance")
                        as u8;
                    display.set_luminance(luminance).unwrap();
                }
                Err(err) => {
                    error!("{:?}", err);
                }
            }
        }

        // Update entities
        for i in 0..NUM_ENTITIES {
            let entity = &ENTITIES[i];
            let old_text_length = &mut entity_text_lengths[i];

            url.clear();
            url.push_str(HA_API_URL).expect("HA API URL too long");
            url.push_str(entity.entity_name)
                .expect("HA API URL too long");

            display.move_cursor(entity.position).unwrap();
            display.write_str(entity.display_name).unwrap();
            display.write_str(": ").unwrap();

            let new_text_length;

            match get_entity_state(&mut client, &mut buf, url.as_str()).await {
                Ok(mut entity_state) => {
                    if entity_state == "unavailable" {
                        entity_state = "?";
                    }

                    new_text_length = entity.display_name.len()
                        + 2
                        + entity_state.len()
                        + entity.display_unit.len();

                    display.write_str(entity_state).unwrap();
                    display.write_bytes(entity.display_unit).unwrap();
                }
                Err(err) => {
                    error!("{:?}", err);
                    new_text_length = entity.display_name.len() + 2 + 3;
                    display.write_str("ERR").unwrap();
                }
            };

            // Fill the display with empty characters where the old text used to be
            for _ in 0..old_text_length.saturating_sub(new_text_length) {
                display.write_str(" ").unwrap();
            }

            *old_text_length = new_text_length;
        }

        Timer::after(REFRESH_EVERY).await;
    }
}
