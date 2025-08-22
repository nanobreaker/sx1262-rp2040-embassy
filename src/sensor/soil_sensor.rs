use core::result::Result;

use embassy_rp::adc::{self};
use embassy_rp::gpio::{self, Level, Pull};

use crate::sensor::Sensor;
use crate::SoilSensorRes;

#[derive(defmt::Format)]
pub enum SoilSensorError {
    Adc(adc::Error),
}

pub struct SoilSensor {
    pwr: gpio::Output<'static>,
    sig: adc::Channel<'static>,
}

impl SoilSensor {
    pub fn new(r: SoilSensorRes) -> Self {
        let pwr = gpio::Output::new(r.pwr, Level::Low);
        let sig = adc::Channel::new_pin(r.sig, Pull::None);

        Self { pwr, sig }
    }
}

impl Sensor<4> for SoilSensor {
    type Error = SoilSensorError;

    async fn on(&mut self) -> Result<(), Self::Error> {
        if self.pwr.is_set_low() {
            self.pwr.set_high();
        }

        Ok(())
    }

    async fn off(&mut self) -> Result<(), Self::Error> {
        if self.pwr.is_set_high() {
            self.pwr.set_low();
        }

        Ok(())
    }

    async fn verify(&mut self) -> Result<(), Self::Error> {
        // todo: is there a way to verify?
        Ok(())
    }

    async fn probe(&mut self, adc: &mut adc::Adc<'static, adc::Async>) -> Result<[u8; 4], Self::Error> {
        match adc.read(&mut self.sig).await {
            Ok(adc_raw) => {
                defmt::info!("Soil sensor data - moist {=u16}", adc_raw);

                let mut buf = [0u8; 4];

                buf[0] = 0x02;
                buf[1] = 0x65;
                buf[2] = (adc_raw >> 8) as u8;
                buf[3] = adc_raw as u8;

                Ok(buf)
            }
            Err(err) => Err(SoilSensorError::Adc(err)),
        }
    }
}
