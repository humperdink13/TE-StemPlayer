#!/usr/bin/env python3
"""
Probe the SP-1 bootloader to discover available commands,
including potential firmware read/dump capabilities.

Usage:
  1. Hold Track 1 + Track 4 buttons while connecting USB-C
  2. Run: python3 probe_bootloader.py
"""

import serial
import serial.tools.list_ports
import struct
import sys
import time
import os

# Constants from solderless.engineering
VID = 0x2367
PID = 0x1701
BAUD = 115200
FLASH_START = 0x20000
FLASH_END = 0xFF000
PAGE_SIZE = 4096
CHUNK_SIZE = 240

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
    """COBS encode with 0x00 terminator"""
    out = bytearray()
    idx = 0
    while idx < len(data):
        # Find next zero or end
        block_start = idx
        while idx < len(data) and data[idx] != 0 and (idx - block_start) < 254:
            idx += 1
        code = idx - block_start + 1
        out.append(code)
        out.extend(data[block_start:idx])
        if idx < len(data) and data[idx] == 0:
            idx += 1
    out.append(0x00)  # terminator
    return bytes(out)

def cobs_decode(data):
    """COBS decode (strip trailing 0x00 first)"""
    if data and data[-1] == 0:
        data = data[:-1]
    out = bytearray()
    idx = 0
    while idx < len(data):
        code = data[idx]
        idx += 1
        for _ in range(code - 1):
            if idx < len(data):
                out.append(data[idx])
                idx += 1
        if code < 0xFF and idx < len(data):
            out.append(0)
    return bytes(out)

def build_packet(cmd, payload=b'', seq=1):
    body = bytes([0x51, seq & 0xFF, cmd, len(payload)]) + payload
    c = crc8(body)
    return cobs_encode(body + bytes([c]))

def parse_response(data):
    if not data or len(data) < 3:
        return None
    decoded = cobs_decode(data)
    if len(decoded) < 5:
        return None
    _, seq, cmd, plen = decoded[0], decoded[1], decoded[2], decoded[3]
    if len(decoded) < 5 + plen:
        return None
    payload = decoded[4:4+plen]
    got_crc = decoded[4+plen]
    exp_crc = crc8(decoded[:4+plen])
    return {'cmd': cmd, 'seq': seq, 'payload': payload, 'crc_ok': got_crc == exp_crc}

def find_device():
    """Find the stem player serial port"""
    ports = serial.tools.list_ports.comports()
    for p in ports:
        if p.vid == VID and p.pid == PID:
            return p.device
        # Also check by name
        if 'stem' in (p.description or '').lower():
            return p.device
    # List all ports for debugging
    print("Available serial ports:")
    for p in ports:
        print(f"  {p.device}: {p.description} (VID:{p.vid:#06x if p.vid else 'N/A'} PID:{p.pid:#06x if p.pid else 'N/A'})")
    return None

def read_until_zero(ser, timeout_ms=3000):
    """Read from serial until 0x00 byte (COBS terminator)"""
    buf = bytearray()
    deadline = time.time() + timeout_ms / 1000
    while time.time() < deadline:
        if ser.in_waiting > 0:
            data = ser.read(ser.in_waiting)
            buf.extend(data)
            zi = buf.find(0x00)
            if zi >= 0:
                return bytes(buf[:zi+1])
        else:
            time.sleep(0.01)
    return bytes(buf) if buf else None

def send_cmd(ser, cmd, payload=b'', seq=1, timeout=3000):
    """Send command and wait for response"""
    # Drain input
    while ser.in_waiting > 0:
        ser.read(ser.in_waiting)
    
    pkt = build_packet(cmd, payload, seq)
    ser.write(pkt)
    ser.flush()
    
    raw = read_until_zero(ser, timeout)
    if raw:
        return parse_response(raw)
    return None

def le32(v):
    return struct.pack('<I', v)

def read_le32(data, offset=0):
    return struct.unpack_from('<I', data, offset)[0]

def probe_commands(ser):
    """Probe all possible command bytes to discover bootloader capabilities"""
    print("\n=== PROBING BOOTLOADER COMMANDS ===\n")
    
    # Known commands from solderless.engineering:
    # 0x45 (E) = Write chunk (no reply)
    # 0x46 (F) = Format
    # 0x48 (H) = Commit  
    # 0x52 (R) = Status
    
    # Try all printable ASCII and common command bytes
    responsive_cmds = []
    seq = 1
    
    for cmd_byte in range(0x20, 0x80):
        char = chr(cmd_byte) if 32 <= cmd_byte < 127 else f'0x{cmd_byte:02x}'
        
        resp = send_cmd(ser, cmd_byte, b'', seq, timeout=500)
        seq += 1
        
        if resp is not None:
            status = "CRC OK" if resp['crc_ok'] else "CRC BAD"
            payload_hex = resp['payload'].hex() if resp['payload'] else '(empty)'
            print(f"  CMD 0x{cmd_byte:02X} ({char}) -> Response 0x{resp['cmd']:02X} ({chr(resp['cmd']) if 32 <= resp['cmd'] < 127 else '?'}) [{status}] payload: {payload_hex}")
            responsive_cmds.append((cmd_byte, resp))
        
        time.sleep(0.05)
    
    print(f"\n=== {len(responsive_cmds)} commands responded ===\n")
    return responsive_cmds

def try_read_flash(ser, address, length=256):
    """Try various potential read commands with address payload"""
    print(f"\n=== TRYING TO READ FLASH AT 0x{address:08X} ===\n")
    
    seq = 100
    
    # Try 'R' (0x52) with address payload
    for cmd_name, cmd_byte in [('R', 0x52), ('D', 0x44), ('r', 0x72), ('d', 0x64), 
                                 ('G', 0x47), ('L', 0x4C), ('M', 0x4D), ('P', 0x50),
                                 ('Q', 0x51), ('T', 0x54), ('V', 0x56), ('X', 0x58)]:
        # Try with 4-byte address
        resp = send_cmd(ser, cmd_byte, le32(address), seq, timeout=1000)
        seq += 1
        if resp is not None and resp['crc_ok']:
            payload_hex = resp['payload'].hex() if resp['payload'] else '(empty)'
            print(f"  CMD '{cmd_name}' (0x{cmd_byte:02X}) + addr -> 0x{resp['cmd']:02X} payload[{len(resp['payload'])}]: {payload_hex[:64]}...")
        
        # Try with 4-byte address + 4-byte length
        resp = send_cmd(ser, cmd_byte, le32(address) + le32(length), seq, timeout=1000)
        seq += 1
        if resp is not None and resp['crc_ok']:
            payload_hex = resp['payload'].hex() if resp['payload'] else '(empty)'
            print(f"  CMD '{cmd_name}' (0x{cmd_byte:02X}) + addr+len -> 0x{resp['cmd']:02X} payload[{len(resp['payload'])}]: {payload_hex[:64]}...")
        
        time.sleep(0.05)

def dump_firmware(ser, output_path):
    """If we find a read command, dump the entire firmware"""
    print(f"\n=== ATTEMPTING FIRMWARE DUMP ===\n")
    print(f"Flash range: 0x{FLASH_START:08X} - 0x{FLASH_END:08X}")
    print(f"Total size: {(FLASH_END - FLASH_START) // 1024} KB")
    
    # First check if any read method works
    # Try reading the first page with various methods
    
    # Method 1: Try Nordic DFU read object command
    # Nordic Secure DFU protocol uses different opcodes
    # Op 0x06 = Select Object, 0x01 = Create Object, 0x03 = Set Receipt Notification
    # Op 0x07 = Get CRC, 0x08 = Execute
    
    # Try sending raw bytes that might trigger a memory read
    seq = 200
    
    # Try a simple approach: see if the bootloader echoes back memory
    for test_addr in [0x00000000, FLASH_START, 0x10001000]:  # Reset vector, app start, UICR
        resp = send_cmd(ser, 0x52, le32(test_addr), seq, timeout=1000)
        seq += 1
        if resp and resp['crc_ok'] and len(resp['payload']) > 4:
            print(f"  Status+addr 0x{test_addr:08X}: payload[{len(resp['payload'])}] = {resp['payload'].hex()[:64]}")

def main():
    print("=" * 60)
    print("SP-1 Bootloader Probe Tool")
    print("=" * 60)
    print()
    print("Make sure device is in BOOTLOADER MODE:")
    print("  Hold Track 1 + Track 4 while connecting USB-C")
    print()
    
    port = find_device()
    if not port:
        print("ERROR: No stem player found on any serial port.")
        print("\nIs the device in bootloader mode?")
        print("  1. Disconnect USB")
        print("  2. Hold Track 1 + Track 4 buttons")
        print("  3. While holding, connect USB-C")
        print("  4. Release buttons after 2 seconds")
        print("  5. Run this script again")
        sys.exit(1)
    
    print(f"Found device on: {port}")
    
    try:
        ser = serial.Serial(port, BAUD, timeout=1)
        print(f"Opened serial port at {BAUD} baud\n")
    except Exception as e:
        print(f"ERROR: Cannot open {port}: {e}")
        sys.exit(1)
    
    time.sleep(0.5)  # Let connection settle
    
    # Phase 1: Verify bootloader connection with known Status command
    print("--- Phase 1: Verify bootloader connection ---")
    resp = send_cmd(ser, 0x52, b'', 1, timeout=3000)
    if resp and resp['crc_ok'] and resp['cmd'] == 0x53:
        pages = read_le32(resp['payload']) if len(resp['payload']) >= 4 else 0
        print(f"  Bootloader responded! Pages: {pages}")
        print(f"  Full response payload: {resp['payload'].hex()}")
    else:
        print(f"  WARNING: Unexpected response: {resp}")
        print("  Device may not be in bootloader mode.")
    
    # Phase 2: Probe all commands
    print("\n--- Phase 2: Probe all command bytes ---")
    responsive = probe_commands(ser)
    
    # Phase 3: Try to read flash with responsive commands
    print("\n--- Phase 3: Try flash read methods ---")
    try_read_flash(ser, FLASH_START)
    try_read_flash(ser, 0x00000000)  # Try reset vector area
    
    # Phase 4: Try firmware dump if possible
    print("\n--- Phase 4: Firmware dump attempt ---")
    dump_firmware(ser, '/Users/jon/TE-StemPlayer/backups/firmware_dump.bin')
    
    # Also try just reading raw bytes after commands
    print("\n--- Phase 5: Raw serial probe ---")
    # Send some raw commands that Nordic bootloaders might understand
    for raw_cmd in [b'\x01', b'\x06', b'\x07', b'\x09', b'\x0a', b'\x0b']:
        ser.write(raw_cmd)
        ser.flush()
        time.sleep(0.3)
        if ser.in_waiting > 0:
            data = ser.read(ser.in_waiting)
            print(f"  Raw 0x{raw_cmd[0]:02X} -> {data.hex()}")
    
    ser.close()
    print("\n=== PROBE COMPLETE ===")
    print("\nResults saved. Review the output above to determine if flash reading is possible.")

if __name__ == '__main__':
    main()
