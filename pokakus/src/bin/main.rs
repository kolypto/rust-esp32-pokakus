#![no_std]
#![no_main]
#![deny(clippy::mem_forget)]
#![deny(clippy::large_stack_frames)]
extern crate alloc;
use {esp_backtrace as _, esp_println as _};
esp_bootloader_esp_idf::esp_app_desc!();

use defmt;
use esp_hal::{
    clock::CpuClock,
    timer::timg::TimerGroup,
    rng::Rng,
    interrupt::software::SoftwareInterruptControl,
    gpio,
};

use embassy_executor::Spawner;
use embassy_time::{
    Duration, Timer
};

use pokakus::{
    self,
    button::wait_for_button_click,
};


#[allow(clippy::large_stack_frames)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // Init allocator: 64K in reclaimed memory + 66K in default RAM
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 72 * 1024);

    // CPU Clock: WiFi in ESP32 requires a fast CPU
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // Init Embassy the usual way
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
    defmt::info!("Embassy initialized!");

    // Init GPIO: button
    let button = gpio::Input::new(peripherals.GPIO9, gpio::InputConfig::default());

    // Init GPIO: LED
    let led = gpio::Output::new(peripherals.GPIO8, gpio::Level::High, gpio::OutputConfig::default());

    // TODO: Spawn some tasks
    spawner.must_spawn(pokakus::button::task_button_clicks(button));
    spawner.must_spawn(led_task(led));

    loop {
        defmt::info!("Running...");
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Task: blinks LED
#[embassy_executor::task]
async fn led_task(mut led: gpio::Output<'static>) {
    loop {
        // Wait for button press event
        wait_for_button_click().await;

        // Blink 3 times
        for _ in 0..6 {
            led.toggle();
            Timer::after(Duration::from_millis(100)).await;
        }
    }
}
