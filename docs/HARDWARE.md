# Hardware Reference — TE Stem Player (SP-1)

## Device Overview

| Spec | Value |
|------|-------|
| Name | Teenage Engineering SP-1 Stem Player |
| Designed by | Kano Computing + Kanye West |
| Form Factor | Circular, ~7cm diameter, beige |
| USB | USB-C (data + charging) |
| USB VID:PID | `0x2367:0x1701` |

## Key Components

### MCU — Nordic nRF52840
- ARM Cortex-M4F @ 64MHz
- 1MB Flash, 256KB RAM
- USB 2.0 Full Speed
- Bluetooth 5.0 (BLE)
- I2C, SPI, I2S, UART, SAADC
- Firmware starts at flash offset `0x20000`, max size `0xDEFFF`

### Speaker Amplifier — TI TAS2505
- Mono Class-D speaker amplifier with DAC
- I2C controlled (address configurable)
- Receives audio via I2S from MCU

### Headphone Codec — Cirrus Logic CS42L42
- Stereo headphone amplifier + ADC/DAC
- I2C controlled
- Provides I2S LRCLK (audio clock master)
- Jack detection for auto-switching

### Storage — Toshiba THGBMNG5D1LBAIL
- 4GB eMMC 5.0
- 1-bit eMMC interface (DAT0, CMD, CLK only)
- No filesystem — raw sector-based storage
- 8192-byte sector size

### Bluetooth — Infineon CYBT-353027-02
- BLE Audio module
- UART controlled from MCU
- Receives audio via I2S
- Used for wireless audio streaming

### Battery Charger — TI BQ24232
- USB-friendly single-cell Li-ion charger
- Charges via USB-C VBUS

### Audio Clock
- 3.072 MHz oscillator
- Feeds I2S bus for 48kHz × 24-bit × 2ch = 2.304 MHz bit clock

## Audio Bus Architecture

```
                    I2S Bus (48kHz, 24-bit)
                           │
     ┌─────────────────────┼──────────────────────┐
     │                     │                      │
┌────┴─────┐        ┌─────┴──────┐        ┌──────┴──────┐
│ TAS2505  │        │  CS42L42   │        │ CYBT-353027 │
│ Speaker  │        │ Headphone  │        │ Bluetooth   │
│ Amp      │        │ Codec      │        │ Module      │
└──────────┘        └────────────┘        └─────────────┘
                     (LRCLK master)
```

## Bootloader Notes

- Bootloader occupies flash `0x00000–0x1FFFF`
- Application starts at `0x20000`
- On boot, bootloader initializes:
  - Watchdog timer (5-second timeout)
  - LFCLK (low-frequency clock)
  - HFCLK (high-frequency clock)
  - PWM2, PWM3
  - SAADC (analog-to-digital)
- **No hard reset available** — must implement `SYSTEM_OFF` to return to bootloader
- Web-based firmware updater: [solderless.engineering](https://solderless.engineering)

## Firmware Development

### SDK Options
- **nRF5 SDK v17.0.2** (legacy, stable)
- **nRF Connect SDK** (Zephyr-based, modern)

### Key Resources
- Hardware wiki & pin definitions: [github.com/timknapen/SP-1-dev](https://github.com/timknapen/SP-1-dev)
- USB reverse engineering: [github.com/leabs/yzy-stemplayer-reverse](https://github.com/leabs/yzy-stemplayer-reverse)
- Pin mapping header: `stemplayer_pins.h` in SP-1-dev repo

## Pin Summary

Refer to `stemplayer_pins.h` for full mapping. Key connections:

| Function | Pin(s) | Notes |
|----------|--------|-------|
| eMMC DAT0 | GPIO | 1-bit data |
| eMMC CMD | GPIO | Command line |
| eMMC CLK | GPIO | Clock |
| I2S SCK | GPIO | Bit clock |
| I2S LRCK | GPIO | From CS42L42 |
| I2S SDOUT | GPIO | MCU → DACs |
| I2S SDIN | GPIO | ADC → MCU |
| I2C SDA | GPIO | Shared bus |
| I2C SCL | GPIO | Shared bus |
| UART TX | GPIO | → BT module |
| UART RX | GPIO | ← BT module |
| USB D+ | GPIO | USB data |
| USB D- | GPIO | USB data |
