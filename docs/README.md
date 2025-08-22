# Preview

![preview_0](screenshot-0.png)
![preview_1](screenshot-1.png)

# Overview

This is a POC project that implements a LoRaWAN sensor node using the SX1262 transceiver and RP2040 microcontroller, leveraging the Embassy framework for efficient, asynchronous embedded programming.
The node collects data from air (temperature, humidity, CO2) and soil (temperature, moisture) sensors, encodes and transmits it over LoRaWAN.

# Prerequisites

## Raspberry Pi Pico
Main board to run the project and orchestrate devices

## Raspberry Pi Debug Probe
Debug probe to ease deployment process and give you logs via defmt crate

## SX1262 LoRa Node Module
Transciever that supports LoRa modulation

## Embedded Toolkit (probe-rs)
Toolkit to let you programm pico via debug probe

## LoRaWAN Gateway
Gateway which recieves uplink messages from devices as ours

## LoRaWAN Network Server
Network server that processes uplink messages and acts as authentication server

## Rust toolchain with thumbv6m-none-eabi support
See rust-toolchain.toml

# Getting Started

## Install
  ```shell
  git clone https://github.com/nanobreaker/sx1262-rp2040-embassy.git
  ```

## Build
  ```shell
  cargo build
  ```

## Deploy
  ```shell
  cargo embed
  ```

## Wiring

<div style="margin-top: 2rem" align="center">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="wiring-dark.png">
      <img alt="demo" src="wiring-light.png">
    </picture>
</div>

## Project Structure

- device
  - mod.rs
- sensor
  - mod.rs
  - system_sensor.rs
  - soil_sensor.rs
  - air_sensor.rs
- storage
  - mod.rs
  - flash_storage.rs
- radio
  - mod.rs
  - lora_radio.rs
- config
  - mod.rs
- main.rs

# License

The code in this project is licensed under MIT license. Check [LICENSE](LICENSE.md) for further
details.
