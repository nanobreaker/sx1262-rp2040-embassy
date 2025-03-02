use crate::{device::Device, error::Error, sensor::Sensor};

use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C0,
};
use embassy_time::Timer;

use core::result::Result;

const SERIAL_NUMBER_COMMAND: u16 = 0x3682;
const READ_MEASUREMENT_COMMAND: u16 = 0xec05;
const MEASURE_SINGLE_SHOT_COMMAND: u16 = 0x219d;

pub struct Data {
    temp: f32,
    hum: f32,
    co2: u16,
}

impl Into<[u8; 11]> for Data {
    fn into(self) -> [u8; 11] {
        let temp_scl = (self.temp * 10.0) as i16;
        let hum_scl = (self.hum * 2.0) as u8;
        [
            0x01,                  // channel    - 1 [air_sensor]
            0x67,                  // type       - temperature [2 bytes]
            (temp_scl >> 8) as u8, //            - first byte
            temp_scl as u8,        //            - second byte
            0x01,                  // channel    - 1 [air_sensor]
            0x68,                  // type       - humidity [1 byte]
            hum_scl,               //            - first byte
            0x01,                  // channel    - 1 [air_sensor]
            0x65,                  // type       - illuminance [2 bytes]
            (self.co2 >> 8) as u8, //            - first byte
            self.co2 as u8,        //            - second byte
        ]
    }
}

pub struct Info {
    pub serial_number: u64,
}

pub struct AirSensor<'a> {
    addr: u16,
    bus: &'a mut I2c<'a, I2C0, Async>,
}

impl<'a> Device<u64, Info> for AirSensor<'a> {
    async fn init(&mut self) -> Result<u64, Error> {
        self.get_serial_number().await
    }

    async fn info(&mut self) -> Result<Info, Error> {
        let serial_number = self.get_serial_number().await?;
        let info = Info { serial_number };

        Ok(info)
    }
}

impl<'a> Sensor<Data> for AirSensor<'a> {
    async fn collect_data(&mut self) -> Result<Data, Error> {
        self.write(MEASURE_SINGLE_SHOT_COMMAND).await?;

        Timer::after_millis(5000).await;

        self.write(READ_MEASUREMENT_COMMAND).await?;
        let mut buffer = [0u8; 9];
        self.read(&mut buffer).await?;

        let bytes_temp = u16::from_be_bytes([buffer[3], buffer[4]]);
        let temp = bytes_temp as f32 * 175.0f32 / (u16::MAX as f32) - 45.0;

        let bytes_hum = u16::from_be_bytes([buffer[6], buffer[7]]);
        let hum = bytes_hum as f32 * 100.0 / (u16::MAX as f32);

        let co2 = u16::from_be_bytes([buffer[0], buffer[1]]);

        let data = Data { temp, hum, co2 };

        Ok(data)
    }
}

impl<'a> AirSensor<'a> {
    pub fn new(addr: u16, i2c_bus: &'a mut I2c<'a, I2C0, Async>) -> Self {
        Self { addr, bus: i2c_bus }
    }

    async fn get_serial_number(&mut self) -> Result<u64, Error> {
        let mut buffer = [0u8; 9];

        self.write(SERIAL_NUMBER_COMMAND).await?;

        Timer::after_millis(1).await;

        self.read(&mut buffer).await?;

        // todo: implement crc checking
        let word0 = u16::from_ne_bytes([buffer[0], buffer[1]]);
        let word1 = u16::from_ne_bytes([buffer[3], buffer[4]]);
        let word2 = u16::from_ne_bytes([buffer[6], buffer[7]]);

        let serial_number: u64 = (u64::from(word0) << 32) | (u64::from(word1) << 16) | u64::from(word2);

        Ok(serial_number)
    }

    async fn write(&mut self, command: u16) -> Result<(), Error> {
        self.bus.write_async(self.addr, command.to_be_bytes()).await.map_err(|e| e.into())
    }

    async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.bus.read_async(self.addr, buffer).await.map_err(|e| e.into())
    }
}
