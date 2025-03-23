use embassy_rp::{
    adc::{self, Async},
    gpio::{self, Input, Pull},
};

use crate::{device::Device, error::Error, sensor::Sensor, BoardSensorResources, Irqs};

pub struct BoardSensor {
    adc: adc::Adc<'static, Async>,
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

#[derive(defmt::Format)]
pub struct Data {
    pub temp: f32,
    pub btr_voltage: f32,
    pub btr_capacity: f32,
    pub vsys_voltage: f32,
    pub power_source: PowerSource,
}

impl From<Data> for [u8; 11] {
    fn from(value: Data) -> Self {
        let temp_scl = (value.temp * 10.0) as u16;
        let btr_voltage_scl = (value.btr_voltage * 100.0) as u16;
        let btr_capacity_scl = (value.btr_capacity * 2.0) as u8;
        [
            0x03,                         // channel    - 3 [rp2040]
            0x67,                         // type       - temperature [2 bytes]
            (temp_scl >> 8) as u8,        //            - first byte
            temp_scl as u8,               //            - second byte
            0x03,                         // channel    - 3 [rp2040]
            0x02,                         // type       - analog input [2 bytes]
            (btr_voltage_scl >> 8) as u8, //            - first byte
            btr_voltage_scl as u8,        //            - second byte
            0x03,                         // channel    - 3 [rp2040]
            0x68,                         // type       - humidity [1 bytes]
            btr_capacity_scl,             //            - first byte
        ]
    }
}

impl Device<BoardSensorResources> for BoardSensor {
    type Info = ();

    async fn prepare(r: BoardSensorResources) -> Result<BoardSensor, Error> {
        let adc = adc::Adc::new(r.adc, Irqs, Default::default());
        let temp_adc = adc::Channel::new_temp_sensor(r.adc_temp_sensor);
        let btr_adc = adc::Channel::new_pin(r.pin_26, Pull::None);
        let vsys_adc = adc::Channel::new_pin(r.pin_29, Pull::None);
        let usb_pwr = Input::new(r.pin_24, Pull::None);

        Ok(BoardSensor {
            adc,
            temp_adc,
            usb_pwr,
            btr_adc,
            vsys_adc,
        })
    }

    async fn init(&mut self) -> Result<Self::Info, Error> {
        Ok(())
    }
}

impl Sensor<Data> for BoardSensor {
    async fn collect_data(&mut self) -> Result<Data, Error> {
        let temp = self.get_temperature().await?;
        let (btr_voltage, btr_capacity) = self.get_battery_capacity().await?;
        let power_source = self.get_power_source().await;
        let vsys_voltage = self.get_vsys_voltage().await?;

        Ok(Data {
            temp,
            btr_voltage,
            btr_capacity,
            vsys_voltage,
            power_source,
        })
    }
}

impl BoardSensor {
    async fn get_temperature(&mut self) -> Result<f32, Error> {
        let temp_adc_raw = self.adc.read(&mut self.temp_adc).await?;
        let temp_adc = 27.0 - (temp_adc_raw as f32 * 3.3 / 4096.0 - 0.706) / 0.001721;
        let sign = if temp_adc < 0.0 { -1.0 } else { 1.0 };
        let rounded_temp_x10: i16 = ((temp_adc * 10.0) + 0.5 * sign) as i16;
        let temp = (rounded_temp_x10 as f32) / 10.0;

        Ok(temp)
    }

    async fn get_battery_capacity(&mut self) -> Result<(f32, f32), Error> {
        let btr_adc_raw = self.adc.read(&mut self.btr_adc).await?;
        let btr_adc = (btr_adc_raw as f32 / 4095.0) * 3.3;
        let btr_voltage = btr_adc * 3.19;
        let percentage = ((btr_voltage - 3.2) / (4.2 - 3.2)) * 100.0;
        let btr_capacity = percentage.clamp(0.0, 100.0);

        Ok((btr_voltage, btr_capacity))
    }

    async fn get_power_source(&mut self) -> PowerSource {
        if self.usb_pwr.is_high() {
            PowerSource::Usb
        } else {
            PowerSource::Battery
        }
    }

    async fn get_vsys_voltage(&mut self) -> Result<f32, Error> {
        let vsys_adc_raw = self.adc.read(&mut self.vsys_adc).await?;
        let vsys_voltage = (vsys_adc_raw as f32) * 3.3 * 3.0 / 4096.0;
        Ok(vsys_voltage)
    }
}
