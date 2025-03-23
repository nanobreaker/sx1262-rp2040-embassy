use embassy_rp::{
    clocks::RoscRng,
    gpio::{Input, Level, Output, Pin, Pull},
    peripherals::SPI1,
    spi::{self, Config, Spi},
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::{
    iv::GenericSx126xInterfaceVariant,
    lorawan_radio::LorawanRadio,
    sx126x::{self, Sx1262, Sx126x, TcxoCtrlVoltage},
    LoRa,
};
use lorawan_device::{
    async_device::{self, EmbassyTimer},
    region, AppEui, AppKey, AppSKey, DevAddr, DevEui, JoinMode, NewSKey,
};
use lorawan_device::{
    async_device::{JoinResponse, SendResponse},
    default_crypto::DefaultFactory as Crypto,
};

use crate::{config, error::Error, Device, TransceiverResources};

type Sx1262Radio = LorawanRadio<
    Sx126x<
        ExclusiveDevice<Spi<'static, SPI1, spi::Async>, Output<'static>, Delay>,
        GenericSx126xInterfaceVariant<Output<'static>, Input<'static>>,
        Sx1262,
    >,
    Delay,
    14,
>;

pub(crate) type RadioDevice = lorawan_device::async_device::Device<Sx1262Radio, Crypto, EmbassyTimer, RoscRng>;

pub trait Transceiver {
    async fn join_otaa(&mut self) -> Result<(NewSKey, AppSKey, DevAddr<[u8; 4]>), Error>;
    async fn join_abp(&mut self, keys: (NewSKey, AppSKey, DevAddr<[u8; 4]>)) -> Result<(), Error>;
    async fn uplink(&mut self, payload: &[u8]) -> Result<SendResponse, Error>;
}

impl Transceiver for RadioDevice {
    async fn join_otaa(&mut self) -> Result<(NewSKey, AppSKey, DevAddr<[u8; 4]>), Error> {
        match self
            .join(&JoinMode::OTAA {
                deveui: DevEui::from(config::Config::DEV_EUI),
                appeui: AppEui::from(config::Config::APP_EUI),
                appkey: AppKey::from(config::Config::APP_KEY),
            })
            .await
        {
            Ok(JoinResponse::JoinSuccess) => {
                let session = self.get_session().unwrap();
                Ok((session.nwkskey, session.appskey, session.devaddr))
            }
            Ok(JoinResponse::NoJoinAccept) => Err(Error::JoinAcceptMissing),
            Err(e) => Err(Error::LoRaWAN(e)),
        }
    }

    async fn join_abp(&mut self, keys: (NewSKey, AppSKey, DevAddr<[u8; 4]>)) -> Result<(), Error> {
        match self
            .join(&JoinMode::ABP {
                nwkskey: keys.0,
                appskey: keys.1,
                devaddr: keys.2,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(Error::LoRaWAN(e)),
        }
    }

    async fn uplink(&mut self, payload: &[u8]) -> Result<SendResponse, Error> {
        self.send(payload, 1, true).await.map_err(|e| e.into())
    }
}

impl Device<TransceiverResources> for RadioDevice {
    type Info = ();
    async fn prepare(r: TransceiverResources) -> Result<Self, Error> {
        let nss = Output::new(r.pin_3.degrade(), Level::High);
        let reset = Output::new(r.pin_15.degrade(), Level::High);
        let dio1 = Input::new(r.pin_20.degrade(), Pull::None);
        let busy = Input::new(r.pin_2.degrade(), Pull::None);
        let spi = Spi::new(r.spi1, r.pin_10, r.pin_11, r.pin_12, r.dma_ch0, r.dma_ch1, Config::default());
        let spi_bus = ExclusiveDevice::new(spi, nss, Delay)?;
        let sx1262_config = sx126x::Config {
            chip: Sx1262,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
            use_dcdc: true,
            rx_boost: false,
        };
        let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None)?;
        let lora = LoRa::new(Sx126x::new(spi_bus, iv, sx1262_config), true, Delay).await?;
        let mut radio: LorawanRadio<_, _, 14> = lora.into();
        radio.set_rx_window_lead_time(config::Config::RX_WINDOW_LEAD_TIME);
        radio.set_rx_window_buffer(config::Config::RX_WINDOW_BUFFER);
        let region: region::Configuration = region::Configuration::new(config::Config::LORAWAN_REGION);
        let lora_radio: async_device::Device<_, Crypto, _, _> =
            async_device::Device::new(region, radio, EmbassyTimer::new(), embassy_rp::clocks::RoscRng);

        Ok(lora_radio)
    }

    async fn init(&mut self) -> Result<Self::Info, Error> {
        Ok(())
    }
}
