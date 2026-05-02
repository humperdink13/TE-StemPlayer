/**
 * Stem Player Custom Firmware
 * 
 * Implements USB CDC ACM (virtual serial port) with COBS-framed protocol
 * for eMMC sector read/write operations.
 * 
 * Protocol (over CDC ACM serial at any baud rate):
 *   Request:  COBS([0x51, seq, cmd, plen, payload..., crc8])
 *   Response: COBS([0x51, seq, cmd+1, plen, payload..., crc8])
 * 
 * Commands:
 *   0x52 'R' - Status: returns device info
 *   0x53 'S' - Read sector: payload = LE32(sector_num), returns 4096 bytes in chunks
 *   0x57 'W' - Write sector: payload = LE32(sector_num) + data
 *   0x49 'I' - Info: returns eMMC size, serial, version
 */

#include <stdint.h>
#include <stdbool.h>
#include <string.h>

#include "nrf.h"
#include "nrf_drv_usbd.h"
#include "nrf_drv_clock.h"
#include "nrf_drv_power.h"
#include "nrf_delay.h"
#include "nrf_gpio.h"
#include "nrfx_spim.h"

#include "app_usbd_core.h"
#include "app_usbd.h"
#include "app_usbd_string_desc.h"
#include "app_usbd_cdc_acm.h"
#include "app_usbd_serial_num.h"
#include "app_error.h"

/* ── Pin definitions (from stemplayer_pins.h research) ── */
// eMMC pins (directly connected to nRF52840 GPIO)
#define EMMC_CMD_PIN    NRF_GPIO_PIN_MAP(0, 17)  // eMMC CMD
#define EMMC_CLK_PIN    NRF_GPIO_PIN_MAP(0, 19)  // eMMC CLK  
#define EMMC_DAT0_PIN   NRF_GPIO_PIN_MAP(0, 20)  // eMMC DAT0 (1-bit mode only)

// LED pins
#define LED_PIN         NRF_GPIO_PIN_MAP(0, 13)

/* ── CDC ACM setup ── */
static void cdc_acm_user_ev_handler(app_usbd_class_inst_t const * p_inst,
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

/* ── Protocol constants ── */
#define SECTOR_SIZE     8192
#define CHUNK_SIZE      240
#define MAX_PACKET_SIZE 512
#define PROTOCOL_MAGIC  0x51

/* ── CRC8 table ── */
static const uint8_t crc8_table[256] = {
    0xea,0xd4,0x96,0xa8,0x12,0x2c,0x6e,0x50,0x7f,0x41,0x03,0x3d,0x87,0xb9,0xfb,0xc5,
    0xa5,0x9b,0xd9,0xe7,0x5d,0x63,0x21,0x1f,0x30,0x0e,0x4c,0x72,0xc8,0xf6,0xb4,0x8a,
    0x74,0x4a,0x08,0x36,0x8c,0xb2,0xf0,0xce,0xe1,0xdf,0x9d,0xa3,0x19,0x27,0x65,0x5b,
    0x3b,0x05,0x47,0x79,0xc3,0xfd,0xbf,0x81,0xae,0x90,0xd2,0xec,0x56,0x68,0x2a,0x14,
    0xb3,0x8d,0xcf,0xf1,0x4b,0x75,0x37,0x09,0x26,0x18,0x5a,0x64,0xde,0xe0,0xa2,0x9c,
    0xfc,0xc2,0x80,0xbe,0x04,0x3a,0x78,0x46,0x69,0x57,0x15,0x2b,0x91,0xaf,0xed,0xd3,
    0x2d,0x13,0x51,0x6f,0xd5,0xeb,0xa9,0x97,0xb8,0x86,0xc4,0xfa,0x40,0x7e,0x3c,0x02,
    0x62,0x5c,0x1e,0x20,0x9a,0xa4,0xe6,0xd8,0xf7,0xc9,0x8b,0xb5,0x0f,0x31,0x73,0x4d,
    0x58,0x66,0x24,0x1a,0xa0,0x9e,0xdc,0xe2,0xcd,0xf3,0xb1,0x8f,0x35,0x0b,0x49,0x77,
    0x17,0x29,0x6b,0x55,0xef,0xd1,0x93,0xad,0x82,0xbc,0xfe,0xc0,0x7a,0x44,0x06,0x38,
    0xc6,0xf8,0xba,0x84,0x3e,0x00,0x42,0x7c,0x53,0x6d,0x2f,0x11,0xab,0x95,0xd7,0xe9,
    0x89,0xb7,0xf5,0xcb,0x71,0x4f,0x0d,0x33,0x1c,0x22,0x60,0x5e,0xe4,0xda,0x98,0xa6,
    0x01,0x3f,0x7d,0x43,0xf9,0xc7,0x85,0xbb,0x94,0xaa,0xe8,0xd6,0x6c,0x52,0x10,0x2e,
    0x4e,0x70,0x32,0x0c,0xb6,0x88,0xca,0xf4,0xdb,0xe5,0xa7,0x99,0x23,0x1d,0x5f,0x61,
    0x9f,0xa1,0xe3,0xdd,0x67,0x59,0x1b,0x25,0x0a,0x34,0x76,0x48,0xf2,0xcc,0x8e,0xb0,
    0xd0,0xee,0xac,0x92,0x28,0x16,0x54,0x6a,0x45,0x7b,0x39,0x07,0xbd,0x83,0xc1,0xff,
};

static uint8_t crc8(const uint8_t *data, size_t len) {
    uint8_t crc = 0;
    for (size_t i = 0; i < len; i++) crc = crc8_table[crc ^ data[i]];
    return crc;
}

/* ── COBS encoding/decoding ── */
static size_t cobs_encode(const uint8_t *input, size_t len, uint8_t *output) {
    size_t out_idx = 0;
    size_t in_idx = 0;
    while (in_idx <= len) {
        size_t end = in_idx;
        while (end < len && input[end] != 0) end++;
        output[out_idx++] = (uint8_t)(end - in_idx + 1);
        for (size_t i = in_idx; i < end; i++) output[out_idx++] = input[i];
        if (end < len) in_idx = end + 1;
        else break;
    }
    output[out_idx++] = 0x00;
    return out_idx;
}

static size_t cobs_decode(const uint8_t *input, size_t len, uint8_t *output) {
    const uint8_t *data = input;
    size_t data_len = len;
    if (data_len > 0 && data[data_len - 1] == 0) data_len--;
    
    size_t out_idx = 0, idx = 0;
    while (idx < data_len) {
        uint8_t code = data[idx++];
        if (code == 0) break;
        for (uint8_t i = 0; i < code - 1; i++) {
            if (idx < data_len) output[out_idx++] = data[idx++];
        }
        if (code < 0xFF && idx < data_len) output[out_idx++] = 0;
    }
    return out_idx;
}

/* ── eMMC driver (bit-bang, 1-bit mode) ── */
// The Stem Player uses 1-bit eMMC (CMD, CLK, DAT0 only)
// We implement a minimal eMMC driver using GPIO bit-banging

static void emmc_delay(void) {
    // ~400kHz clock - slow but reliable
    nrf_delay_us(2);
}

static void emmc_clk_pulse(void) {
    nrf_gpio_pin_set(EMMC_CLK_PIN);
    emmc_delay();
    nrf_gpio_pin_clear(EMMC_CLK_PIN);
    emmc_delay();
}

static void emmc_send_cmd(uint8_t cmd, uint32_t arg) {
    uint8_t buf[6];
    buf[0] = 0x40 | cmd;
    buf[1] = (arg >> 24) & 0xFF;
    buf[2] = (arg >> 16) & 0xFF;
    buf[3] = (arg >> 8) & 0xFF;
    buf[4] = arg & 0xFF;
    
    // Calculate CRC7
    uint8_t crc = 0;
    for (int i = 0; i < 5; i++) {
        uint8_t d = buf[i];
        for (int j = 0; j < 8; j++) {
            crc <<= 1;
            if (((d ^ crc) & 0x80) != 0) crc ^= 0x09;
            d <<= 1;
        }
    }
    buf[5] = (crc & 0xFE) | 0x01;
    
    // Send on CMD line
    nrf_gpio_cfg_output(EMMC_CMD_PIN);
    for (int i = 0; i < 6; i++) {
        for (int j = 7; j >= 0; j--) {
            if (buf[i] & (1 << j))
                nrf_gpio_pin_set(EMMC_CMD_PIN);
            else
                nrf_gpio_pin_clear(EMMC_CMD_PIN);
            emmc_clk_pulse();
        }
    }
}

static uint32_t emmc_recv_r1(void) {
    nrf_gpio_cfg_input(EMMC_CMD_PIN, NRF_GPIO_PIN_PULLUP);
    
    // Wait for start bit (0)
    for (int i = 0; i < 256; i++) {
        emmc_clk_pulse();
        if (nrf_gpio_pin_read(EMMC_CMD_PIN) == 0) {
            // Read 47 more bits (48 total for R1)
            uint32_t response = 0;
            for (int b = 0; b < 38; b++) {
                emmc_clk_pulse();
                if (b >= 6 && b < 38) {
                    response = (response << 1) | nrf_gpio_pin_read(EMMC_CMD_PIN);
                }
            }
            return response;
        }
    }
    return 0xFFFFFFFF; // Timeout
}

static bool emmc_read_block(uint32_t sector, uint8_t *buf, size_t len) {
    // CMD17: READ_SINGLE_BLOCK
    emmc_send_cmd(17, sector);
    uint32_t r1 = emmc_recv_r1();
    if (r1 == 0xFFFFFFFF) return false;
    
    // Wait for data token on DAT0
    nrf_gpio_cfg_input(EMMC_DAT0_PIN, NRF_GPIO_PIN_PULLUP);
    for (int i = 0; i < 65536; i++) {
        emmc_clk_pulse();
        if (nrf_gpio_pin_read(EMMC_DAT0_PIN) == 0) {
            // Start bit received, read data
            for (size_t byte_idx = 0; byte_idx < len; byte_idx++) {
                uint8_t val = 0;
                for (int bit = 7; bit >= 0; bit--) {
                    emmc_clk_pulse();
                    val |= (nrf_gpio_pin_read(EMMC_DAT0_PIN) << bit);
                }
                buf[byte_idx] = val;
            }
            // Read CRC (16 bits) and end bit
            for (int i = 0; i < 17; i++) emmc_clk_pulse();
            return true;
        }
    }
    return false;
}

static bool emmc_write_block(uint32_t sector, const uint8_t *buf, size_t len) {
    // CMD24: WRITE_BLOCK
    emmc_send_cmd(24, sector);
    uint32_t r1 = emmc_recv_r1();
    if (r1 == 0xFFFFFFFF) return false;
    
    // Send data token
    nrf_gpio_cfg_output(EMMC_DAT0_PIN);
    // 8 clock cycles gap
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    for (int i = 0; i < 8; i++) emmc_clk_pulse();
    
    // Start bit
    nrf_gpio_pin_clear(EMMC_DAT0_PIN);
    emmc_clk_pulse();
    
    // Data
    for (size_t byte_idx = 0; byte_idx < len; byte_idx++) {
        for (int bit = 7; bit >= 0; bit--) {
            if (buf[byte_idx] & (1 << bit))
                nrf_gpio_pin_set(EMMC_DAT0_PIN);
            else
                nrf_gpio_pin_clear(EMMC_DAT0_PIN);
            emmc_clk_pulse();
        }
    }
    
    // CRC (dummy)
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    for (int i = 0; i < 16; i++) emmc_clk_pulse();
    
    // End bit
    nrf_gpio_pin_set(EMMC_DAT0_PIN);
    emmc_clk_pulse();
    
    // Wait for busy (DAT0 low)
    nrf_gpio_cfg_input(EMMC_DAT0_PIN, NRF_GPIO_PIN_PULLUP);
    for (int i = 0; i < 500000; i++) {
        emmc_clk_pulse();
        if (nrf_gpio_pin_read(EMMC_DAT0_PIN) == 1) return true;
    }
    return false;
}

static bool emmc_initialized = false;

static bool emmc_init(void) {
    if (emmc_initialized) return true;
    
    nrf_gpio_cfg_output(EMMC_CLK_PIN);
    nrf_gpio_pin_clear(EMMC_CLK_PIN);
    nrf_gpio_cfg_output(EMMC_CMD_PIN);
    nrf_gpio_pin_set(EMMC_CMD_PIN);
    nrf_gpio_cfg_input(EMMC_DAT0_PIN, NRF_GPIO_PIN_PULLUP);
    
    // 74+ clock cycles with CMD high
    for (int i = 0; i < 80; i++) emmc_clk_pulse();
    
    // CMD0: GO_IDLE_STATE
    emmc_send_cmd(0, 0);
    nrf_delay_ms(10);
    
    // CMD1: SEND_OP_COND (eMMC specific)
    for (int retry = 0; retry < 1000; retry++) {
        emmc_send_cmd(1, 0x40FF8080); // Sector addressing + voltage
        uint32_t r = emmc_recv_r1();
        if (r != 0xFFFFFFFF && (r & 0x80000000)) {
            // CMD2: ALL_SEND_CID
            emmc_send_cmd(2, 0);
            // Read R2 response (skip for now, just clock out)
            for (int i = 0; i < 200; i++) emmc_clk_pulse();
            
            // CMD3: SET_RELATIVE_ADDR (eMMC uses assigned RCA)
            emmc_send_cmd(3, 0x00010000); // RCA = 1
            emmc_recv_r1();
            
            // CMD7: SELECT_CARD
            emmc_send_cmd(7, 0x00010000); // RCA = 1
            emmc_recv_r1();
            
            // CMD16: SET_BLOCKLEN
            emmc_send_cmd(16, SECTOR_SIZE);
            emmc_recv_r1();
            
            emmc_initialized = true;
            return true;
        }
        nrf_delay_ms(1);
    }
    return false;
}

/* ── Packet processing ── */
static uint8_t rx_buf[MAX_PACKET_SIZE];
static size_t rx_len = 0;
static uint8_t tx_buf[MAX_PACKET_SIZE * 2]; // COBS can expand
static uint8_t sector_buf[SECTOR_SIZE];

static void send_response(uint8_t seq, uint8_t cmd, const uint8_t *payload, size_t plen) {
    uint8_t raw[MAX_PACKET_SIZE];
    raw[0] = PROTOCOL_MAGIC;
    raw[1] = seq;
    raw[2] = cmd;
    raw[3] = (uint8_t)plen;
    if (plen > 0) memcpy(&raw[4], payload, plen);
    raw[4 + plen] = crc8(raw, 4 + plen);
    
    size_t encoded_len = cobs_encode(raw, 5 + plen, tx_buf);
    
    // Send in CDC chunks
    size_t sent = 0;
    while (sent < encoded_len) {
        size_t chunk = encoded_len - sent;
        if (chunk > 64) chunk = 64;
        app_usbd_cdc_acm_write(&m_app_cdc_acm, &tx_buf[sent], chunk);
        sent += chunk;
        // Small delay for USB to process
        nrf_delay_us(100);
    }
}

static void process_packet(const uint8_t *decoded, size_t len) {
    if (len < 5) return;
    if (decoded[0] != PROTOCOL_MAGIC) return;
    
    uint8_t seq = decoded[1];
    uint8_t cmd = decoded[2];
    uint8_t plen = decoded[3];
    
    if (len < (size_t)(5 + plen)) return;
    
    // Verify CRC
    uint8_t expected_crc = crc8(decoded, 4 + plen);
    if (decoded[4 + plen] != expected_crc) return;
    
    const uint8_t *payload = &decoded[4];
    
    switch (cmd) {
        case 0x52: { // 'R' Status
            uint8_t resp[8];
            // Return: version(4) + emmc_status(4)
            resp[0] = 0x01; resp[1] = 0x00; resp[2] = 0x00; resp[3] = 0x00; // v1.0
            bool emmc_ok = emmc_init();
            resp[4] = emmc_ok ? 1 : 0;
            resp[5] = 0; resp[6] = 0; resp[7] = 0;
            send_response(seq, 0x53, resp, 8);
            break;
        }
        
        case 0x53: { // 'S' Read sector
            if (plen < 4) { send_response(seq, 0x54, NULL, 0); break; }
            uint32_t sector = payload[0] | (payload[1] << 8) | (payload[2] << 16) | (payload[3] << 24);
            
            if (!emmc_init()) {
                uint8_t err = 0xFF;
                send_response(seq, 0x54, &err, 1);
                break;
            }
            
            if (emmc_read_block(sector, sector_buf, SECTOR_SIZE)) {
                // Send sector data in chunks
                for (size_t offset = 0; offset < SECTOR_SIZE; offset += CHUNK_SIZE) {
                    size_t chunk_len = CHUNK_SIZE;
                    if (offset + chunk_len > SECTOR_SIZE) chunk_len = SECTOR_SIZE - offset;
                    
                    uint8_t chunk_resp[CHUNK_SIZE + 4];
                    chunk_resp[0] = (offset >> 0) & 0xFF;
                    chunk_resp[1] = (offset >> 8) & 0xFF;
                    chunk_resp[2] = (chunk_len >> 0) & 0xFF;
                    chunk_resp[3] = (chunk_len >> 8) & 0xFF;
                    memcpy(&chunk_resp[4], &sector_buf[offset], chunk_len);
                    send_response(seq, 0x54, chunk_resp, chunk_len + 4);
                }
            } else {
                uint8_t err = 0xFE;
                send_response(seq, 0x54, &err, 1);
            }
            break;
        }
        
        case 0x57: { // 'W' Write sector
            if (plen < 4) { send_response(seq, 0x58, NULL, 0); break; }
            uint32_t sector = payload[0] | (payload[1] << 8) | (payload[2] << 16) | (payload[3] << 24);
            size_t data_len = plen - 4;
            
            // For large writes, accumulate chunks
            static uint32_t write_sector = 0xFFFFFFFF;
            static size_t write_offset = 0;
            
            if (sector != write_sector) {
                write_sector = sector;
                write_offset = 0;
                memset(sector_buf, 0xFF, SECTOR_SIZE);
            }
            
            if (data_len > 0 && write_offset + data_len <= SECTOR_SIZE) {
                memcpy(&sector_buf[write_offset], &payload[4], data_len);
                write_offset += data_len;
            }
            
            uint8_t resp[1] = { 0x00 }; // ACK
            
            if (write_offset >= SECTOR_SIZE || data_len == 0) {
                // Flush to eMMC
                if (!emmc_init() || !emmc_write_block(write_sector, sector_buf, SECTOR_SIZE)) {
                    resp[0] = 0xFF; // Error
                }
                write_sector = 0xFFFFFFFF;
                write_offset = 0;
            }
            
            send_response(seq, 0x58, resp, 1);
            break;
        }
        
        case 0x49: { // 'I' Info
            uint8_t info[16];
            memset(info, 0, sizeof(info));
            // Device ID string "SP1" + version
            info[0] = 'S'; info[1] = 'P'; info[2] = '1'; info[3] = 0;
            // eMMC size: 4GB = ~488281 sectors at 8192 bytes
            uint32_t total_sectors = 488281;
            info[4] = (total_sectors >> 0) & 0xFF;
            info[5] = (total_sectors >> 8) & 0xFF;
            info[6] = (total_sectors >> 16) & 0xFF;
            info[7] = (total_sectors >> 24) & 0xFF;
            // Sector size
            info[8] = (SECTOR_SIZE >> 0) & 0xFF;
            info[9] = (SECTOR_SIZE >> 8) & 0xFF;
            send_response(seq, 0x4A, info, 16);
            break;
        }
        
        default:
            send_response(seq, cmd + 1, NULL, 0);
            break;
    }
}

/* ── CDC ACM event handler ── */
static uint8_t cdc_rx_byte;
static uint8_t cobs_rx_buf[MAX_PACKET_SIZE];
static size_t cobs_rx_len = 0;

static void cdc_acm_user_ev_handler(app_usbd_class_inst_t const * p_inst,
                                     app_usbd_cdc_acm_user_event_t event) {
    switch (event) {
        case APP_USBD_CDC_ACM_USER_EVT_PORT_OPEN:
            // Start receiving
            app_usbd_cdc_acm_read(&m_app_cdc_acm, &cdc_rx_byte, 1);
            break;
            
        case APP_USBD_CDC_ACM_USER_EVT_PORT_CLOSE:
            break;
            
        case APP_USBD_CDC_ACM_USER_EVT_RX_DONE: {
            // Accumulate COBS frame (terminated by 0x00)
            if (cdc_rx_byte == 0x00) {
                if (cobs_rx_len > 0) {
                    // Decode COBS
                    uint8_t decoded[MAX_PACKET_SIZE];
                    cobs_rx_buf[cobs_rx_len] = 0x00; // append terminator
                    size_t dec_len = cobs_decode(cobs_rx_buf, cobs_rx_len + 1, decoded);
                    process_packet(decoded, dec_len);
                    cobs_rx_len = 0;
                }
            } else {
                if (cobs_rx_len < sizeof(cobs_rx_buf) - 1) {
                    cobs_rx_buf[cobs_rx_len++] = cdc_rx_byte;
                }
            }
            // Continue receiving
            app_usbd_cdc_acm_read(&m_app_cdc_acm, &cdc_rx_byte, 1);
            break;
        }
        
        case APP_USBD_CDC_ACM_USER_EVT_TX_DONE:
            break;
            
        default:
            break;
    }
}

/* ── USB event handler ── */
static void usbd_user_ev_handler(app_usbd_event_type_t event) {
    switch (event) {
        case APP_USBD_EVT_DRV_SUSPEND:
        case APP_USBD_EVT_DRV_RESUME:
        case APP_USBD_EVT_STARTED:
        case APP_USBD_EVT_STOPPED:
            break;
        case APP_USBD_EVT_POWER_DETECTED:
            if (!nrf_drv_usbd_is_started()) {
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

/* ── Main ── */
int main(void) {
    ret_code_t ret;
    
    // Initialize clocks
    ret = nrf_drv_clock_init();
    APP_ERROR_CHECK(ret);
    nrf_drv_clock_lfclk_request(NULL);
    while (!nrf_drv_clock_lfclk_is_running()) { /* wait */ }
    
    // LED indicator
    nrf_gpio_cfg_output(LED_PIN);
    nrf_gpio_pin_set(LED_PIN);
    
    // Initialize power
    ret = nrf_drv_power_init(NULL);
    APP_ERROR_CHECK(ret);
    
    // Initialize USB
    static const app_usbd_config_t usbd_config = {
        .ev_state_proc = usbd_user_ev_handler
    };
    ret = app_usbd_init(&usbd_config);
    APP_ERROR_CHECK(ret);
    
    // Add CDC ACM class
    app_usbd_class_inst_t const * class_cdc_acm = app_usbd_cdc_acm_class_inst_get(&m_app_cdc_acm);
    ret = app_usbd_class_append(class_cdc_acm);
    APP_ERROR_CHECK(ret);
    
    // Enable USB
    app_usbd_enable();
    app_usbd_start();
    
    // Main loop
    while (true) {
        while (app_usbd_event_queue_process()) {
            /* process USB events */
        }
        __WFE();
    }
}
