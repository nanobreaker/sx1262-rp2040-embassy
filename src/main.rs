#![no_std]
#![no_main]

mod air_sensor;
mod board;
mod board_sensor;
mod config;
mod device;
mod error;
mod sensor;
mod soil_sensor;
mod transceiver;

use air_sensor::AirSensor;
use assign_resources::assign_resources;
use board::{Board, BoardBuilder};
use board_sensor::BoardSensor;
use device::Device;
use embassy_executor::Spawner;
use embassy_rp::peripherals::{I2C0, I2C1};
use embassy_rp::{bind_interrupts, peripherals};
use embassy_time::{Duration, Ticker, Timer};
use soil_sensor::SoilSensor;
use transceiver::{RadioDevice, Transceiver};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ADC_IRQ_FIFO => embassy_rp::adc::InterruptHandler;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<I2C0>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
});

assign_resources! {
    board: BoardSensorResources {
        adc: ADC,
        adc_temp_sensor: ADC_TEMP_SENSOR,
        pin_24: PIN_24,
        pin_26: PIN_26,
        pin_29: PIN_29,
    },
    air: AirSensorResources {
        pin_16: PIN_16,
        pin_17: PIN_17,
        i2c0: I2C0,
    },
    soil: SoilSensorResources {
        pin_18: PIN_18,
        pin_19: PIN_19,
        i2c1: I2C1,
    },
    xcvr: TransceiverResources{
        pin_2: PIN_2,
        pin_3: PIN_3,
        pin_10: PIN_10,
        pin_11: PIN_11,
        pin_12: PIN_12,
        pin_15: PIN_15,
        pin_20: PIN_20,
        dma_ch0: DMA_CH0,
        dma_ch1: DMA_CH1,
        spi1: SPI1,
    }
}

enum State {
    Auth,
    Run,
    Sleep,
}

#[embassy_executor::main]
async fn main(s: Spawner) {
    let peripherals = embassy_rp::init(Default::default());
    let res = split_resources! {peripherals};
    let board_sensor = BoardSensor::prepare(res.board).await.expect("board sensors should be functional");
    let air_sensor = AirSensor::prepare(res.air).await.expect("air sensor should be connected");
    let soil_sensor = SoilSensor::prepare(res.soil).await.expect("soil sensor should be connected");
    let radio = RadioDevice::prepare(res.xcvr).await.expect("radio module should be connected");
    let mut board = BoardBuilder::new()
        .with_board_sensor(board_sensor)
        .with_air_sensor(air_sensor)
        .with_soil_sensor(soil_sensor)
        .with_radio(radio)
        .build()
        .expect("all devices should be connected");

    if let Ok(()) = board.init().await {
        s.spawn(orchestator(board)).expect("embassy executor should be available")
    } else {
        defmt::error!("Reseting device")
    }
}

#[embassy_executor::task]
async fn orchestator(mut board: Board) {
    let mut ticker = Ticker::every(Duration::from_secs(30));
    let mut state = State::Auth;
    let mut auth_counter: u8 = 0;
    loop {
        state = match state {
            State::Auth => {
                defmt::info!("Entering authentication mode");

                let mut transceiver = board.transceiver.lock().await;

                match transceiver.join_otaa().await {
                    Ok(()) => {
                        defmt::info!("Joined lorawan network");
                        State::Run
                    }
                    Err(e) => {
                        defmt::error!("Failed to join lorawan network {:?}", e);
                        if auth_counter >= 5 {
                            State::Sleep
                        } else {
                            auth_counter += 1;
                            State::Auth
                        }
                    }
                }
            }
            State::Run => {
                defmt::info!("Entering duty cycle mode");

                if let Ok(data) = board.collect_data().await {
                    match board.uplink(data.as_slice()).await {
                        Ok(()) => State::Run,
                        Err(_) => State::Sleep,
                    }
                } else {
                    State::Sleep
                }
            }
            State::Sleep => {
                defmt::info!("Entering sleep mode");
                Timer::after_secs(60 * 10).await;
                State::Auth
            }
        };
        ticker.next().await;
    }
}
