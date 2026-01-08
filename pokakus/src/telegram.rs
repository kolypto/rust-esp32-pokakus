use defmt;
use heapless::{
    String,
};

use esp_hal::{rng::Rng};
use reqwless::{
    client::{HttpClient, TlsConfig},
    headers::ContentType, request::RequestBuilder
};
use serde::Serialize;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use embassy_sync::{
    channel::Channel,
    blocking_mutex::raw::CriticalSectionRawMutex,
};

// Bot token
const BOT_TOKEN: &str = env!("TELEGRAM_BOT_TOKEN");
const SEND_TO: &str = env!("TELEGRAM_SEND_TO");

/// Send a message
pub fn send_telegram_message(msg: &str){
    // We only got a reference. To take ownership, we need a copy.
    let owned: String<32> = String::try_from(msg).unwrap();
    match MESSAGES_QUEUE.try_send(owned) {
        Ok(()) => (),
        Err(_) => defmt::error!("Queue full: cannot send message"),
    }
}

/// Messages queue
static MESSAGES_QUEUE: Channel<CriticalSectionRawMutex, String::<32>, 8> = Channel::new();

// Task: send messages to Telegram
#[embassy_executor::task()]
pub async fn task_telegram_sender(stack: embassy_net::Stack<'static>) {
    let send_to: i64 = SEND_TO.parse().expect("Failed to parse SEND_TO");

    // Input
    let receiver = MESSAGES_QUEUE.receiver();
    loop {
        let message = receiver.receive().await;

        // Wait for network
        // TODO: timeout, warning?
        stack.wait_config_up().await;

        // Request
        defmt::debug!("Telegram: sending message...");
        let led_status = crate::led_op::Status::new();
        match telegram_send_message(stack, send_to, message.as_str()).await {
            Ok(()) => {
                defmt::info!("Message sent!");
                led_status.success();
            },
            Err(e) => {
                defmt::error!("Failed to send: {:?}", defmt::Debug2Format(&e));
                led_status.failure();
            }
        }
    }
}

// Send a message
async fn telegram_send_message(stack: embassy_net::Stack<'_>, send_to: i64, message: &str) -> Result<(), TelegramSendMessageError> {
    // TLS needs a random value
    let rng = Rng::new();  // it's ok: nothing's really initialized
    let tls_seed = {
        let mut bytes = [0; 8];
        rng.read(&mut bytes);
        u64::from_le_bytes(bytes)
    };

    // Init TLS.
    // Quirks:
    // 1. TLS recommends that the rx buffer is at least 16640 bytes long: because this is the size of the biggest packet (2^14+256).
    // 2. By default, the `reqwless` crate uses `embedded-tls`, which is limited to algorithms that can run entirely on the stack.
    //    To enable all algorithms, add the "alloc" feature.
    //    To check whether the default list suffices with your server:
    //    $ vopenssl s_client -tls1_3 -ciphersuites TLS_AES_128_GCM_SHA256 -sigalgs "ECDSA+SHA256:ECDSA+SHA384:ed25519" -connect api.telegram.org:443
    // 3. On ESP32, you can further speed up TLS by using the RSA peripheral.
    //    TODO: see `esp-mbedtls`
    let (mut rx_buffer, mut tx_buffer) = ([0; 16640], [0; 16640]);
    let tls = TlsConfig::new(
        tls_seed,
        &mut rx_buffer,
        &mut tx_buffer,
        reqwless::client::TlsVerify::None,
    );

    let tcp_state = TcpClientState::<1, 4096, 4096>::new();
    let tcp = TcpClient::new(stack, &tcp_state);
    let dns = DnsSocket::new(stack);
    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);

    // Data
    let mut url: String<128> = String::new();
    use core::fmt::Write;
    write!(url, "https://api.telegram.org/bot{}/sendMessage", BOT_TOKEN).unwrap();
    // write!(url, "https://jsonplaceholder.typicode.com/posts").unwrap();  // for testing
    // let mut body: String<256> = String::new();
    // write!(body, r#"{{"chat_id":{},"text":"{}"}}"#, send_to, message).unwrap();
    let msg = TelegramMessageInput {
        chat_id: send_to,
        text: message,
    };
    let mut body_buf = [0u8; 256];
    let _body_len = serde_json_core::to_slice(&msg, &mut body_buf)?;

    // Request
    let mut buf = [0; 4096];
    let mut req = client.request(reqwless::request::Method::POST, url.as_str())
        .await?
        .content_type(ContentType::ApplicationJson)
        .body(body_buf.as_slice());
    let resp = req.send(&mut buf)
        .await?;

    // Read response
    let response = resp.body().read_to_end()
        .await?;
    let resp_text = core::str::from_utf8(&response)
        .map_err(|_| TelegramSendMessageError::ResponseError)?;

    // Check for success
    if !resp_text.contains(r#""ok":true"#) {
        defmt::error!("Telegram failed: {}", resp_text);
        return Err(TelegramSendMessageError::ResponseError)
    }

    // Ok
    return Ok(())
}


// Error handling: only return as much info as the caller needs to have.
// Everything else: log, don't return.
// "Log generously, return sparingly."
#[derive(Debug, defmt::Format)]
pub enum TelegramSendMessageError {
    InvalidArguments,
    RequestError(reqwless::Error),
    ResponseError,  // see logs
}

// Auto-convert with From impls
impl From<reqwless::Error> for TelegramSendMessageError {
    fn from(e: reqwless::Error) -> Self {
        TelegramSendMessageError::RequestError(e)
    }
}
impl From<serde_json_core::ser::Error> for TelegramSendMessageError {
    fn from(_: serde_json_core::ser::Error) -> Self {
        TelegramSendMessageError::InvalidArguments
    }
}


#[derive(Serialize, defmt::Format)]
struct TelegramMessageInput<'a> {
    chat_id: i64,
    text: &'a str,
}


/* Telegram API:
 * $ http POST 'https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage' chat_id:=${TELEGRAM_SEND_TO} text="hey"
 * { "ok":true,
 *   "result":{
 *     "message_id":39,
 *     "from":{"id":6415095545,"is_bot":true,"first_name":"....","username":"...bot"},
 *     "chat":{"id":691814383,"first_name":"...","last_name":"...","username":"...","type":"private"},
 *     "date":1767364636,
 *     "text":"hi there"
 *   }
 * }
 */
