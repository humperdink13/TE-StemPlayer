#!/usr/bin/env python3
"""
Stem Player Firmware Flasher
Uses the same serial protocol as solderless.engineering
Requires device to be in bootloader mode (hold function button while plugging in)
"""

import serial
import serial.tools.list_ports
import struct
import sys
import time

PAGE_SIZE = 4096
CHUNK_SIZE = 240
FLASH_START = 0x20000
FLASH_END = 0xFF000
FIRST_PAGE = 0x20
LAST_PAGE = 0xFE

CRC8_TABLE = bytes([
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
])

def crc8(data):
    crc = 0
    for b in data:
        crc = CRC8_TABLE[crc ^ b]
    return crc

def cobs_encode(data):
    out = bytearray()
    idx = 0
    while idx <= len(data):
        end = idx
        while end < len(data) and data[end] != 0:
            end += 1
        out.append(end - idx + 1)
        out.extend(data[idx:end])
        if end < len(data):
            idx = end + 1
        else:
            break
    out.append(0x00)
    return bytes(out)

def cobs_decode(raw):
    data = raw
    if len(data) > 0 and data[-1] == 0:
        data = data[:-1]
    out = bytearray()
    idx = 0
    while idx < len(data):
        code = data[idx]; idx += 1
        if code == 0: break
        for _ in range(code - 1):
            if idx < len(data):
                out.append(data[idx]); idx += 1
        if code < 0xFF and idx < len(data):
            out.append(0)
    return bytes(out)

def build_packet(cmd, payload=b'', seq=1):
    body = bytes([0x51, seq & 0xFF, cmd, len(payload)]) + payload
    c = crc8(body)
    return cobs_encode(body + bytes([c]))

def parse_response(data):
    if not data or len(data) < 3: return None
    decoded = cobs_decode(data)
    if len(decoded) < 5: return None
    _, seq, cmd, plen = decoded[0], decoded[1], decoded[2], decoded[3]
    if len(decoded) < 5 + plen: return None
    payload = decoded[4:4+plen]
    got_crc = decoded[4+plen]
    exp_crc = crc8(decoded[:4+plen])
    return {'cmd': cmd, 'seq': seq, 'payload': payload, 'crc_ok': got_crc == exp_crc}

def le32(v):
    return struct.pack('<I', v)

def read_le32(b, o=0):
    return struct.unpack_from('<I', b, o)[0]

def find_serial_port():
    """Find stem player serial port"""
    ports = serial.tools.list_ports.comports()
    for p in ports:
        if '2367' in (p.vid and hex(p.vid) or '') or 'stem' in (p.description or '').lower():
            return p.device
    # Return first non-system port
    for p in ports:
        if 'Bluetooth' not in p.device and 'debug' not in p.device and 'wlan' not in p.device:
            return p.device
    return None

def send_cmd(ser, cmd, payload=b'', seq=1, timeout=3.0):
    ser.reset_input_buffer()
    pkt = build_packet(cmd, payload, seq)
    ser.write(pkt)
    
    # Read until 0x00 terminator
    deadline = time.time() + timeout
    buf = bytearray()
    while time.time() < deadline:
        if ser.in_waiting > 0:
            b = ser.read(ser.in_waiting)
            buf.extend(b)
            if 0x00 in buf:
                idx = buf.index(0x00)
                return parse_response(bytes(buf[:idx+1]))
        else:
            time.sleep(0.01)
    return None

def flash_firmware(port_path, fw_path):
    with open(fw_path, 'rb') as f:
        fw = f.read()
    
    max_size = FLASH_END - FLASH_START
    if len(fw) == 0 or len(fw) > max_size:
        print(f"Error: firmware size {len(fw)} invalid (max {max_size})")
        return False
    
    num_pages = (len(fw) + PAGE_SIZE - 1) // PAGE_SIZE
    print(f"Firmware: {len(fw)} bytes, {num_pages} pages")
    
    ser = serial.Serial(port_path, 115200, timeout=1)
    seq = 1
    
    # Phase 0: Status
    print("Checking device...")
    r = send_cmd(ser, 0x52, seq=seq); seq += 1
    if not r or not r['crc_ok'] or r['cmd'] != 0x53:
        print("Device not responding. Is it in bootloader mode?")
        ser.close()
        return False
    if len(r['payload']) >= 4:
        print(f"  Device pages: {read_le32(r['payload'], 0)}")
    
    # Phase 1: Format
    print("Formatting...")
    r = send_cmd(ser, 0x46, seq=seq, timeout=5); seq += 1
    if not r or not r['crc_ok'] or r['cmd'] != 0x47:
        print("Format failed!")
        ser.close()
        return False
    print("  Format OK")
    
    # Phase 2: Write chunks
    print(f"Writing {len(fw)} bytes...")
    e_seq = 0
    for pg_idx in range(num_pages):
        page_base = pg_idx * PAGE_SIZE
        page_data_len = min(PAGE_SIZE, len(fw) - page_base)
        
        for offset in range(0, page_data_len, CHUNK_SIZE):
            e_seq += 1
            chunk_len = min(CHUNK_SIZE, page_data_len - offset)
            addr = FLASH_START + page_base + offset
            data = fw[page_base + offset:page_base + offset + chunk_len]
            
            payload = le32(e_seq) + le32(addr) + data
            pkt = build_packet(0x45, payload, seq); seq += 1
            ser.write(pkt)
            
            if offset == 0 and pg_idx > 0:
                time.sleep(0.1)
            else:
                time.sleep(0.005)
        
        pct = (pg_idx + 1) * 100 // num_pages
        print(f"\r  Page {pg_idx+1}/{num_pages} ({pct}%)", end='', flush=True)
    
    print(f"\n  Sent {e_seq} chunks")
    
    # Phase 3: Commit
    print("Committing...")
    time.sleep(0.15)
    last_page = FIRST_PAGE + num_pages - 1
    r = send_cmd(ser, 0x48, le32(last_page), seq=seq, timeout=5); seq += 1
    if not r or not r['crc_ok'] or r['cmd'] != 0x49:
        print("Commit failed!")
        ser.close()
        return False
    
    counter_val = e_seq
    if len(r['payload']) >= 4:
        counter_val = read_le32(r['payload'], 0)
        print(f"  Counter: {counter_val}")
    
    # Phase 4: Finalize
    print("Finalizing...")
    r = send_cmd(ser, 0x48, le32(counter_val), seq=seq, timeout=5); seq += 1
    if not r or not r['crc_ok']:
        print("Finalize failed!")
        ser.close()
        return False
    
    if r['cmd'] == 0x49:
        print("Firmware update COMPLETE!")
        print("Press the function button to restart your device.")
        ser.close()
        return True
    else:
        print(f"Unexpected response: {hex(r['cmd'])}")
        ser.close()
        return False

if __name__ == '__main__':
    if len(sys.argv) < 2:
        fw_path = '/private/tmp/TE-StemPlayer/firmware/armgcc/_build/nrf52840_xxaa.bin'
    else:
        fw_path = sys.argv[1]
    
    port_path = sys.argv[2] if len(sys.argv) > 2 else find_serial_port()
    
    if not port_path:
        print("No serial port found. Is the device in bootloader mode?")
        print("To enter bootloader mode: hold the function button while plugging in USB")
        print("\nAvailable ports:")
        for p in serial.tools.list_ports.comports():
            print(f"  {p.device}: {p.description}")
        sys.exit(1)
    
    print(f"Using port: {port_path}")
    print(f"Firmware: {fw_path}")
    
    if flash_firmware(port_path, fw_path):
        sys.exit(0)
    else:
        sys.exit(1)
