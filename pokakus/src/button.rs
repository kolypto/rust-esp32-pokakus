use defmt;
use esp_hal::{
    gpio,
};

use embassy_sync::{
    channel::Channel,
    blocking_mutex::raw::CriticalSectionRawMutex,
};



/// Wait until the button's clicked.
//
// NOTE: exposed as a function to hide implementation detail
pub async fn wait_for_button_click() {
    BUTTON_CLICKS.receive().await;
}

/// Channel: button clicks.
/// An empty message is sent along every time the button's clicked.
//
// A channel will send separate events.
static BUTTON_CLICKS: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();

/// Task: listen to button clicks
#[embassy_executor::task]
pub async fn task_button_clicks(mut button: gpio::Input<'static>) {
    loop {
        // Wait for press (high→low transition)
        button.wait_for_falling_edge().await;

        // Debounce.
        // Verify button is still pressed (not a bounce)
        embassy_time::Timer::after_millis(20).await;
        if button.is_low() {
            // Send ONE event
            defmt::debug!("Button clicked");
            let _ = BUTTON_CLICKS.try_send(()); // Non-blocking

            // Wait for it to be released. Don't send any more events.
            button.wait_for_high().await;
        }
    }
}
