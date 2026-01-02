use defmt;
use esp_hal::{
    gpio,
};

use embassy_executor;

use embassy_sync::{
    signal::Signal,
    blocking_mutex::raw::CriticalSectionRawMutex,
};
use embassy_futures::select;
use embassy_time::{Duration, Instant, Timer};


#[derive(defmt::Format, Clone, Copy)]
pub enum LedState {
    PresenceBlink,      // Up and running
    PatientBlink,       // In Progress: WiFi connecting
    RapidBlink,         // In Progress: HTTP sending
    Success,            // Result: Success
    Failure,            // Result: Error
    ViolentBlink,       // Error state (failing)
}

// Global signal - other tasks write to this
//
// Signal's perfect here because it sends a notification *immediately* when the state changes.
// This is unlike a mutex-guarded static: it notifies of an incoming change.
// The Signal, however, isn't holding the state. It's just delivering updates.
//
// Also see: `Watch`. Watch stores the value AND notifies.
static LED_STATE: Signal<CriticalSectionRawMutex, LedState> = Signal::new();

/// Change the LED's behavior from anywhere
pub fn set_led_state(state: LedState) {
    LED_STATE.signal(state);
}


/// Task: blinks LED
#[embassy_executor::task]
pub async fn led_task(led: gpio::Output<'static>) {
    let mut led = ActiveLowLed{ pin: led };
    let mut current_state = LedState::PatientBlink;

    loop {
        // Decide on the blinking pattern:
        // - on_duration: stay ON
        // - off_duration: stay OFF
        // - hold: hold the state for this long
        let (on_duration, off_duration, hold) = match current_state {
            LedState::PresenceBlink     => (Duration::from_millis(  30), Duration::from_millis(3000), None),
            LedState::PatientBlink      => (Duration::from_millis( 500), Duration::from_millis(1000), None),
            LedState::RapidBlink        => (Duration::from_millis( 100), Duration::from_millis( 100), None),
            LedState::ViolentBlink      => (Duration::from_millis(  30), Duration::from_millis(  70), None),
            LedState::Success           => (Duration::from_millis(3000), Duration::from_millis(   0), Some(Duration::from_secs(3))),
            LedState::Failure           => (Duration::from_millis(  30), Duration::from_millis(  70), Some(Duration::from_secs(3))),
        };

        // Blink pattern
        let pattern = [
            (true, on_duration),
            (false, off_duration),
        ];

        // Hold the state?
        // Problem: if a task fails, it sets `ViolentBlink` — but then the main task will immediately set `PresenceBlink` and reset it.
        // Solution: some states "hold" the blinking pattern for a while.
        if let Some(hold) = hold {
            let delay_start = Instant::now();

            // Keep blinking until it's passed
            while delay_start.elapsed() < hold {
                // Blink the whole pattern
                for (state, dur) in pattern {
                    led.set(state);
                    Timer::after(dur).await;
                }
            }

            // Update state.
            if let Some(v) = LED_STATE.try_take() {
                current_state = v;
            }
            continue;
        }

        // Blink, but interrupt as soon as another signal comes
        for (state, dur) in pattern {
            led.set(state);

            // Sleep, but interrupt if a state change comes in
            match select::select(Timer::after(dur), LED_STATE.wait()).await {
                select::Either::First(_) => { }  // timer expired
                select::Either::Second(new_state) => {
                    // State changed!
                    current_state = new_state;
                    defmt::info!("LED state changed to {:?}", new_state);
                }
            }
        }
    }
}



// Wrapper for LED on GPIO, Active-LOW
struct ActiveLowLed {
    pin: gpio::Output<'static>,
}

impl ActiveLowLed {
    fn turn_on(&mut self) {
        self.pin.set_low();
    }

    fn turn_off(&mut self) {
        self.pin.set_high();
    }

    fn set(&mut self, on: bool){
        if on {
            self.turn_on();
        } else {
            self.turn_off();
        }
    }
}
