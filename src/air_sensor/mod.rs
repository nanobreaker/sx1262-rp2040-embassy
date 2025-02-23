use crate::error::Error;

use defmt::info;
use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C0,
};
use embassy_time::Timer;

use core::result::Result;

const SERIAL_NUMBER_COMMAND: u16 = 0x3682;
const READ_MEASUREMENT_COMMAND: u16 = 0xec05;
const MEASURE_SINGLE_SHOT_COMMAND: u16 = 0x219d;

pub struct AirSensor<'a> {
    addr: u16,
    i2c: &'a mut I2c<'a, I2C0, Async>,
}

impl<'a> AirSensor<'a> {
    pub fn new(addr: u16, i2c_bus: &'a mut I2c<'a, I2C0, Async>) -> Self {
        Self { addr, i2c: i2c_bus }
    }

    pub async fn init(&mut self) -> Result<u64, Error> {
        let serial_number = self.get_serial_number().await?;

        info!(
            "Air Sensor [SCD41]
            - serial number {:?}",
            serial_number
        );

        Ok(serial_number)
    }

    pub async fn measure(&mut self) -> Result<(f32, f32, u32), Error> {
        self.write(MEASURE_SINGLE_SHOT_COMMAND).await?;

        Timer::after_millis(5000).await;

        self.write(READ_MEASUREMENT_COMMAND).await?;
        let mut buffer = [0u8; 9];
        self.read(&mut buffer).await?;

        let co2 = u16::from_be_bytes([buffer[0], buffer[1]]) as u32;
        let temperature = u16::from_be_bytes([buffer[2], buffer[3]]) as f32 / 100.0;
        let humidity = u16::from_be_bytes([buffer[4], buffer[5]]) as f32 / 100.0;

        Ok((temperature, humidity, co2))
    }

    pub async fn get_serial_number(&mut self) -> Result<u64, Error> {
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
        self.i2c.write_async(self.addr, command.to_be_bytes()).await.map_err(|e| e.into())
    }

    async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.i2c.read_async(self.addr, buffer).await.map_err(|e| e.into())
    }
}
