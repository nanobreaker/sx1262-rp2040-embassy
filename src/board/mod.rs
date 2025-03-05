use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use static_cell::StaticCell;

use crate::{air_sensor::AirSensor, board_sensor::BoardSensor, error::Error, soil_sensor::SoilSensor, transceiver::RadioDevice};

static BOARD_SENSOR_CELL: StaticCell<Mutex<ThreadModeRawMutex, BoardSensor>> = StaticCell::new();
static AIR_SENSOR_CELL: StaticCell<Mutex<ThreadModeRawMutex, AirSensor>> = StaticCell::new();
static SOIL_SENSOR_CELL: StaticCell<Mutex<ThreadModeRawMutex, SoilSensor>> = StaticCell::new();
static RADIO_CELL: StaticCell<Mutex<ThreadModeRawMutex, RadioDevice>> = StaticCell::new();

pub struct Board {
    pub board_sensor: &'static mut Mutex<ThreadModeRawMutex, BoardSensor>,
    pub air_sensor: &'static mut Mutex<ThreadModeRawMutex, AirSensor>,
    pub soil_sensor: &'static mut Mutex<ThreadModeRawMutex, SoilSensor<'static>>,
    pub radio: &'static mut Mutex<ThreadModeRawMutex, RadioDevice>,
}

pub struct BoardBuilder {
    board_sensor: Option<BoardSensor>,
    air_sensor: Option<AirSensor>,
    soil_sensor: Option<SoilSensor<'static>>,
    radio: Option<RadioDevice>,
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

    pub fn with_soil_sensor(mut self, soil_sensor: SoilSensor<'static>) -> BoardBuilder {
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
                radio: radio_ref,
            })
        } else {
            Err(Error::FailedToInitialize)
        }
    }
}
