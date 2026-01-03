use defmt;
use heapless::{
    String,
};

use esp_hal::{rng::Rng};
use reqwless::{
    client::{HttpClient, TlsConfig},
    headers::ContentType, request::RequestBuilder
};
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};

// Bot token
const BOT_TOKEN: &str = env!("TELEGRAM_BOT_TOKEN");
const SEND_TO: &str = env!("TELEGRAM_SEND_TO");

// Task: send messages to Telegram
#[embassy_executor::task()]
pub async fn task_telegram_sender(stack: embassy_net::Stack<'static>) {
    // TLS needs a random value
    let rng = Rng::new();
    let tls_seed = {
        let mut bytes = [0; 8];
        rng.read(&mut bytes);
        u64::from_le_bytes(bytes)
    };

    // Wait for network
    stack.wait_config_up().await;

    // Request
    if let Err(e) = telegram_send_message(stack, tls_seed, "💩").await {
        defmt::error!("Failed to send: {:?}", defmt::Debug2Format(&e));
    }
}

// Send a message
async fn telegram_send_message(stack: embassy_net::Stack<'_>, tls_seed: u64, message: &str) -> anyhow::Result<(), reqwless::Error> {
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
    let mut body: String<256> = String::new();
    write!(body, r#"{{"chat_id":{},"text":"{}"}}"#, SEND_TO, message).unwrap();

    // Request
    let mut buf = [0; 4096];
    let mut req = client.request(reqwless::request::Method::POST, url.as_str())
        .await?
        .content_type(ContentType::ApplicationJson)
        .body(body.as_bytes());
    let resp = req.send(&mut buf)
        .await?;

    // Read response
    let response = resp.body().read_to_end().await?;
    let resp_text = core::str::from_utf8(&response)?;

    // Check for success
    if resp_text.contains(r#""ok":true"#) {
        defmt::info!("Message sent!");
        return Ok(())
    } else {
        defmt::error!("Failed: {}", resp_text);
    }

    return Ok(())
}


// #[derive(Debug, defmt::Format)]
// pub enum TelegramSendMessageError {
//     RequestFailed(reqwless::Error),
//     InvalidResponse(core::error::Error),
//     // Add context variants if needed
//     HeaderParseFailed(reqwless::Error),
//     BodyReadFailed(reqwless::Error),
// }


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
