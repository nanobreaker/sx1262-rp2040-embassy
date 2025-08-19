use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{self, Config, Spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::lorawan_radio::LorawanRadio;
use lora_phy::mod_params::RadioError;
use lora_phy::sx126x::{self, Sx1262, Sx126x, TcxoCtrlVoltage};
use lora_phy::LoRa;
use lorawan_device::async_device::{self, EmbassyTimer, JoinResponse, SendResponse};
use lorawan_device::{region, AppSKey, DevAddr, JoinMode, NewSKey};

use crate::radio::Radio;
use crate::{config, RadioRes};

type SX1262 = lorawan_device::async_device::Device<
    LorawanRadio<
        Sx126x<
            ExclusiveDevice<Spi<'static, SPI1, spi::Async>, Output<'static>, Delay>,
            GenericSx126xInterfaceVariant<Output<'static>, Input<'static>>,
            Sx1262,
        >,
        Delay,
        14,
    >,
    EmbassyTimer,
    RoscRng,
>;

#[derive(defmt::Format)]
pub enum LoraRadioError {
    NoJoinAccept,
    NoAck,
    SessionExpired,
    LoRaWAN(lorawan_device::async_device::Error<lora_phy::lorawan_radio::Error>),
}

pub struct LoraRadio {
    radio: SX1262,
}

impl LoraRadio {
    pub async fn try_new(r: RadioRes) -> Result<Self, RadioError> {
        let nss = Output::new(r.cs, Level::High);
        let reset = Output::new(r.rst, Level::High);
        let dio1 = Input::new(r.dio1, Pull::None);
        let busy = Input::new(r.busy, Pull::None);
        let spi = Spi::new(r.spi1, r.clk, r.mosi, r.miso, r.dma_ch0, r.dma_ch1, Config::default());
        let spi_bus = ExclusiveDevice::new(spi, nss, Delay);
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
        let lora_radio: async_device::Device<_, _, _> =
            async_device::Device::new(region, radio, EmbassyTimer::new(), embassy_rp::clocks::RoscRng);

        Ok(Self { radio: lora_radio })
    }
}

impl Radio for LoraRadio {
    type Error = LoraRadioError;

    async fn join(&mut self, mode: &JoinMode) -> Result<(NewSKey, AppSKey, DevAddr<[u8; 4]>), Self::Error> {
        match self.radio.join(mode).await {
            Ok(JoinResponse::JoinSuccess) => {
                let session = self.radio.get_session().unwrap();
                Ok((session.nwkskey, session.appskey, session.devaddr))
            }
            Ok(JoinResponse::NoJoinAccept) => Err(LoraRadioError::NoJoinAccept),
            Err(err) => Err(LoraRadioError::LoRaWAN(err)),
        }
    }

    async fn uplink(&mut self, payload: &[u8]) -> Result<u32, Self::Error> {
        match self.radio.send(payload, 1, true).await {
            Ok(response) => match response {
                SendResponse::DownlinkReceived(fcnt_down) => Ok(fcnt_down),
                SendResponse::SessionExpired => Err(LoraRadioError::SessionExpired),
                SendResponse::NoAck | SendResponse::RxComplete => Err(LoraRadioError::NoAck),
            },
            Err(err) => Err(LoraRadioError::LoRaWAN(err)),
        }
    }
}
