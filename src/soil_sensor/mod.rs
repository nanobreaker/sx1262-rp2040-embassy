use crate::{config, device::Device, error::Error, sensor::Sensor, Irqs, SoilSensorResources};

use embassy_rp::{
    i2c::{self, Async, I2c},
    peripherals::I2C1,
};

use core::{result::Result, u16};

#[derive(defmt::Format)]
pub struct Data {
    pub temp: f32,
    pub mois: u16,
}

impl From<Data> for [u8; 8] {
    fn from(val: Data) -> Self {
        let temp_scl = (val.temp * 10.0) as i16;
        [
            0x02,                  // channel    - 2 [soil_sensor]
            0x67,                  // type       - temperature [2 bytes]
            (temp_scl >> 8) as u8, //            - first byte
            temp_scl as u8,        //            - second byte
            0x02,                  // channel    - 2 [soil_sensor]
            0x65,                  // type       - illuminance [2 bytes]
            (val.mois >> 8) as u8, //            - first byte
            val.mois as u8,        //            - second byte
        ]
    }
}

#[derive(defmt::Format)]
pub struct Info {
    pub hw_id: u8,
    pub product_code: u16,
    pub year: u8,
    pub month: u8,
    pub day: u8,
}

pub struct SoilSensor {
    addr: u8,
    bus: I2c<'static, I2C1, Async>,
}

impl Device<SoilSensorResources> for SoilSensor {
    type Info = Info;
    async fn prepare(r: SoilSensorResources) -> Result<Self, Error> {
        let i2c_1_bus = I2c::new_async(r.i2c1, r.pin_19, r.pin_18, Irqs, i2c::Config::default());

        Ok(SoilSensor {
            addr: config::Config::I2C_ADDR_SOIL_SENSOR,
            bus: i2c_1_bus,
        })
    }

    async fn init(&mut self) -> Result<Self::Info, Error> {
        let hw_id = self.get_hw_id().await?;
        let status = self.get_status().await?;
        let product_code: u16 = (status >> 16) as u16;
        let year: u8 = (status & 0x3f) as u8;
        let month: u8 = ((status >> 7) & 0xf) as u8;
        let day: u8 = ((status >> 11) & 0x1f) as u8;

        Ok(Info {
            hw_id,
            product_code,
            year,
            month,
            day,
        })
    }
}

impl Sensor<Data> for SoilSensor {
    async fn collect_data(&mut self) -> Result<Data, Error> {
        let temp = self.get_temperature().await?;
        let mois = self.get_moisture().await?;
        let data = Data { temp, mois };

        Ok(data)
    }
}

impl SoilSensor {
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
