use crate::error::Error;

use defmt::info;
use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};

use core::result::Result;

pub struct SoilSensor<'a> {
    addr: u8,
    i2c: &'a mut I2c<'a, I2C1, Async>,
}

impl<'a> SoilSensor<'a> {
    pub fn new(addr: u8, i2c_bus: &'a mut I2c<'a, I2C1, Async>) -> Self {
        Self { addr, i2c: i2c_bus }
    }

    pub async fn init(&mut self) -> Result<u16, Error> {
        let hw_id = self.get_hw_id().await?;
        let status = self.get_status().await?;
        let product_code: u16 = (status >> 16) as u16;
        let year: u8 = (status & 0x3f) as u8;
        let month: u8 = ((status >> 7) & 0xf) as u8;
        let day: u8 = ((status >> 11) & 0x1f) as u8;

        info!(
            "Soil Sensor [ATSAMD09]
            - hardware id code {:?}
            - product code {:?}
            - manufactoring year {:?} month {:?} day {:?}",
            hw_id, product_code, year, month, day
        );

        Ok(product_code)
    }

    pub async fn measure(&mut self) -> Result<(f32, u16), Error> {
        let temperature = self.get_temperature().await?;
        let moisture = self.get_moisture().await?;

        Ok((temperature, moisture))
    }

    async fn get_temperature(&mut self) -> Result<f32, Error> {
        let mut buffer = [0u8; 4];

        self.write(0x00, 0x04).await?;
        embassy_time::Timer::after_millis(1000).await;
        self.read(&mut buffer).await?;

        let temperature = u32::from_be_bytes(buffer);
        let temperature = (temperature as f32) / 65536.0;

        Ok(temperature)
    }

    async fn get_moisture(&mut self) -> Result<u16, Error> {
        let mut buffer = [0u8; 2];

        self.write(0x0f, 0x10).await?;
        embassy_time::Timer::after_millis(1000).await;
        self.read(&mut buffer).await?;

        let moisture = u16::from_be_bytes(buffer);

        Ok(moisture)
    }

    async fn get_hw_id(&mut self) -> Result<u8, Error> {
        let mut buffer = [0u8; 1];

        self.write(0x00, 0x01).await?;
        self.read(&mut buffer).await?;

        let hw_id = u8::from_be(buffer[0]);

        Ok(hw_id)
    }

    async fn get_status(&mut self) -> Result<u32, Error> {
        let mut buffer = [0u8; 4];

        self.write(0x00, 0x02).await?;
        self.read(&mut buffer).await?;

        let status = u32::from_be_bytes(buffer);

        Ok(status)
    }

    async fn write(&mut self, base_reg: u8, fn_reg: u8) -> Result<(), Error> {
        self.i2c.write_async(self.addr, [base_reg, fn_reg]).await.map_err(|e| e.into())
    }

    async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.i2c.read_async(self.addr, buffer).await.map_err(|e| e.into())
    }
}
