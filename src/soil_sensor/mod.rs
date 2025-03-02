use crate::{device::Device, error::Error, sensor::Sensor};

use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};

use core::{result::Result, u16};

pub struct Data {
    temp: f32,
    mois: u16,
}

impl Into<[u8; 8]> for Data {
    fn into(self) -> [u8; 8] {
        let temp_scl = (self.temp * 10.0) as i16;
        [
            0x02,                   // channel    - 2 [soil_sensor]
            0x67,                   // type       - temperature [2 bytes]
            (temp_scl >> 8) as u8,  //            - first byte
            temp_scl as u8,         //            - second byte
            0x02,                   // channel    - 2 [soil_sensor]
            0x65,                   // type       - illuminance [2 bytes]
            (self.mois >> 8) as u8, //            - first byte
            self.mois as u8,        //            - second byte
        ]
    }
}

pub struct Info {
    pub hw_id: u8,
    pub product_code: u16,
    pub year: u8,
    pub month: u8,
    pub day: u8,
}

pub struct SoilSensor<'a> {
    addr: u8,
    bus: &'a mut I2c<'a, I2C1, Async>,
}

impl<'a> Device<u16, Info> for SoilSensor<'a> {
    async fn init(&mut self) -> Result<u16, Error> {
        let status = self.get_status().await?;
        let product_code: u16 = (status >> 16) as u16;

        Ok(product_code)
    }

    async fn info(&mut self) -> Result<Info, Error> {
        let hw_id = self.get_hw_id().await?;
        let status = self.get_status().await?;
        let product_code: u16 = (status >> 16) as u16;
        let year: u8 = (status & 0x3f) as u8;
        let month: u8 = ((status >> 7) & 0xf) as u8;
        let day: u8 = ((status >> 11) & 0x1f) as u8;
        let info = Info {
            hw_id,
            product_code,
            year,
            month,
            day,
        };

        Ok(info)
    }
}

impl<'a> Sensor<Data> for SoilSensor<'a> {
    async fn collect_data(&mut self) -> Result<Data, Error> {
        let temp = self.get_temperature().await?;
        let mois = self.get_moisture().await?;
        let data = Data { temp, mois };

        Ok(data)
    }
}

impl<'a> SoilSensor<'a> {
    pub fn new(addr: u8, i2c_bus: &'a mut I2c<'a, I2C1, Async>) -> Self {
        Self { addr, bus: i2c_bus }
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
        self.bus.write_async(self.addr, [base_reg, fn_reg]).await.map_err(|e| e.into())
    }

    async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.bus.read_async(self.addr, buffer).await.map_err(|e| e.into())
    }
}
