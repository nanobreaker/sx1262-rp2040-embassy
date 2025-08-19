use lorawan_device::{AppSKey, DevAddr, JoinMode, NewSKey};

pub mod lora_radio;

// Trait to represent basic functionality of lora radio.
// Be able to join the network, support both otaa and abp methods.
// Send uplink messages.
pub trait Radio {
    /// Error type representation, left up to the implementor
    type Error;

    // Join the LoRaWAN network
    async fn join(&mut self, mode: &JoinMode) -> Result<(NewSKey, AppSKey, DevAddr<[u8; 4]>), Self::Error>;

    // Send uplink message, in case of success we receive u32 which represent FcntDown
    async fn uplink(&mut self, payload: &[u8]) -> Result<u32, Self::Error>;
}
