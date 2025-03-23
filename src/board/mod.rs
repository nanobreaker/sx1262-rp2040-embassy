use embassy_time::{Duration, Ticker, Timer};
use heapless::Vec;
use lorawan_device::{async_device::SendResponse, AppSKey, DevAddr, NewSKey};

use crate::{
    air_sensor::AirSensor,
    board_sensor::BoardSensor,
    database::{Database, DbKey, EkvDatabase},
    device::Device,
    error::Error,
    sensor::Sensor,
    soil_sensor::SoilSensor,
    transceiver::{RadioDevice, Transceiver},
};

enum State {
    Init,
    Auth,
    Duty,
    Send,
    Wait(u64),
}

pub struct Board {
    state: State,

    board_sensor: BoardSensor,
    air_sensor: AirSensor,
    soil_sensor: SoilSensor,
    transceiver: RadioDevice,
    database: EkvDatabase,

    sensors_data: Vec<u8, 37>,
    join_attempt: u8,
    reset: bool,
}

impl Board {
    pub async fn run(mut self) {
        let mut ticker = Ticker::every(Duration::from_secs(30));
        loop {
            self.state = match self.state {
                State::Init => match self.init().await {
                    Ok(()) => State::Auth,
                    Err(_) => State::Wait(60 * 10),
                },
                State::Auth => match self.auth().await {
                    Ok(()) => State::Duty,
                    Err(Error::JoinLimitReached) => State::Wait(60 * 60),
                    Err(_) => State::Wait(60 * 10),
                },
                State::Duty => match self.collect_data().await {
                    Ok(()) => State::Send,
                    Err(_) => State::Wait(60 * 10),
                },
                State::Send => match self.uplink().await {
                    Ok(()) => State::Duty,
                    Err(Error::JoinSessionExpired) => State::Auth,
                    Err(_) => State::Wait(60 * 10),
                },
                State::Wait(secs) => {
                    Timer::after_secs(secs).await;
                    State::Init
                }
            };
            ticker.next().await;
        }
    }

    pub async fn init(&mut self) -> Result<(), Error> {
        let mut errors = Vec::<Error, 4>::new();

        defmt::info!("Initializing board");

        let _ = self
            .board_sensor
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized board sensor {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = self
            .air_sensor
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized air sensor {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = self
            .soil_sensor
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized soil sensor {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = self
            .transceiver
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized transceiver {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = self
            .database
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized flash memory {:?}", i))
            .map_err(|e| errors.push(e));

        if self.reset {
            return self.database.format().await.map_err(Error::Flash);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            defmt::error!("Error(s) during board initialization");
            for error in errors {
                defmt::error!("{:?}", error);
            }
            Err(Error::Init)
        }
    }

    pub async fn auth(&mut self) -> Result<(), Error> {
        defmt::info!("Verifying if device was already authenticated");

        if let Some(keys) = self.get_session_keys().await {
            defmt::info!("Device is already authenticated");

            match self.transceiver.join_abp(keys).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    defmt::info!("Failed to join lorawan network {:?}", e);

                    Err(Error::JoinFailed)
                }
            }
        } else {
            defmt::info!("Authenticating device");

            match self.transceiver.join_otaa().await {
                Ok((news_key, apps_key, dev_addr)) => {
                    defmt::info!("Joined lorawan network");

                    self.persist_session_keys((news_key, apps_key, dev_addr)).await?;
                    Ok(())
                }
                Err(e) => {
                    defmt::info!("Failed to join lorawan network {:?}", e);

                    if self.join_attempt > 9 {
                        self.join_attempt = 0;
                        Err(Error::JoinLimitReached)
                    } else {
                        self.join_attempt += 1;
                        Err(Error::JoinFailed)
                    }
                }
            }
        }
    }

    async fn get_session_keys(&mut self) -> Option<(lorawan_device::NewSKey, lorawan_device::AppSKey, lorawan_device::DevAddr<[u8; 4]>)> {
        defmt::info!("Reading keys");

        let mut apps_key_buf = [0u8; 16];
        let apps_key = self
            .database
            .get(&DbKey::AppSKey, &mut apps_key_buf)
            .await
            .map(|_size| AppSKey::from(apps_key_buf));

        let mut news_key_buf = [0u8; 16];
        let news_key = self
            .database
            .get(&DbKey::NewSKey, &mut news_key_buf)
            .await
            .map(|_size| NewSKey::from(news_key_buf));

        let mut dev_addr_buf = [0u8; 4];
        let dev_addr = self
            .database
            .get(&DbKey::DevAddr, &mut dev_addr_buf)
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
    ) -> Result<(), Error> {
        defmt::info!("Persisting keys");

        let news_key = keys.0.as_ref();
        defmt::info!("NewSKey {=[u8]}", news_key);
        self.database.put(&DbKey::NewSKey, news_key).await?;

        let apps_key = keys.1.as_ref();
        defmt::info!("AppSKey {=[u8]}", apps_key);
        self.database.put(&DbKey::AppSKey, apps_key).await?;

        let dev_addr = keys.2.as_ref();
        defmt::info!("DevAddr {=[u8]}", dev_addr);
        self.database.put(&DbKey::DevAddr, dev_addr).await?;

        Ok(())
    }

    pub async fn collect_data(&mut self) -> Result<(), Error> {
        let mut errors = Vec::<Error, 4>::new();
        self.sensors_data.clear();

        defmt::info!("Collecting data from sensors");

        let _ = self
            .board_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Board {:?}", d))
            .map(|d| {
                let bytes: [u8; 18] = d.into();
                self.sensors_data.extend_from_slice(&bytes).expect("should extend");
            })
            .map_err(|e| errors.push(e));

        let _ = self
            .air_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Air {:?}", d))
            .map(|d| {
                let bytes: [u8; 11] = d.into();
                self.sensors_data.extend_from_slice(&bytes).expect("should extend");
            })
            .map_err(|e| errors.push(e));

        let _ = self
            .soil_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Soil {:?}", d))
            .map(|d| {
                let bytes: [u8; 8] = d.into();
                self.sensors_data.extend_from_slice(&bytes).expect("should extend");
            })
            .map_err(|e| errors.push(e));

        if errors.is_empty() {
            Ok(())
        } else {
            defmt::error!("Error(s) during data collection");
            for error in errors {
                defmt::error!("{:?}", error);
            }
            Err(Error::Duty)
        }
    }

    pub async fn uplink(&mut self) -> Result<(), Error> {
        let data: &[u8] = self.sensors_data.as_ref();

        defmt::info!("Sending uplink message");
        defmt::info!("Payload {=[u8]:#x}", data);

        match self.transceiver.uplink(data).await {
            Ok(SendResponse::DownlinkReceived(f_count)) => {
                defmt::info!("Sent uplink, received downlink with fcount {=u32}", f_count);
                Ok(())
            }
            Ok(SendResponse::NoAck) => {
                defmt::warn!("Sent uplink, but no ack received");
                Ok(())
            }
            Ok(SendResponse::RxComplete) => {
                defmt::info!("RxComplete");
                Ok(())
            }
            Ok(SendResponse::SessionExpired) => {
                defmt::error!("Failed to send uplink, session expired");
                Err(Error::JoinSessionExpired)
            }
            Err(e) => {
                defmt::error!("Failed to send uplink message: {:?}", e);
                Err(e)
            }
        }
    }
}

pub struct BoardBuilder {
    board_sensor: Option<BoardSensor>,
    air_sensor: Option<AirSensor>,
    soil_sensor: Option<SoilSensor>,
    radio: Option<RadioDevice>,
    database: Option<EkvDatabase>,
}

impl BoardBuilder {
    pub fn new() -> BoardBuilder {
        BoardBuilder {
            board_sensor: None,
            air_sensor: None,
            soil_sensor: None,
            radio: None,
            database: None,
        }
    }

    pub fn with_board_sensor(mut self, board_sensor: BoardSensor) -> BoardBuilder {
        self.board_sensor = Some(board_sensor);
        self
    }

    pub fn with_air_sensor(mut self, air_sensor: AirSensor) -> BoardBuilder {
        self.air_sensor = Some(air_sensor);
        self
    }

    pub fn with_soil_sensor(mut self, soil_sensor: SoilSensor) -> BoardBuilder {
        self.soil_sensor = Some(soil_sensor);
        self
    }

    pub fn with_radio(mut self, radio: RadioDevice) -> BoardBuilder {
        self.radio = Some(radio);
        self
    }

    pub fn with_database(mut self, database: EkvDatabase) -> BoardBuilder {
        self.database = Some(database);
        self
    }

    pub fn build(self) -> Result<Board, crate::error::Error> {
        if let (Some(board_sensor), Some(air_sensor), Some(soil_sensor), Some(transceiver), Some(database)) =
            (self.board_sensor, self.air_sensor, self.soil_sensor, self.radio, self.database)
        {
            Ok(Board {
                state: State::Init,
                board_sensor,
                air_sensor,
                soil_sensor,
                transceiver,
                database,
                sensors_data: Vec::<u8, 37>::new(),
                join_attempt: 0,
                reset: false,
            })
        } else {
            Err(Error::Init)
        }
    }
}
