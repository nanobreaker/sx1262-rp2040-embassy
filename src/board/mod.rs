use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use heapless::Vec;
use lorawan_device::async_device::SendResponse;
use static_cell::StaticCell;

use crate::{
    air_sensor::AirSensor,
    board_sensor::BoardSensor,
    device::Device,
    error::Error,
    sensor::Sensor,
    soil_sensor::SoilSensor,
    transceiver::{RadioDevice, Transceiver},
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
}

pub struct BoardBuilder {
    board_sensor: Option<BoardSensor>,
    air_sensor: Option<AirSensor>,
    soil_sensor: Option<SoilSensor>,
    radio: Option<RadioDevice>,
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
            .inspect(|d| defmt::info!("Board sensor {:?}", d))
            .map(|d| {
                let bytes: [u8; 11] = d.into();
                for byte in bytes {
                    let _ = payload.push(byte);
                }
            })
            .map_err(|e| errors.push(e));

        let _ = air_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Air sensor {:?}", d))
            .map(|d| {
                let bytes: [u8; 11] = d.into();
                for byte in bytes {
                    let _ = payload.push(byte);
                }
            })
            .map_err(|e| errors.push(e));

        let _ = soil_sensor
            .collect_data()
            .await
            .inspect(|d| defmt::info!("Soil sensor {:?}", d))
            .map(|d| {
                let bytes: [u8; 8] = d.into();
                for byte in bytes {
                    let _ = payload.push(byte);
                }
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

        defmt::info!("Sending uplink message: {=[u8]:#x}", data);

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

    pub fn build(self) -> Result<Board, crate::error::Error> {
        if let (Some(board_sensor), Some(air_sensor), Some(soil_sensor), Some(radio)) =
            (self.board_sensor, self.air_sensor, self.soil_sensor, self.radio)
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
            })
        } else {
            Err(Error::FailedToInitialize)
        }
    }
}
