use embassy_rp::adc::{self};
use embassy_rp::gpio::{self, Input, Pull};

use crate::sensor::Sensor;
use crate::SystemRes;

#[derive(defmt::Format)]
pub enum SystemSensorError {
    Adc(adc::Error),
}

impl From<adc::Error> for SystemSensorError {
    fn from(value: adc::Error) -> Self {
        Self::Adc(value)
    }
}

pub struct SystemSensor {
    temp_adc: adc::Channel<'static>, // rp2040 chip temperature
    usb_pwr: gpio::Input<'static>,   // usb power connection
    btr_adc: adc::Channel<'static>,  // battery power connection
    vsys_adc: adc::Channel<'static>, // system voltage
}

#[derive(defmt::Format)]
pub enum PowerSource {
    Battery,
    Usb,
}

impl SystemSensor {
    pub fn new(r: SystemRes) -> Self {
        let temp_adc = adc::Channel::new_temp_sensor(r.adc_tmp);
        let btr_adc = adc::Channel::new_pin(r.btr, Pull::None);
        let vsys_adc = adc::Channel::new_pin(r.vsys, Pull::None);
        let usb_pwr = Input::new(r.usb, Pull::None);

        Self {
            temp_adc,
            usb_pwr,
            btr_adc,
            vsys_adc,
        }
    }

    async fn get_temperature(&mut self, adc: &mut adc::Adc<'static, adc::Async>) -> Result<f32, adc::Error> {
        let adc_raw = adc.read(&mut self.temp_adc).await?;
        let adc_voltage = adc_raw as f32 * 3.3 / 4096.0;
        let temp = 27.0 - (adc_voltage - 0.706) / 0.001721;
        let sign = if temp < 0.0 { -1.0 } else { 1.0 };
        let rounded_temp_x10: i16 = ((temp * 10.0) + 0.5 * sign) as i16;
        let temp = (rounded_temp_x10 as f32) / 10.0;

        defmt::debug!("temp adc_raw {=u16}", adc_raw);
        defmt::debug!("temp adc_voltage {=f32}", adc_voltage);

        Ok(temp)
    }

    async fn get_battery_capacity(&mut self, adc: &mut adc::Adc<'static, adc::Async>) -> Result<(f32, f32), adc::Error> {
        let adc_raw = adc.read(&mut self.btr_adc).await?;
        let adc_voltage = (adc_raw as f32) * 3.3 * 3.0 / 4096.0;
        let percentage = ((adc_voltage - 3.0) / (4.2 - 3.0)) * 100.0;

        defmt::debug!("battery adc_raw {=u16}", adc_raw);
        defmt::debug!("battery adc_voltage {=f32}", adc_voltage);
        defmt::debug!("battery percentage {=f32}", percentage);

        Ok((adc_voltage, percentage))
    }

    async fn get_vsys_voltage(&mut self, adc: &mut adc::Adc<'static, adc::Async>) -> Result<f32, adc::Error> {
        let adc_raw = adc.read(&mut self.vsys_adc).await?;
        let adc_voltage = (adc_raw as f32) * 3.3 * 3.0 / 4096.0;

        defmt::debug!("vsys adc_raw {=u16}", adc_raw);
        defmt::debug!("vsys adc_voltage {=f32}", adc_voltage);

        Ok(adc_voltage)
    }

    fn get_power_source(&mut self) -> PowerSource {
        if self.usb_pwr.is_high() {
            PowerSource::Usb
        } else {
            PowerSource::Battery
        }
    }
}

impl Sensor<18> for SystemSensor {
    type Error = SystemSensorError;

    async fn on(&mut self) -> Result<(), Self::Error> {
        // we don't have control over the board power
        Ok(())
    }

    async fn off(&mut self) -> Result<(), Self::Error> {
        // we don't have control over the board power
        Ok(())
    }

    async fn verify(&mut self) -> Result<(), Self::Error> {
        // todo: to implement some basic verify check, maybe read pico serial number?
        Ok(())
    }

    async fn probe(&mut self, adc: &mut adc::Adc<'static, adc::Async>) -> Result<[u8; 18], Self::Error> {
        let temp = self.get_temperature(adc).await?;
        let (btr_voltage, btr_capacity) = self.get_battery_capacity(adc).await?;
        let vsys_voltage = self.get_vsys_voltage(adc).await?;
        let power_source = match self.get_power_source() {
            PowerSource::Battery => 0,
            PowerSource::Usb => 1,
        };

        let temp_scl = (temp * 10.0) as u16;
        let btr_voltage_scl = (btr_voltage * 100.0) as u16;
        let btr_capacity_scl = (btr_capacity * 2.0) as u8;
        let vsys_voltage_scl = (vsys_voltage * 100.0) as u16;

        defmt::info!(
            "System sensor data - tmp {=f32}°C vbtr {=f32}V cbtr {=f32}% vsys {=f32}V pwr {=u8}",
            temp,
            btr_voltage,
            btr_capacity,
            vsys_voltage,
            power_source,
        );

        let mut buf = [0u8; 18];
        buf[0] = 0x03; // channel    - 3 [rp2040]
        buf[1] = 0x67; // type       - temperature [2 bytes]
        buf[2] = (temp_scl >> 8) as u8; //            - first byte
        buf[3] = temp_scl as u8; //            - second byte
        buf[4] = 0x03; // channel    - 3 [rp2040]
        buf[5] = 0x02; // type       - analog input [2 bytes]
        buf[6] = (btr_voltage_scl >> 8) as u8; //            - first byte
        buf[7] = btr_voltage_scl as u8; //            - second byte
        buf[8] = 0x03; // channel    - 3 [rp2040]
        buf[9] = 0x68; // type       - humidity [1 bytes]
        buf[10] = btr_capacity_scl; //            - first byte
        buf[11] = 0x04; // channel    - 3 [rp2040]
        buf[12] = 0x02; // type       - analog input [2 bytes]
        buf[13] = (vsys_voltage_scl >> 8) as u8; //            - first byte
        buf[14] = vsys_voltage_scl as u8; //            - second byte
        buf[15] = 0x04; // channel    - 3 [rp2040]
        buf[16] = 0x00; // type       - diginal input [1 bytes]
        buf[17] = power_source; //            - first byte

        Ok(buf)
    }
}
