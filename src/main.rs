#![no_std]
#![no_main]

mod air_sensor;
mod config;
mod error;
mod soil_sensor;

use air_sensor::AirSensor;
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::i2c::{I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, I2C1, SPI1};
use embassy_rp::spi::{Async, Config, Spi};
use embassy_rp::{bind_interrupts, i2c};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::lorawan_radio::LorawanRadio;
use lora_phy::sx126x::TcxoCtrlVoltage;
use lora_phy::sx126x::{self, Sx1262, Sx126x};
use lora_phy::LoRa;
use lorawan_device::async_device::{region, Device, EmbassyTimer, JoinMode};
use lorawan_device::default_crypto::DefaultFactory as Crypto;
use lorawan_device::{AppEui, AppKey, DevEui};
use soil_sensor::SoilSensor;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

type Sx1262Radio = LorawanRadio<
    Sx126x<
        ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
        GenericSx126xInterfaceVariant<Output<'static>, Input<'static>>,
        Sx1262,
    >,
    Delay,
    14,
>;
type RadioDevice = Device<Sx1262Radio, Crypto, EmbassyTimer, RoscRng>;

static RADIO_MUTEX: StaticCell<Mutex<ThreadModeRawMutex, RadioDevice>> = StaticCell::new();

static AIR_SENSOR_I2C_BUS: StaticCell<I2c<'static, I2C0, i2c::Async>> = StaticCell::new();
static AIR_SENSOR_MUTEX: StaticCell<Mutex<ThreadModeRawMutex, AirSensor>> = StaticCell::new();
static AIR_CTRL_CHNL: Channel<ThreadModeRawMutex, AirSensorCommand, 4> = Channel::new();

static SOIL_SENSOR_I2C_BUS: StaticCell<I2c<'static, I2C1, i2c::Async>> = StaticCell::new();
static SOIL_SENSOR_MUTEX: StaticCell<Mutex<ThreadModeRawMutex, SoilSensor>> = StaticCell::new();
static SOIL_CTRL_CHNL: Channel<ThreadModeRawMutex, SoilSensorCommand, 4> = Channel::new();

static DATA_CHNL: Channel<ThreadModeRawMutex, SensorData, 10> = Channel::new();
static RADIO_CTRL_CHNL: Channel<ThreadModeRawMutex, RadioCommand, 10> = Channel::new();

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
    I2C1_IRQ => InterruptHandler<I2C1>;
});

enum SensorData {
    Soil(f32, u16),
    Air(f32, f32, u16),
}

enum RadioCommand {
    Join(JoinMode),
    UplinkAirData([u8; 11]),
    UplinkSoilData([u8; 8]),
}

enum AirSensorCommand {
    Init,
    Measure,
}

enum SoilSensorCommand {
    Init,
    Measure,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let nss = Output::new(p.PIN_3.degrade(), Level::High);
    let reset = Output::new(p.PIN_15.degrade(), Level::High);
    let dio1 = Input::new(p.PIN_20.degrade(), Pull::None);
    let busy = Input::new(p.PIN_2.degrade(), Pull::None);
    let spi = Spi::new(p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, p.DMA_CH0, p.DMA_CH1, Config::default());
    let spi_bus = ExclusiveDevice::new(spi, nss, Delay).unwrap();
    let sx1262_config = sx126x::Config {
        chip: Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
        use_dcdc: true,
        rx_boost: false,
    };
    let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
    let lora = LoRa::new(Sx126x::new(spi_bus, iv, sx1262_config), true, Delay).await.unwrap();
    let mut radio: LorawanRadio<_, _, 14> = lora.into();
    radio.set_rx_window_lead_time(config::Config::RX_WINDOW_LEAD_TIME);
    radio.set_rx_window_buffer(config::Config::RX_WINDOW_BUFFER);
    let region: region::Configuration = region::Configuration::new(config::Config::LORAWAN_REGION);
    let device: Device<_, Crypto, _, _> = Device::new(region, radio, EmbassyTimer::new(), embassy_rp::clocks::RoscRng);
    let radio_device: &'static _ = RADIO_MUTEX.init(Mutex::new(device));

    let scl_0 = p.PIN_17;
    let sda_0 = p.PIN_16;
    let i2c_0_bus = I2c::new_async(p.I2C0, scl_0, sda_0, Irqs, i2c::Config::default());
    let i2c_0_bus_ref = AIR_SENSOR_I2C_BUS.init(i2c_0_bus);
    let air_sensor = air_sensor::AirSensor::new(config::Config::I2C_ADDR_AIR_SENSOR, i2c_0_bus_ref);
    let air_sensor_ref = AIR_SENSOR_MUTEX.init(Mutex::new(air_sensor));

    let scl_1 = p.PIN_19;
    let sda_1 = p.PIN_18;
    let i2c_1_bus = I2c::new_async(p.I2C1, scl_1, sda_1, Irqs, i2c::Config::default());
    let i2c_1_bus_ref = SOIL_SENSOR_I2C_BUS.init(i2c_1_bus);
    let soil_sensor = soil_sensor::SoilSensor::new(config::Config::I2C_ADDR_SOIL_SENSOR, i2c_1_bus_ref);
    let soil_sensor_ref = SOIL_SENSOR_MUTEX.init(Mutex::new(soil_sensor));

    spawner.spawn(radio_module_task(radio_device, &RADIO_CTRL_CHNL)).unwrap();
    spawner.spawn(air_sensor_task(air_sensor_ref, &AIR_CTRL_CHNL, &DATA_CHNL)).unwrap();
    spawner
        .spawn(soil_sensor_task(soil_sensor_ref, &SOIL_CTRL_CHNL, &DATA_CHNL))
        .unwrap();

    AIR_CTRL_CHNL.send(AirSensorCommand::Init).await;
    SOIL_CTRL_CHNL.send(SoilSensorCommand::Init).await;

    Timer::after_secs(5).await;

    RADIO_CTRL_CHNL
        .send(RadioCommand::Join(JoinMode::OTAA {
            deveui: DevEui::from(config::Config::DEV_EUI),
            appeui: AppEui::from(config::Config::APP_EUI),
            appkey: AppKey::from(config::Config::APP_KEY),
        }))
        .await;

    loop {
        Timer::after_secs(30).await;

        AIR_CTRL_CHNL.send(AirSensorCommand::Measure).await;
        SOIL_CTRL_CHNL.send(SoilSensorCommand::Measure).await;

        for _ in 0..2 {
            match DATA_CHNL.receive().await {
                SensorData::Air(tmp, hum, co2) => {
                    info!(
                        "received air sensor measurements: temperature {:?} humidity {:?} co2 levels {:?}",
                        tmp, hum, co2
                    );
                    let air_tmp_scaled = (tmp * 10.0) as i16;
                    let air_hum_scaled = (hum * 2.0) as u8;
                    let payload: [u8; 11] = [
                        // encoding using Cayenne LPP codec
                        0x01,                        // channel     - 1 [air_sensor]
                        0x67,                        // type        - temperature [2 bytes]
                        (air_tmp_scaled >> 8) as u8, //             - first byte
                        air_tmp_scaled as u8,        //             - second byte
                        0x01,                        // channel     - 1 [air_sensor]
                        0x68,                        // type        - humidity [1 byte]
                        air_hum_scaled,              //             - first byte
                        0x01,                        // channel     - 1 [air_sensor]
                        0x02,                        // type        - analog input [2 bytes]
                        (co2 >> 8) as u8,            //             - first byte
                        co2 as u8,                   //             - second byte
                    ];
                    RADIO_CTRL_CHNL.send(RadioCommand::UplinkAirData(payload)).await;
                }
                SensorData::Soil(tmp, mst) => {
                    info!("received soil sensor measurements: temperature {:?} moisture {:?}", tmp, mst);
                    let soil_tmp_scaled = (tmp * 10.0) as i16;
                    let payload: [u8; 8] = [
                        // encoding using Cayenne LPP codec
                        0x02,                         // channel    - 2 [soil_sensor]
                        0x67,                         // type       - temperature [2 bytes]
                        (soil_tmp_scaled >> 8) as u8, //            - first byte
                        soil_tmp_scaled as u8,        //            - second byte
                        0x02,                         // channel    - 2 [soil_sensor]
                        0x02,                         // type       - analog input [2 bytes]
                        (mst >> 8) as u8,             //            - first byte
                        mst as u8,                    //            - second byte
                    ];
                    RADIO_CTRL_CHNL.send(RadioCommand::UplinkSoilData(payload)).await;
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn radio_module_task(
    radio_mutex: &'static Mutex<ThreadModeRawMutex, RadioDevice>,
    radio_channel: &'static Channel<ThreadModeRawMutex, RadioCommand, 10>,
) {
    loop {
        match radio_channel.receive().await {
            RadioCommand::Join(join_mode) => {
                info!("joining lora network");

                let mut radio = radio_mutex.lock().await;
                let response = radio.join(&join_mode).await;

                match response {
                    Ok(_) => info!("successfully joined"),
                    Err(_) => warn!("failed to join"),
                }
            }
            RadioCommand::UplinkAirData(vec) => {
                info!("sending uplink message {:?}", vec);

                let mut radio = radio_mutex.lock().await;
                let response = radio.send(&vec, 1, true).await;

                match response {
                    Ok(_) => info!("successfully sent uplink"),
                    Err(_) => warn!("failed to send uplink"),
                }
            }
            RadioCommand::UplinkSoilData(vec) => {
                info!("sending uplink message {:?}", vec);

                let mut radio = radio_mutex.lock().await;
                let response = radio.send(&vec, 1, true).await;

                match response {
                    Ok(_) => info!("successfully sent uplink"),
                    Err(_) => warn!("failed to send uplink"),
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn air_sensor_task(
    air_sensor_mutex: &'static Mutex<ThreadModeRawMutex, AirSensor<'static>>,
    command_channel: &'static Channel<ThreadModeRawMutex, AirSensorCommand, 4>,
    data_channel: &'static Channel<ThreadModeRawMutex, SensorData, 10>,
) {
    loop {
        match command_channel.receive().await {
            AirSensorCommand::Init => {
                let mut air_sensor = air_sensor_mutex.lock().await;

                info!("initializing air sensor");

                match air_sensor.init().await {
                    Ok(id) => info!("succesfully initialized air sensor [id:{:?}]", id),
                    Err(_) => warn!("failed to initialize air sensor"),
                }
            }
            AirSensorCommand::Measure => {
                let mut air_sensor = air_sensor_mutex.lock().await;

                info!("reading air temperature, humidty and co2 levels");

                match air_sensor.measure().await {
                    Ok((tmp, hum, co2)) => data_channel.send(SensorData::Air(tmp, hum, co2)).await,
                    Err(_) => warn!("failed to read air sensor data"),
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn soil_sensor_task(
    soil_sensor_mutex: &'static Mutex<ThreadModeRawMutex, SoilSensor<'static>>,
    command_channel: &'static Channel<ThreadModeRawMutex, SoilSensorCommand, 4>,
    data_channel: &'static Channel<ThreadModeRawMutex, SensorData, 10>,
) {
    loop {
        match command_channel.receive().await {
            SoilSensorCommand::Init => {
                let mut soil_sensor = soil_sensor_mutex.lock().await;

                info!("initializing soil sensor");

                match soil_sensor.init().await {
                    Ok(id) => info!("succesfully initialized soil sensor [id:{:?}]", id),
                    Err(_) => warn!("failed to initialize soil sensor"),
                }
            }
            SoilSensorCommand::Measure => {
                let mut soil_sensor = soil_sensor_mutex.lock().await;

                info!("reading soil temperature and moisture");

                match soil_sensor.measure().await {
                    Ok((tmp, mst)) => data_channel.send(SensorData::Soil(tmp, mst)).await,
                    Err(_) => warn!("failed to read soil sensor data"),
                }
            }
        }
    }
}
