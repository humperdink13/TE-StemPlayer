#!/usr/bin/env python3
"""
Flash firmware to SP-1 Stem Player via bootloader serial protocol.

The bootloader uses COBS-encoded packets with CRC8 over serial at 115200 baud.
Protocol (from solderless.engineering research):
  - Packets are COBS encoded with 0x00 delimiter
  - Each packet: [cmd_byte] [payload...] [crc8]
  - Commands: 'F' = flash start, 'W' = write chunk, 'E' = execute/boot
  - Flash writes in 240-byte chunks
  - CRC8 polynomial: x^8 + x^2 + x + 1 (0x07)

Usage: python3 flash_firmware.py <firmware.bin> [serial_port]
"""

import sys
import time
import struct
import glob
import serial


def crc8(data):
    """CRC8 with polynomial 0x07."""
    crc = 0
    for byte in data:
        crc ^= byte
        for _ in range(8):
            if crc & 0x80:
                crc = (crc << 1) ^ 0x07
            else:
                crc = crc << 1
            crc &= 0xFF
    return crc


def cobs_encode(data):
    """COBS encode data."""
    output = bytearray()
    idx = 0
    while idx < len(data):
        # Find next zero byte
        next_zero = idx
        while next_zero < len(data) and data[next_zero] != 0 and (next_zero - idx) < 254:
            next_zero += 1
        
        # Write overhead byte (distance to next zero + 1)
        output.append(next_zero - idx + 1)
        # Write data bytes
        output.extend(data[idx:next_zero])
        
        if next_zero < len(data) and data[next_zero] == 0:
            next_zero += 1  # Skip the zero
        idx = next_zero
    
    return bytes(output)


def make_packet(cmd, payload=b''):
    """Build a COBS-encoded packet with CRC8."""
    raw = bytes([cmd]) + payload
    raw += bytes([crc8(raw)])
    encoded = cobs_encode(raw)
    return encoded + b'\x00'  # Delimiter


def find_serial_port():
    """Find the SP-1 bootloader serial port."""
    patterns = [
        '/dev/cu.usbmodem*',
        '/dev/ttyACM*',
        '/dev/ttyUSB*',
    ]
    for pattern in patterns:
        ports = glob.glob(pattern)
        if ports:
            return ports[0]
    return None


def flash_firmware(bin_path, port=None):
    """Flash a binary firmware file to the SP-1 via bootloader."""
    
    with open(bin_path, 'rb') as f:
        firmware = f.read()
    
    print(f"Firmware size: {len(firmware)} bytes")
    
    if port is None:
        port = find_serial_port()
        if port is None:
            print("ERROR: No serial port found. Is the device connected?")
            sys.exit(1)
    
    print(f"Using serial port: {port}")
    
    ser = serial.Serial(port, 115200, timeout=2)
    time.sleep(0.5)
    
    # Drain any pending data
    ser.reset_input_buffer()
    
    # Step 1: Send 'F' command (flash start) with firmware size
    print("Sending flash start command...")
    size_payload = struct.pack('<I', len(firmware))
    pkt = make_packet(ord('F'), size_payload)
    ser.write(pkt)
    time.sleep(0.2)
    
    resp = ser.read(100)
    print(f"  Response: {resp.hex() if resp else 'none'}")
    
    # Step 2: Send firmware in 240-byte chunks with 'W' command
    chunk_size = 240
    total_chunks = (len(firmware) + chunk_size - 1) // chunk_size
    
    print(f"Sending {total_chunks} chunks...")
    for i in range(total_chunks):
        offset = i * chunk_size
        chunk = firmware[offset:offset + chunk_size]
        
        # Payload: 4-byte offset + chunk data
        payload = struct.pack('<I', offset) + chunk
        pkt = make_packet(ord('W'), payload)
        ser.write(pkt)
        
        # Wait for ack
        time.sleep(0.01)
        resp = ser.read(100)
        
        progress = (i + 1) * 100 // total_chunks
        print(f"\r  Progress: {progress}% ({i+1}/{total_chunks})", end='', flush=True)
    
    print()
    
    # Step 3: Send 'E' command (execute/boot)
    print("Sending execute command...")
    pkt = make_packet(ord('E'))
    ser.write(pkt)
    time.sleep(0.5)
    
    resp = ser.read(100)
    print(f"  Response: {resp.hex() if resp else 'none'}")
    
    ser.close()
    print("Flash complete! Device should reboot with new firmware.")


if __name__ == '__main__':
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <firmware.bin> [serial_port]")
        sys.exit(1)
    
    bin_file = sys.argv[1]
    serial_port = sys.argv[2] if len(sys.argv) > 2 else None
    
    flash_firmware(bin_file, serial_port)
