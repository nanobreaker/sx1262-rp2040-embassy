use embassy_rp::adc::{self, Async};
use embassy_time::{Duration, Ticker, Timer};
use heapless::Vec;
use lorawan_device::{AppEui, AppKey, AppSKey, DevAddr, DevEui, NewSKey};

use crate::config;
use crate::radio::lora_radio::LoraRadioError;
use crate::radio::Radio;
use crate::sensor::air_sensor::AirSensorError;
use crate::sensor::soil_sensor::SoilSensorError;
use crate::sensor::system_sensor::SystemSensorError;
use crate::sensor::Sensor;
use crate::storage::flash_storage::FlashStorageError;
use crate::storage::{Key, Storage};

#[derive(defmt::Format)]
pub enum DeviceError {
    Auth,
    AuthFailed,
    AuthJoinAttemptsExhausted,
    SessionExpired,
    NoAck,
    Duty,
    Send,
    Storage(FlashStorageError),
}

pub enum State {
    Boot,
    Auth,
    Duty,
    Send,
    Idle(u64),
}

impl Default for State {
    fn default() -> Self {
        Self::Boot
    }
}

pub struct Device<S0, S1, S2, R, D>
where
    S0: Sensor<18>,
    S1: Sensor<4>,
    S2: Sensor<11>,
    R: Radio,
    D: Storage,
{
    state: State,

    adc: adc::Adc<'static, Async>,

    system: S0,
    soil: S1,
    air: S2,
    radio: R,
    storage: D,

    data: Vec<u8, 33>,
    auth_attempt: u8,
}

impl<S0, S1, S2, R, D> Device<S0, S1, S2, R, D>
where
    S0: Sensor<18, Error = SystemSensorError>,
    S1: Sensor<4, Error = SoilSensorError>,
    S2: Sensor<11, Error = AirSensorError>,
    R: Radio<Error = LoraRadioError>,
    D: Storage<Error = FlashStorageError>,
{
    pub fn new(adc: adc::Adc<'static, Async>, board_sensor: S0, soil_sensor: S1, air_sensor: S2, transceiver: R, database: D) -> Self {
        Self {
            state: State::default(),
            adc,
            system: board_sensor,
            soil: soil_sensor,
            air: air_sensor,
            radio: transceiver,
            storage: database,
            data: Vec::new(),
            auth_attempt: 0,
        }
    }

    pub async fn run(mut self) {
        let mut ticker = Ticker::every(Duration::from_secs(60 * 10));
        loop {
            self.state = match self.state {
                State::Boot => match self.boot().await {
                    Ok(()) => State::Auth,
                    Err(_) => State::Idle(60 * 60),
                },
                State::Auth => match self.auth().await {
                    Ok(()) => State::Duty,
                    Err(DeviceError::AuthFailed) => State::Auth,
                    Err(_) => State::Idle(60 * 60),
                },
                State::Duty => match self.collect_data().await {
                    Ok(()) => State::Send,
                    Err(_) => State::Idle(60 * 60),
                },
                State::Send => match self.uplink().await {
                    Ok(()) | Err(DeviceError::NoAck) => State::Duty,
                    Err(DeviceError::SessionExpired) => State::Auth,
                    Err(_) => State::Idle(60 * 60),
                },
                State::Idle(secs) => {
                    Timer::after_secs(secs).await;
                    State::Auth
                }
            };
            ticker.next().await;
        }
    }

    pub async fn boot(&mut self) -> Result<(), DeviceError> {
        defmt::info!("Booting device");

        if (self.storage.mount().await).is_err() || config::Config::RESET {
            defmt::info!("Formating flash storage");

            match self.storage.format().await {
                Ok(()) => defmt::info!("Flash storage formatted"),
                Err(e) => defmt::error!("Flash storage format failed, {:?}", e),
            }
        }

        match self.system.verify().await {
            Ok(()) => defmt::info!("System sensors booted"),
            Err(e) => defmt::error!("System sensors boot failed, {:?}", e),
        }

        let _ = self.soil.on().await;
        match self.soil.verify().await {
            Ok(()) => defmt::info!("Soil sensor booted"),
            Err(e) => defmt::error!("Soil sensor boot failed, {:?}", e),
        }
        let _ = self.soil.off().await;

        match self.air.verify().await {
            Ok(()) => defmt::info!("Air sensor booted"),
            Err(e) => defmt::error!("Air sensor boot failed, {:?}", e),
        }

        Ok(())
    }

    pub async fn auth(&mut self) -> Result<(), DeviceError> {
        if let Some(keys) = self.get_session_keys().await {
            defmt::info!("Device was already authenticated - joining via ABP method");

            match self
                .radio
                .join(&lorawan_device::JoinMode::ABP {
                    nwkskey: keys.0,
                    appskey: keys.1,
                    devaddr: keys.2,
                })
                .await
            {
                Ok(_) => {
                    defmt::info!("ABP authentication ok");
                    Ok(())
                }
                Err(e) => {
                    defmt::error!("ABP authentication failed {:?}", e);
                    Err(DeviceError::Auth)
                }
            }
        } else {
            defmt::info!("Device was not authenticated - joining via OTAA method");

            match self
                .radio
                .join(&lorawan_device::JoinMode::OTAA {
                    deveui: DevEui::from(config::Config::DEV_EUI),
                    appeui: AppEui::from(config::Config::APP_EUI),
                    appkey: AppKey::from(config::Config::APP_KEY),
                })
                .await
            {
                Ok(keys) => {
                    defmt::info!("OTAA authentication ok");

                    match self.persist_session_keys(keys).await {
                        Ok(_) => Ok(()),
                        Err(_) => todo!(),
                    }
                }
                Err(e) => {
                    defmt::error!("OTAA authentication failed {:?}", e);

                    if self.auth_attempt > 9 {
                        self.auth_attempt = 0;
                        Err(DeviceError::AuthJoinAttemptsExhausted)
                    } else {
                        self.auth_attempt += 1;
                        Err(DeviceError::AuthFailed)
                    }
                }
            }
        }
    }

    pub async fn uplink(&mut self) -> Result<(), DeviceError> {
        let data: &[u8] = self.data.as_ref();

        defmt::info!("Sending uplink message with payload {=[u8]:#x}", data);

        match self.radio.uplink(data).await {
            Ok(fcnt_down) => {
                defmt::info!("Sent uplink, received downlink with fcount {=u32}", fcnt_down);
                Ok(())
            }
            Err(LoraRadioError::SessionExpired) => {
                defmt::error!("LoRaWAN session expired, re-authenticating");
                Err(DeviceError::SessionExpired)
            }
            Err(LoraRadioError::NoAck) => {
                defmt::error!("No acknoledgement received");
                // todo: is it worth retrying? might be expensive on power
                Err(DeviceError::NoAck)
            }
            Err(_) => {
                defmt::error!("Failed to send uplink");
                Err(DeviceError::Send)
            }
        }
    }

    async fn get_session_keys(&mut self) -> Option<(lorawan_device::NewSKey, lorawan_device::AppSKey, lorawan_device::DevAddr<[u8; 4]>)> {
        defmt::info!("Reading LoRaWAN session keys");

        let mut apps_key_buf = [0u8; 16];
        let apps_key = self
            .storage
            .get(&Key::AppSKey, &mut apps_key_buf)
            .await
            .map(|_size| AppSKey::from(apps_key_buf));

        let mut news_key_buf = [0u8; 16];
        let news_key = self
            .storage
            .get(&Key::NewSKey, &mut news_key_buf)
            .await
            .map(|_size| NewSKey::from(news_key_buf));

        let mut dev_addr_buf = [0u8; 4];
        let dev_addr = self
            .storage
            .get(&Key::DevAddr, &mut dev_addr_buf)
            .await
            .map(|_size| DevAddr::from(dev_addr_buf));

        if let (Some(news_key), Some(apps_key), Some(dev_addr)) = (news_key, apps_key, dev_addr) {
            Some((news_key, apps_key, dev_addr))
        } else {
            None
        }
    }

    async fn persist_session_keys(
        &mut self,
        keys: (lorawan_device::NewSKey, lorawan_device::AppSKey, lorawan_device::DevAddr<[u8; 4]>),
    ) -> Result<(), DeviceError> {
        defmt::info!("Persisting LoRaWAN session keys");

        let news_key = keys.0.as_ref();
        defmt::debug!("NewSKey {=[u8]}", news_key);
        if let Err(e) = self.storage.put(&Key::NewSKey, news_key).await {
            return Err(DeviceError::Storage(e));
        }

        let apps_key = keys.1.as_ref();
        defmt::debug!("AppSKey {=[u8]}", apps_key);
        if let Err(e) = self.storage.put(&Key::AppSKey, apps_key).await {
            return Err(DeviceError::Storage(e));
        }

        let dev_addr = keys.2.as_ref();
        defmt::debug!("DevAddr {=[u8]}", dev_addr);
        if let Err(e) = self.storage.put(&Key::DevAddr, dev_addr).await {
            return Err(DeviceError::Storage(e));
        }

        Ok(())
    }

    pub async fn collect_data(&mut self) -> Result<(), DeviceError> {
        self.data.clear();

        match self.system.probe(&mut self.adc).await {
            Ok(probe_data) => self.data.extend_from_slice(&probe_data).unwrap(),
            Err(e) => {
                defmt::error!("System sensors probe failed {:?}", e);
                return Err(DeviceError::Duty);
            }
        }

        let _ = self.soil.on().await;
        match self.soil.probe(&mut self.adc).await {
            Ok(probe_data) => self.data.extend_from_slice(&probe_data).unwrap(),
            Err(e) => {
                defmt::error!("Soil sensor probe failed {:?}", e);
                return Err(DeviceError::Duty);
            }
        }
        let _ = self.soil.off().await;

        // todo: there is a bug with air sensor
        // after wake up it might not send ack during i2c communication and result in error
        // hence for now air sensor will always be powered
        // let _ = self.air.on().await;
        match self.air.probe(&mut self.adc).await {
            Ok(probe_data) => self.data.extend_from_slice(&probe_data).unwrap(),
            Err(e) => {
                defmt::error!("Air sensor probe failed {:?}", e);
                return Err(DeviceError::Duty);
            }
        }
        // let _ = self.air.off().await;

        Ok(())
    }
}
