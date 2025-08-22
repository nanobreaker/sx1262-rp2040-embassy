#![no_std]
#![no_main]

mod config;
mod device;
mod radio;
mod sensor;
mod storage;

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_rp::config::Config;
use embassy_rp::peripherals::{self, I2C0};
use embassy_rp::{adc, bind_interrupts, Peri};
use {defmt_rtt as _, panic_probe as _};

use crate::device::Device;
use crate::radio::lora_radio::LoraRadio;
use crate::sensor::air_sensor::AirSensor;
use crate::sensor::soil_sensor::SoilSensor;
use crate::sensor::system_sensor::SystemSensor;
use crate::storage::flash_storage::FlashStorage;

bind_interrupts!(struct Irqs {
    ADC_IRQ_FIFO => embassy_rp::adc::InterruptHandler;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<I2C0>;
});

assign_resources! {
    adc: AdcRes {
        adc: ADC,
    },
    system: SystemRes {
        adc_tmp: ADC_TEMP_SENSOR,
        usb: PIN_24,
        btr: PIN_26,
        vsys: PIN_29,
    },
    flash: FlashRes {
        flash: FLASH,
    },
    air: AirSensorRes {
        sda: PIN_16,
        scl: PIN_17,
        i2c0: I2C0,
    },
    soil: SoilSensorRes {
        pwr: PIN_22,
        sig: PIN_27,
    },
    radio: RadioRes {
        busy: PIN_2,
        cs: PIN_3,
        clk: PIN_10,
        mosi: PIN_11,
        miso: PIN_12,
        rst: PIN_15,
        dio1: PIN_20,
        dma_ch0: DMA_CH0,
        dma_ch1: DMA_CH1,
        spi1: SPI1,
    },
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Config::default());
    let r = split_resources! {p};

    let adc = adc::Adc::new(r.adc.adc, Irqs, adc::Config::default());
    let system = SystemSensor::new(r.system);
    let soil = SoilSensor::new(r.soil);
    let air = AirSensor::new(r.air);
    let storage = FlashStorage::new(r.flash);
    let radio = LoraRadio::try_new(r.radio).await.expect("radio init failed");
    let device = Device::new(adc, system, soil, air, radio, storage);

    device.run().await;
}
