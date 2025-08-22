use core::result::Result;

use embassy_rp::adc;
use embassy_rp::i2c::{self, Async};
use embassy_rp::peripherals::I2C0;
use embassy_time::Timer;

use crate::sensor::Sensor;
use crate::{config, AirSensorRes, Irqs};

const SERIAL_NUMBER_COMMAND: u16 = 0x3682;
const READ_MEASUREMENT_COMMAND: u16 = 0xec05;
const MEASURE_SINGLE_SHOT_COMMAND: u16 = 0x219d;
const POWER_DOWN: u16 = 0x36e0;
const WAKE_UP: u16 = 0x36f6;

#[derive(defmt::Format)]
pub enum AirSensorError {
    I2C(i2c::Error),
}

pub struct AirSensor {
    adr: u16,
    bus: i2c::I2c<'static, I2C0, Async>,
    powered: bool,
}

impl AirSensor {
    pub fn new(r: AirSensorRes) -> Self {
        let i2c_0_bus = i2c::I2c::new_async(r.i2c0, r.scl, r.sda, Irqs, i2c::Config::default());

        Self {
            adr: config::Config::I2C_ADDR_AIR_SENSOR,
            bus: i2c_0_bus,
            powered: true,
        }
    }

    async fn write(&mut self, command: u16) -> Result<(), i2c::Error> {
        self.bus.write_async(self.adr, command.to_be_bytes()).await
    }

    async fn read(&mut self, buffer: &mut [u8]) -> Result<(), i2c::Error> {
        self.bus.read_async(self.adr, buffer).await
    }
}

impl Sensor<11> for AirSensor {
    type Error = AirSensorError;

    async fn on(&mut self) -> Result<(), Self::Error> {
        if self.powered {
            return Ok(());
        }

        if let Err(err) = self.write(WAKE_UP).await {
            return Err(AirSensorError::I2C(err));
        }

        // wait 30 ms according to spec
        Timer::after_millis(30).await;

        // todo: verify that device is turned on?
        // spec recomends to read serial number as test

        Ok(())
    }

    async fn off(&mut self) -> Result<(), Self::Error> {
        if !self.powered {
            return Ok(());
        }

        if let Err(err) = self.write(POWER_DOWN).await {
            return Err(AirSensorError::I2C(err));
        }

        // wait 1 ms according to spec
        Timer::after_millis(1).await;

        Ok(())
    }

    async fn verify(&mut self) -> Result<(), Self::Error> {
        let mut buffer = [0u8; 9];

        if let Err(err) = self.write(SERIAL_NUMBER_COMMAND).await {
            return Err(AirSensorError::I2C(err));
        }

        // wait 1ms according to spec
        Timer::after_millis(1).await;

        if let Err(err) = self.read(&mut buffer).await {
            return Err(AirSensorError::I2C(err));
        }

        let word0 = u16::from_ne_bytes([buffer[0], buffer[1]]);
        let word1 = u16::from_ne_bytes([buffer[3], buffer[4]]);
        let word2 = u16::from_ne_bytes([buffer[6], buffer[7]]);
        let serial_number: u64 = (u64::from(word0) << 32) | (u64::from(word1) << 16) | u64::from(word2);

        defmt::debug!("Air sensor serial number {=u64}", serial_number);

        Ok(())
    }

    async fn probe(&mut self, _adc: &mut adc::Adc<'static, adc::Async>) -> Result<[u8; 11], Self::Error> {
        if let Err(err) = self.write(MEASURE_SINGLE_SHOT_COMMAND).await {
            return Err(AirSensorError::I2C(err));
        }

        // wait 5000ms according to spec
        Timer::after_millis(5000).await;

        if let Err(err) = self.write(READ_MEASUREMENT_COMMAND).await {
            return Err(AirSensorError::I2C(err));
        }

        // wait 1ms according to spec
        Timer::after_millis(1).await;

        let mut buffer = [0u8; 9];
        if let Err(err) = self.read(&mut buffer).await {
            return Err(AirSensorError::I2C(err));
        }

        let bytes_temp = u16::from_be_bytes([buffer[3], buffer[4]]);
        let temp = bytes_temp as f32 * 175.0f32 / (u16::MAX as f32) - 45.0;
        let bytes_hum = u16::from_be_bytes([buffer[6], buffer[7]]);
        let hum = bytes_hum as f32 * 100.0 / (u16::MAX as f32);
        let co2 = u16::from_be_bytes([buffer[0], buffer[1]]);

        let temp_scl = (temp * 10.0) as i16;
        let hum_scl = (hum * 2.0) as u8;

        defmt::info!("Air sensor data - tmp {=f32}°C hum {=f32}% co2 {=u16}ppm", temp, hum, co2);

        let mut buf = [0u8; 11];
        buf[0] = 0x01; // channel    - 1 [air_sensor]
        buf[1] = 0x67; // type       - temperature [2 bytes]
        buf[2] = (temp_scl >> 8) as u8; //            - first byte
        buf[3] = temp_scl as u8; //            - second byte
        buf[4] = 0x01; // channel    - 1 [air_sensor]
        buf[5] = 0x68; // type       - humidity [1 byte]
        buf[6] = hum_scl; //            - first byte
        buf[7] = 0x01; // channel    - 1 [air_sensor]
        buf[8] = 0x65; // type       - illuminance [2 bytes]
        buf[9] = (co2 >> 8) as u8; //            - first byte
        buf[10] = co2 as u8; //            - second byte

        Ok(buf)
    }
}
