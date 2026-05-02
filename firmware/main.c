/**
 * SP-1 Stem Player USB-eMMC Bridge Firmware
 * 
 * Custom firmware for nRF52840 on the Teenage Engineering SP-1 Stem Player.
 * Provides USB CDC ACM serial interface for reading/writing eMMC storage.
 * 
 * eMMC Pins:
 *   CLK  = P0.06
 *   DAT0 = P0.07
 *   CMD  = P0.08
 *   RST  = P1.08
 *   VCCQ = P0.14 (power enable)
 *
 * Protocol (text over CDC ACM):
 *   STATUS        -> "OK <total_sectors> <sector_size>"
 *   READ <sector> -> binary 8192 bytes
 *   WRITE <sector> <- binary 8192 bytes, then "OK" or "ERR"
 *   BOOTLOADER    -> reset to bootloader
 *   INFO          -> device info string
 */

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include "nrf.h"
#include "nrf_drv_usbd.h"
#include "nrf_drv_clock.h"
#include "nrf_gpio.h"
#include "nrf_delay.h"
#include "nrf_drv_power.h"

#include "app_error.h"
#include "app_util.h"
#include "app_usbd_core.h"
#include "app_usbd.h"
#include "app_usbd_string_desc.h"
#include "app_usbd_cdc_acm.h"
#include "app_usbd_serial_num.h"

/* ── Pin Definitions ───────────────────────────────────────────── */
#define EMMC_CLK_PIN    NRF_GPIO_PIN_MAP(0, 6)
#define EMMC_DAT0_PIN   NRF_GPIO_PIN_MAP(0, 7)
#define EMMC_CMD_PIN    NRF_GPIO_PIN_MAP(0, 8)
#define EMMC_RST_PIN    NRF_GPIO_PIN_MAP(1, 8)
#define EMMC_VCCQ_PIN   NRF_GPIO_PIN_MAP(0, 14)

/* ── eMMC Constants ────────────────────────────────────────────── */
#define EMMC_BLOCK_SIZE     512
#define EMMC_BLOCKS_PER_SECTOR  16
#define SECTOR_SIZE         (EMMC_BLOCK_SIZE * EMMC_BLOCKS_PER_SECTOR)  /* 8192 */
#define EMMC_TOTAL_SECTORS  524288  /* 4GB / 8192 = 524288 sectors */

/* eMMC Commands */
#define CMD0    0   /* GO_IDLE_STATE */
#define CMD1    1   /* SEND_OP_COND */
#define CMD2    2   /* ALL_SEND_CID */
#define CMD3    3   /* SEND_RELATIVE_ADDR */
#define CMD7    7   /* SELECT_CARD */
#define CMD8    8   /* SEND_EXT_CSD */
#define CMD13   13  /* SEND_STATUS */
#define CMD16   16  /* SET_BLOCKLEN */
#define CMD17   17  /* READ_SINGLE_BLOCK */
#define CMD24   24  /* WRITE_BLOCK */

/* ── Watchdog ──────────────────────────────────────────────────── */
static void wdt_feed(void)
{
    if (NRF_WDT->RUNSTATUS & WDT_RUNSTATUS_RUNSTATUS_Msk) {
        NRF_WDT->RR[0] = WDT_RR_RR_Reload;
    }
}

/* ── eMMC Bit-Bang Driver ──────────────────────────────────────── */

static volatile bool m_emmc_initialized = false;
static uint16_t m_emmc_rca = 0;

static inline void emmc_clk_high(void)  { nrf_gpio_pin_set(EMMC_CLK_PIN); }
static inline void emmc_clk_low(void)   { nrf_gpio_pin_clear(EMMC_CLK_PIN); }
static inline void emmc_cmd_high(void)  { nrf_gpio_pin_set(EMMC_CMD_PIN); }
static inline void emmc_cmd_low(void)   { nrf_gpio_pin_clear(EMMC_CMD_PIN); }

static inline void emmc_clock_cycle(void)
{
    emmc_clk_high();
    __NOP(); __NOP(); __NOP(); __NOP();
    emmc_clk_low();
    __NOP(); __NOP(); __NOP(); __NOP();
}

static void emmc_send_clocks(uint32_t count)
{
    nrf_gpio_pin_set(EMMC_CMD_PIN);
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    for (uint32_t i = 0; i < count; i++) {
        emmc_clock_cycle();
    }
}

/* Send a command on the CMD line (48-bit frame) */
static void emmc_send_cmd_bits(uint8_t cmd_idx, uint32_t arg)
{
    /* Build 48-bit command: start(0) + transmission(1) + cmd(6) + arg(32) + crc7(7) + stop(1) */
    uint8_t frame[6];
    frame[0] = 0x40 | (cmd_idx & 0x3F);
    frame[1] = (arg >> 24) & 0xFF;
    frame[2] = (arg >> 16) & 0xFF;
    frame[3] = (arg >> 8) & 0xFF;
    frame[4] = arg & 0xFF;

    /* Calculate CRC7 */
    uint8_t crc = 0;
    for (int i = 0; i < 5; i++) {
        uint8_t d = frame[i];
        for (int j = 7; j >= 0; j--) {
            crc = (crc << 1) | ((d >> j) & 1);
            if (crc & 0x80) crc ^= 0x09;  /* polynomial x^7 + x^3 + 1 */
        }
    }
    frame[5] = (crc << 1) | 0x01;  /* CRC7 + stop bit */

    /* Configure CMD as output */
    nrf_gpio_cfg_output(EMMC_CMD_PIN);

    /* Send 48 bits, MSB first */
    for (int i = 0; i < 6; i++) {
        for (int j = 7; j >= 0; j--) {
            if (frame[i] & (1 << j))
                nrf_gpio_pin_set(EMMC_CMD_PIN);
            else
                nrf_gpio_pin_clear(EMMC_CMD_PIN);
            emmc_clock_cycle();
        }
    }

    /* Release CMD line */
    nrf_gpio_cfg_input(EMMC_CMD_PIN, NRF_GPIO_PIN_PULLUP);
}

/* Wait for and read a 48-bit response (R1/R3/R6) on CMD line.
   Returns the 32-bit card status/payload. Returns 0xFFFFFFFF on timeout. */
static uint32_t emmc_read_r1(void)
{
    uint32_t timeout = 1000000;
    uint8_t response[6];

    /* Wait for start bit (CMD goes low) */
    while (nrf_gpio_pin_read(EMMC_CMD_PIN) && --timeout) {
        emmc_clock_cycle();
    }
    if (timeout == 0) return 0xFFFFFFFF;

    /* Read 48 bits */
    for (int i = 0; i < 48; i++) {
        int byte_idx = i / 8;
        int bit_idx = 7 - (i % 8);
        if (i == 0) {
            response[0] = 0;  /* start bit is 0, already consumed */
        }
        if (nrf_gpio_pin_read(EMMC_CMD_PIN))
            response[byte_idx] |= (1 << bit_idx);
        else
            response[byte_idx] &= ~(1 << bit_idx);
        emmc_clock_cycle();
    }

    /* Extract 32-bit status from response bytes [1..4] */
    uint32_t status = ((uint32_t)response[1] << 24) |
                      ((uint32_t)response[2] << 16) |
                      ((uint32_t)response[3] << 8)  |
                      (uint32_t)response[4];
    return status;
}

/* Read a 136-bit response (R2) for CID/CSD. Returns first 4 bytes of CID. */
static uint32_t emmc_read_r2(void)
{
    uint32_t timeout = 1000000;

    while (nrf_gpio_pin_read(EMMC_CMD_PIN) && --timeout) {
        emmc_clock_cycle();
    }
    if (timeout == 0) return 0xFFFFFFFF;

    /* Read 136 bits total, we only care that it completes */
    uint32_t first_word = 0;
    for (int i = 0; i < 136; i++) {
        uint32_t bit = nrf_gpio_pin_read(EMMC_CMD_PIN) ? 1 : 0;
        if (i >= 8 && i < 40) {
            first_word = (first_word << 1) | bit;
        }
        emmc_clock_cycle();
    }
    return first_word;
}

/* Send command and get R1 response */
static uint32_t emmc_cmd(uint8_t cmd_idx, uint32_t arg)
{
    emmc_send_cmd_bits(cmd_idx, arg);
    if (cmd_idx == CMD0) {
        /* CMD0 has no response */
        emmc_send_clocks(8);
        return 0;
    }
    if (cmd_idx == CMD2) {
        return emmc_read_r2();
    }
    return emmc_read_r1();
}

/* Initialize eMMC in 1-bit mode */
static bool emmc_init(void)
{
    /* Power on eMMC */
    nrf_gpio_cfg_output(EMMC_VCCQ_PIN);
    nrf_gpio_pin_set(EMMC_VCCQ_PIN);
    nrf_delay_ms(10);

    /* Reset eMMC */
    nrf_gpio_cfg_output(EMMC_RST_PIN);
    nrf_gpio_pin_clear(EMMC_RST_PIN);
    nrf_delay_ms(5);
    nrf_gpio_pin_set(EMMC_RST_PIN);
    nrf_delay_ms(10);

    /* Configure pins */
    nrf_gpio_cfg_output(EMMC_CLK_PIN);
    nrf_gpio_pin_clear(EMMC_CLK_PIN);
    nrf_gpio_cfg_input(EMMC_CMD_PIN, NRF_GPIO_PIN_PULLUP);
    nrf_gpio_cfg_input(EMMC_DAT0_PIN, NRF_GPIO_PIN_PULLUP);

    /* Send 80+ clock cycles with CMD/DAT high */
    emmc_send_clocks(80);

    wdt_feed();

    /* CMD0: GO_IDLE_STATE */
    emmc_cmd(CMD0, 0);
    nrf_delay_ms(5);

    /* CMD1: SEND_OP_COND - poll until ready */
    uint32_t resp;
    for (int i = 0; i < 1000; i++) {
        wdt_feed();
        resp = emmc_cmd(CMD1, 0x40FF8080);  /* sector mode, voltage range */
        if (resp & 0x80000000) break;  /* ready bit set */
        nrf_delay_ms(1);
    }
    if (!(resp & 0x80000000)) return false;

    /* CMD2: ALL_SEND_CID */
    emmc_cmd(CMD2, 0);

    /* CMD3: SET_RELATIVE_ADDR */
    resp = emmc_cmd(CMD3, 0x00010000);  /* RCA = 1 */
    m_emmc_rca = 1;

    /* CMD7: SELECT_CARD */
    resp = emmc_cmd(CMD7, (uint32_t)m_emmc_rca << 16);

    /* CMD16: SET_BLOCKLEN to 512 */
    resp = emmc_cmd(CMD16, EMMC_BLOCK_SIZE);

    m_emmc_initialized = true;
    return true;
}

/* Read a single 512-byte block from eMMC */
static bool emmc_read_block(uint32_t block_addr, uint8_t *buf)
{
    /* CMD17: READ_SINGLE_BLOCK */
    emmc_send_cmd_bits(CMD17, block_addr);
    uint32_t r1 = emmc_read_r1();
    if (r1 == 0xFFFFFFFF) return false;

    /* Wait for start bit on DAT0 (goes low) */
    uint32_t timeout = 1000000;
    while (nrf_gpio_pin_read(EMMC_DAT0_PIN) && --timeout) {
        emmc_clock_cycle();
    }
    if (timeout == 0) return false;

    /* Read 512 bytes (4096 bits) on DAT0, 1-bit mode */
    for (int i = 0; i < EMMC_BLOCK_SIZE; i++) {
        uint8_t byte = 0;
        for (int j = 7; j >= 0; j--) {
            emmc_clk_high();
            __NOP(); __NOP();
            if (nrf_gpio_pin_read(EMMC_DAT0_PIN))
                byte |= (1 << j);
            emmc_clk_low();
            __NOP(); __NOP();
        }
        buf[i] = byte;
    }

    /* Read 16-bit CRC (discard) */
    for (int i = 0; i < 16; i++) {
        emmc_clock_cycle();
    }

    /* End bit */
    emmc_clock_cycle();

    return true;
}

/* Write a single 512-byte block to eMMC */
static bool emmc_write_block(uint32_t block_addr, const uint8_t *buf)
{
    /* CMD24: WRITE_BLOCK */
    emmc_send_cmd_bits(CMD24, block_addr);
    uint32_t r1 = emmc_read_r1();
    if (r1 == 0xFFFFFFFF) return false;

    /* Switch DAT0 to output */
    nrf_gpio_cfg_output(EMMC_DAT0_PIN);

    /* Send a few clocks */
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    for (int i = 0; i < 8; i++) emmc_clock_cycle();

    /* Start bit */
    nrf_gpio_pin_clear(EMMC_DAT0_PIN);
    emmc_clock_cycle();

    /* Send 512 bytes on DAT0 */
    for (int i = 0; i < EMMC_BLOCK_SIZE; i++) {
        uint8_t byte = buf[i];
        for (int j = 7; j >= 0; j--) {
            if (byte & (1 << j))
                nrf_gpio_pin_set(EMMC_DAT0_PIN);
            else
                nrf_gpio_pin_clear(EMMC_DAT0_PIN);
            emmc_clock_cycle();
        }
    }

    /* CRC16 - send dummy (0xFFFF) */
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    for (int i = 0; i < 16; i++) emmc_clock_cycle();

    /* End bit */
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    emmc_clock_cycle();

    /* Switch DAT0 back to input, wait for busy (DAT0 low) to clear */
    nrf_gpio_cfg_input(EMMC_DAT0_PIN, NRF_GPIO_PIN_PULLUP);

    uint32_t timeout = 5000000;
    while (!nrf_gpio_pin_read(EMMC_DAT0_PIN) && --timeout) {
        emmc_clock_cycle();
        wdt_feed();
    }

    return timeout > 0;
}

/* ── USB CDC ACM ───────────────────────────────────────────────── */

static void cdc_acm_user_ev_handler(app_usbd_class_inst_t const *p_inst,
                                     app_usbd_cdc_acm_user_event_t event);

#define CDC_ACM_COMM_INTERFACE  0
#define CDC_ACM_COMM_EPIN       NRF_DRV_USBD_EPIN2
#define CDC_ACM_DATA_INTERFACE  1
#define CDC_ACM_DATA_EPIN       NRF_DRV_USBD_EPIN1
#define CDC_ACM_DATA_EPOUT      NRF_DRV_USBD_EPOUT1

APP_USBD_CDC_ACM_GLOBAL_DEF(m_app_cdc_acm,
                             cdc_acm_user_ev_handler,
                             CDC_ACM_COMM_INTERFACE,
                             CDC_ACM_DATA_INTERFACE,
                             CDC_ACM_COMM_EPIN,
                             CDC_ACM_DATA_EPIN,
                             CDC_ACM_DATA_EPOUT,
                             APP_USBD_CDC_COMM_PROTOCOL_AT_V250);

/* ── Command Processing ────────────────────────────────────────── */

#define CMD_BUF_SIZE    64
#define DATA_BUF_SIZE   SECTOR_SIZE

static char m_cmd_buf[CMD_BUF_SIZE];
static uint32_t m_cmd_len = 0;
static uint8_t m_data_buf[DATA_BUF_SIZE];
static uint8_t m_rx_buf[64];

static bool m_port_open = false;
static bool m_write_pending = false;
static uint32_t m_write_sector = 0;
static uint32_t m_write_received = 0;

static void usb_send_string(const char *str)
{
    size_t len = strlen(str);
    app_usbd_cdc_acm_write(&m_app_cdc_acm, str, len);
}

static void usb_send_data(const uint8_t *data, size_t len)
{
    /* Send in chunks of 64 bytes (USB FS max packet) */
    while (len > 0) {
        size_t chunk = (len > 64) ? 64 : len;
        app_usbd_cdc_acm_write(&m_app_cdc_acm, data, chunk);
        data += chunk;
        len -= chunk;
        wdt_feed();
        /* Small delay for USB to process */
        nrf_delay_us(100);
    }
}

static void process_command(void)
{
    m_cmd_buf[m_cmd_len] = '\0';

    if (strcmp(m_cmd_buf, "STATUS") == 0) {
        char resp[64];
        snprintf(resp, sizeof(resp), "OK %u %u\r\n",
                 (unsigned)EMMC_TOTAL_SECTORS, (unsigned)SECTOR_SIZE);
        usb_send_string(resp);
    }
    else if (strcmp(m_cmd_buf, "INFO") == 0) {
        const char *info = "SP1-BRIDGE v1.0 nRF52840 eMMC-USB\r\n";
        usb_send_string(info);
    }
    else if (strncmp(m_cmd_buf, "READ ", 5) == 0) {
        uint32_t sector = (uint32_t)strtoul(m_cmd_buf + 5, NULL, 10);
        if (sector >= EMMC_TOTAL_SECTORS) {
            usb_send_string("ERR INVALID_SECTOR\r\n");
        } else if (!m_emmc_initialized) {
            usb_send_string("ERR EMMC_NOT_READY\r\n");
        } else {
            /* Read 16 blocks (one sector = 8192 bytes) */
            uint32_t base_block = sector * EMMC_BLOCKS_PER_SECTOR;
            bool ok = true;
            for (int i = 0; i < EMMC_BLOCKS_PER_SECTOR && ok; i++) {
                ok = emmc_read_block(base_block + i,
                                     m_data_buf + (i * EMMC_BLOCK_SIZE));
                wdt_feed();
            }
            if (ok) {
                usb_send_string("OK\r\n");
                usb_send_data(m_data_buf, SECTOR_SIZE);
            } else {
                usb_send_string("ERR READ_FAILED\r\n");
            }
        }
    }
    else if (strncmp(m_cmd_buf, "WRITE ", 6) == 0) {
        uint32_t sector = (uint32_t)strtoul(m_cmd_buf + 6, NULL, 10);
        if (sector >= EMMC_TOTAL_SECTORS) {
            usb_send_string("ERR INVALID_SECTOR\r\n");
        } else if (!m_emmc_initialized) {
            usb_send_string("ERR EMMC_NOT_READY\r\n");
        } else {
            m_write_pending = true;
            m_write_sector = sector;
            m_write_received = 0;
            usb_send_string("READY\r\n");
        }
    }
    else if (strcmp(m_cmd_buf, "BOOTLOADER") == 0) {
        usb_send_string("OK REBOOTING\r\n");
        nrf_delay_ms(100);
        /* Clear reset reason and trigger system reset */
        NRF_POWER->RESETREAS = NRF_POWER->RESETREAS;
        NVIC_SystemReset();
    }
    else if (strcmp(m_cmd_buf, "INIT") == 0) {
        if (emmc_init()) {
            usb_send_string("OK EMMC_READY\r\n");
        } else {
            usb_send_string("ERR EMMC_INIT_FAILED\r\n");
        }
    }
    else {
        usb_send_string("ERR UNKNOWN_CMD\r\n");
    }

    m_cmd_len = 0;
}

static void handle_rx_data(const uint8_t *data, size_t len)
{
    if (m_write_pending) {
        /* Receiving binary data for write */
        for (size_t i = 0; i < len && m_write_received < SECTOR_SIZE; i++) {
            m_data_buf[m_write_received++] = data[i];
        }
        if (m_write_received >= SECTOR_SIZE) {
            /* Write all 16 blocks */
            uint32_t base_block = m_write_sector * EMMC_BLOCKS_PER_SECTOR;
            bool ok = true;
            for (int i = 0; i < EMMC_BLOCKS_PER_SECTOR && ok; i++) {
                ok = emmc_write_block(base_block + i,
                                      m_data_buf + (i * EMMC_BLOCK_SIZE));
                wdt_feed();
            }
            m_write_pending = false;
            if (ok) {
                usb_send_string("OK WRITTEN\r\n");
            } else {
                usb_send_string("ERR WRITE_FAILED\r\n");
            }
        }
    } else {
        /* Command mode - accumulate until \n or \r */
        for (size_t i = 0; i < len; i++) {
            if (data[i] == '\n' || data[i] == '\r') {
                if (m_cmd_len > 0) {
                    process_command();
                }
            } else if (m_cmd_len < CMD_BUF_SIZE - 1) {
                m_cmd_buf[m_cmd_len++] = (char)data[i];
            }
        }
    }
}

static void cdc_acm_user_ev_handler(app_usbd_class_inst_t const *p_inst,
                                     app_usbd_cdc_acm_user_event_t event)
{
    switch (event) {
        case APP_USBD_CDC_ACM_USER_EVT_PORT_OPEN:
            m_port_open = true;
            /* Start reading */
            app_usbd_cdc_acm_read(&m_app_cdc_acm, m_rx_buf, sizeof(m_rx_buf));
            break;

        case APP_USBD_CDC_ACM_USER_EVT_PORT_CLOSE:
            m_port_open = false;
            break;

        case APP_USBD_CDC_ACM_USER_EVT_RX_DONE: {
            size_t bytes_read = app_usbd_cdc_acm_rx_size(&m_app_cdc_acm);
            handle_rx_data(m_rx_buf, bytes_read);
            /* Continue reading */
            app_usbd_cdc_acm_read(&m_app_cdc_acm, m_rx_buf, sizeof(m_rx_buf));
            break;
        }

        case APP_USBD_CDC_ACM_USER_EVT_TX_DONE:
            break;

        default:
            break;
    }
}

static void usbd_user_ev_handler(app_usbd_event_type_t event)
{
    switch (event) {
        case APP_USBD_EVT_STOPPED:
            app_usbd_disable();
            break;
        case APP_USBD_EVT_POWER_DETECTED:
            if (!nrf_drv_usbd_is_enabled()) {
                app_usbd_enable();
            }
            break;
        case APP_USBD_EVT_POWER_REMOVED:
            app_usbd_stop();
            break;
        case APP_USBD_EVT_POWER_READY:
            app_usbd_start();
            break;
        default:
            break;
    }
}

/* ── Main ──────────────────────────────────────────────────────── */

int main(void)
{
    /* Feed watchdog immediately - bootloader starts it */
    wdt_feed();

    /* Initialize clocks (bootloader may have already done this) */
    ret_code_t ret;

    ret = nrf_drv_clock_init();
    if (ret != NRF_SUCCESS && ret != NRF_ERROR_MODULE_ALREADY_INITIALIZED) {
        /* Non-fatal, bootloader likely already initialized */
    }

    nrf_drv_clock_lfclk_request(NULL);
    while (!nrf_drv_clock_lfclk_is_running()) {
        wdt_feed();
    }

    wdt_feed();

    /* Initialize USB */
    static const app_usbd_config_t usbd_config = {
        .ev_state_proc = usbd_user_ev_handler,
    };

    ret = nrf_drv_power_init(NULL);
    if (ret != NRF_SUCCESS && ret != NRF_ERROR_MODULE_ALREADY_INITIALIZED) {
        /* Non-fatal */
    }

    ret = app_usbd_init(&usbd_config);
    APP_ERROR_CHECK(ret);

    app_usbd_class_inst_t const *class_cdc_acm =
        app_usbd_cdc_acm_class_inst_get(&m_app_cdc_acm);
    ret = app_usbd_class_append(class_cdc_acm);
    APP_ERROR_CHECK(ret);

    app_usbd_enable();
    app_usbd_start();

    wdt_feed();

    /* Try to initialize eMMC at startup */
    emmc_init();

    /* Main loop */
    while (true) {
        wdt_feed();

        while (app_usbd_event_queue_process()) {
            /* Process USB events */
        }

        /* Power management - sleep until next event */
        __WFE();
    }
}
