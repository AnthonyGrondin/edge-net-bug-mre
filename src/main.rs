#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

#[doc(hidden)]
pub use esp_hal as hal;

use edge_http::io::server::{Connection, DefaultServer, Handler};
use edge_http::io::Error;
use edge_http::Method;
use edge_nal_embassy::{Tcp, TcpBuffers};

use embedded_io_async::{ErrorType, Read, Write};

use embassy_net::{Config, Stack, StackResources};

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_println::logger::init_logger;
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice,
    WifiState,
};
use esp_wifi::{initialize, EspWifiInitFor};
use hal::{
    clock::ClockControl,
    peripherals::Peripherals,
    prelude::*,
    rng::Rng,
    system::SystemControl,
    timer::{timg::TimerGroup, OneShotTimer, PeriodicTimer},
};
use static_cell::make_static;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

/// Number of sockets used for the HTTP server
const SERVER_SOCKETS: usize = 4;

/// Total number of sockets used for the application
const SOCKET_COUNT: usize = 1 + 1 + SERVER_SOCKETS; // DHCP + DNS + Server

#[main]
async fn main(spawner: Spawner) -> ! {
    init_logger(log::LevelFilter::Debug);

    let peripherals = Peripherals::take();
    let system = SystemControl::new(peripherals.SYSTEM);
    let clocks = ClockControl::max(system.clock_control).freeze();

    #[cfg(target_arch = "xtensa")]
    let timer = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1, &clocks, None).timer0;
    #[cfg(target_arch = "riscv32")]
    let timer = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0;
    let init = initialize(
        EspWifiInitFor::Wifi,
        PeriodicTimer::new(timer.into()),
        Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
        &clocks,
    )
    .unwrap();

    let wifi = peripherals.WIFI;
    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();

    let timer_group0 = TimerGroup::new(peripherals.TIMG0, &clocks, None);
    let oneshot_timer = make_static!([OneShotTimer::new(timer_group0.timer0.into())]);
    esp_hal_embassy::init(&clocks, oneshot_timer);

    let config = Config::dhcpv4(Default::default());

    let seed = 1234; // very random, very secure seed

    // Init network stack
    let stack = &*make_static!(Stack::new(
        wifi_interface,
        config,
        make_static!(StackResources::<SOCKET_COUNT>::new()),
        seed
    ));

    spawner.spawn(connection(controller)).ok();
    spawner.spawn(net_task(&stack)).ok();

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    println!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            println!(
                "Point your browser to http://{}/",
                config.address.address()
            );
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    let server = make_static!(DefaultServer::new());
    let buffers = make_static!(TcpBuffers::<SERVER_SOCKETS, 2048, 2048>::new());
    let tcp: &'static Tcp<WifiDevice<'static, WifiStaDevice>, SERVER_SOCKETS, 2048, 2048> =
        make_static!(Tcp::new(stack, buffers));

    use edge_nal::TcpBind;
    let acceptor = tcp
        .bind(core::net::SocketAddr::V4(core::net::SocketAddrV4::new(
            core::net::Ipv4Addr::new(0, 0, 0, 0),
            80,
        )))
        .await
        .unwrap();
    server
        .run(acceptor, HttpHandler, Some(15_000))
        .await
        .unwrap();

    loop {}
}

struct HttpHandler;

impl<'b, T, const N: usize> Handler<'b, T, N> for HttpHandler
where
    T: Read + Write,
    T::Error: Send + Sync,
{
    type Error = Error<<T as ErrorType>::Error>;

    async fn handle(&self, connection: &mut Connection<'b, T, N>) -> Result<(), Self::Error> {
        println!("Got new connection");
        let headers = connection.headers()?;

        if !matches!(headers.method, Some(Method::Get)) {
            connection
                .initiate_response(405, Some("Method Not Allowed"), &[])
                .await?;
        } else if !matches!(headers.path, Some("/")) {
            connection
                .initiate_response(404, Some("Not Found"), &[])
                .await?;
        } else {
            connection
                .initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
                .await?;

            connection.write_all(b"Hello world!").await?;
        }

        Ok(())
    }
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    println!("start connection task");
    println!("Device capabilities: {:?}", controller.get_capabilities());
    loop {
        match esp_wifi::wifi::get_wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASSWORD.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            println!("Starting wifi");
            controller.start().await.unwrap();
            println!("Wifi started!");
        }
        println!("About to connect...");

        match controller.connect().await {
            Ok(_) => println!("Wifi connected!"),
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
