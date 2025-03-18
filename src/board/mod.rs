use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use heapless::Vec;
use lorawan_device::async_device::SendResponse;
use static_cell::StaticCell;

use crate::{
    air_sensor::AirSensor,
    board_sensor::BoardSensor,
    database::{Database, EkvDatabase},
    device::Device,
    error::Error,
    sensor::Sensor,
    soil_sensor::SoilSensor,
    transceiver::{self, RadioDevice, Transceiver},
};

static BOARD_SENSOR_CELL: StaticCell<Mutex<ThreadModeRawMutex, BoardSensor>> = StaticCell::new();
static AIR_SENSOR_CELL: StaticCell<Mutex<ThreadModeRawMutex, AirSensor>> = StaticCell::new();
static SOIL_SENSOR_CELL: StaticCell<Mutex<ThreadModeRawMutex, SoilSensor>> = StaticCell::new();
static RADIO_CELL: StaticCell<Mutex<ThreadModeRawMutex, RadioDevice>> = StaticCell::new();

pub struct Board {
    pub board_sensor: &'static mut Mutex<ThreadModeRawMutex, BoardSensor>,
    pub air_sensor: &'static mut Mutex<ThreadModeRawMutex, AirSensor>,
    pub soil_sensor: &'static mut Mutex<ThreadModeRawMutex, SoilSensor>,
    pub transceiver: &'static mut Mutex<ThreadModeRawMutex, RadioDevice>,
    pub database: EkvDatabase,
}

pub struct BoardBuilder {
    board_sensor: Option<BoardSensor>,
    air_sensor: Option<AirSensor>,
    soil_sensor: Option<SoilSensor>,
    radio: Option<RadioDevice>,
    database: Option<EkvDatabase>,
}

impl Board {
    pub async fn init(&mut self) -> Result<(), Error> {
        let mut board_sensor = self.board_sensor.lock().await;
        let mut air_sensor = self.air_sensor.lock().await;
        let mut soil_sensor = self.soil_sensor.lock().await;
        let mut transceiver = self.transceiver.lock().await;
        let mut errors = Vec::<Error, 4>::new();

        defmt::info!("Initializing board");

        let _ = board_sensor
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized board sensor {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = air_sensor
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized air sensor {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = soil_sensor
            .init()
            .await
            .inspect(|i| defmt::info!("Initialized soil sensor {:?}", i))
            .map_err(|e| errors.push(e));

        let _ = transceiver
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

        if errors.is_empty() {
            Ok(())
        } else {
            defmt::error!("Error(s) during board initialization");
            for error in errors {
                defmt::error!("{:?}", error);
            }
            Err(Error::FailedToInitialize)
        }
    }

    pub async fn join_otaa(&mut self) -> Result<(), Error> {
        let mut transceiver = self.transceiver.lock().await;

        transceiver.join_otaa().await
    }

    pub async fn initialize_uplink_frame_counter(&mut self) -> Result<(), Error> {
        if let None = self.database.get(&crate::database::Key::UPLINK_FRAME_COUNTER).await {
            self.database.put(&crate::database::Key::UPLINK_FRAME_COUNTER, 0).await
        } else {
            Ok(())
        }
    }

    pub async fn initialize_downlink_frame_counter(&mut self) -> Result<(), Error> {
        if let None = self.database.get(&crate::database::Key::DOWNLINK_FRAME_COUNTER).await {
            self.database.put(&crate::database::Key::DOWNLINK_FRAME_COUNTER, 0).await
        } else {
            Ok(())
        }
    }

    pub async fn increment_uplink_frame_counter(&mut self) -> Result<(), Error> {
        if let Some(mut counter) = self.database.get(&crate::database::Key::UPLINK_FRAME_COUNTER).await {
            counter += 1;
            self.database.put(&crate::database::Key::UPLINK_FRAME_COUNTER, counter).await
        } else {
            Err(Error::FailedToInitialize)
        }
    }

    pub async fn increment_downlink_frame_counter(&mut self) -> Result<(), Error> {
        if let Some(mut counter) = self.database.get(&crate::database::Key::DOWNLINK_FRAME_COUNTER).await {
            counter += 1;
            self.database.put(&crate::database::Key::DOWNLINK_FRAME_COUNTER, counter).await
        } else {
            Err(Error::FailedToInitialize)
        }
    }

    pub async fn collect_data(&mut self) -> Result<Vec<u8, 30>, Error> {
        let mut board_sensor = self.board_sensor.lock().await;
        let mut air_sensor = self.air_sensor.lock().await;
        let mut soil_sensor = self.soil_sensor.lock().await;
        let mut payload = Vec::<u8, 30>::new();
        let mut errors = Vec::<Error, 4>::new();

        defmt::info!("Collecting data from sensors");

        let _ = board_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Board {:?}", d))
            .map(|d| {
                let bytes: [u8; 11] = d.into();
                payload.extend_from_slice(&bytes).expect("should extend");
            })
            .map_err(|e| errors.push(e));

        let _ = air_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Air {:?}", d))
            .map(|d| {
                let bytes: [u8; 11] = d.into();
                payload.extend_from_slice(&bytes).expect("should extend");
            })
            .map_err(|e| errors.push(e));

        let _ = soil_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Soil {:?}", d))
            .map(|d| {
                let bytes: [u8; 8] = d.into();
                payload.extend_from_slice(&bytes).expect("should extend");
            })
            .map_err(|e| errors.push(e));

        if errors.is_empty() {
            Ok(payload)
        } else {
            defmt::error!("Error(s) during data collection");
            for error in errors {
                defmt::error!("{:?}", error);
            }
            Err(Error::FailedToCollectData)
        }
    }

    pub async fn uplink(&mut self, data: &[u8]) -> Result<(), Error> {
        let mut transceiver = self.transceiver.lock().await;

        defmt::info!("Sending uplink message");
        defmt::info!("Payload {=[u8]:x}", data);

        match transceiver.uplink(data).await {
            Ok(SendResponse::DownlinkReceived(f_count)) => {
                defmt::info!("Sent uplink, received downlink with fcount {=u32}", f_count);
                Ok(())
            }
            Ok(SendResponse::NoAck) => {
                defmt::warn!("Sent uplink, but no ack received");
                Ok(())
            }
            Ok(SendResponse::RxComplete) => {
                defmt::info!(" and acknowledged by the gateway");
                Ok(())
            }
            Ok(SendResponse::SessionExpired) => {
                defmt::error!("Failed to send uplink, session expired");
                Err(Error::LoraRadio)
            }
            Err(e) => {
                defmt::error!("Failed to send uplink message: {:?}", e);
                Err(e)
            }
        }
    }
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
        if let (Some(board_sensor), Some(air_sensor), Some(soil_sensor), Some(radio), Some(database)) =
            (self.board_sensor, self.air_sensor, self.soil_sensor, self.radio, self.database)
        {
            let board_sensor_ref = BOARD_SENSOR_CELL.init(Mutex::new(board_sensor));
            let air_sensor_ref = AIR_SENSOR_CELL.init(Mutex::new(air_sensor));
            let soil_sensor_ref = SOIL_SENSOR_CELL.init(Mutex::new(soil_sensor));
            let radio_ref = RADIO_CELL.init(Mutex::new(radio));

            Ok(Board {
                board_sensor: board_sensor_ref,
                air_sensor: air_sensor_ref,
                soil_sensor: soil_sensor_ref,
                transceiver: radio_ref,
                database,
            })
        } else {
            Err(Error::FailedToInitialize)
        }
    }
}
