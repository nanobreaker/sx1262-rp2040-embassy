use embassy_rp::{
    clocks::RoscRng,
    gpio::{Input, Output},
    peripherals::SPI1,
    spi::{self, Spi},
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::{
    iv::GenericSx126xInterfaceVariant,
    lorawan_radio::LorawanRadio,
    sx126x::{Sx1262, Sx126x},
};
use lorawan_device::{
    async_device::{Device, EmbassyTimer},
    JoinMode,
};
use lorawan_device::{
    async_device::{JoinResponse, SendResponse},
    default_crypto::DefaultFactory as Crypto,
};

use crate::error::Error;

type Sx1262Radio = LorawanRadio<
    Sx126x<
        ExclusiveDevice<Spi<'static, SPI1, spi::Async>, Output<'static>, Delay>,
        GenericSx126xInterfaceVariant<Output<'static>, Input<'static>>,
        Sx1262,
    >,
    Delay,
    14,
>;

pub(crate) type RadioDevice = Device<Sx1262Radio, Crypto, EmbassyTimer, RoscRng>;

pub trait Transceiver {
    async fn auth(&mut self, mode: &JoinMode) -> Result<JoinResponse, Error>;
    async fn uplink(&mut self, payload: &[u8]) -> Result<SendResponse, Error>;
}

impl Transceiver for RadioDevice {
    async fn auth(&mut self, mode: &JoinMode) -> Result<JoinResponse, Error> {
        self.join(mode).await.map_err(|e| e.into())
    }

    async fn uplink(&mut self, payload: &[u8]) -> Result<SendResponse, Error> {
        self.send(payload, 1, true).await.map_err(|e| e.into())
    }
}

impl crate::device::Device<(), ()> for RadioDevice {
    async fn init(&mut self) -> Result<(), Error> {
        Ok(())
    }

    async fn info(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
