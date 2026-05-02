
#!/usr/bin/env python3
"""
Stem Player Firmware Flasher - libusb version
Uses direct USB bulk transfers instead of serial driver (which fails on macOS)
Requires sudo for kernel driver detach
"""
import usb.core, usb.util, time, struct, sys

PAGE_SIZE = 4096
CHUNK_SIZE = 240
FLASH_START = 0x20000
FLASH_END = 0xFF000
FIRST_PAGE = 0x20

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
    for b in data: crc = CRC8_TABLE[crc ^ b]
    return crc

def cobs_encode(data):
    out = bytearray(); idx = 0
    while idx <= len(data):
        end = idx
        while end < len(data) and data[end] != 0: end += 1
        out.append(end - idx + 1); out.extend(data[idx:end])
        if end < len(data): idx = end + 1
        else: break
    out.append(0x00); return bytes(out)

def cobs_decode(raw):
    data = raw
    if len(data) > 0 and data[-1] == 0: data = data[:-1]
    out = bytearray(); idx = 0
    while idx < len(data):
        code = data[idx]; idx += 1
        if code == 0: break
        for _ in range(code - 1):
            if idx < len(data): out.append(data[idx]); idx += 1
        if code < 0xFF and idx < len(data): out.append(0)
    return bytes(out)

def build_packet(cmd, payload=b'', seq=1):
    body = bytes([0x51, seq & 0xFF, cmd, len(payload)]) + payload
    return cobs_encode(body + bytes([crc8(body)]))

def le32(v): return struct.pack('<I', v)

def parse_response(decoded):
    if len(decoded) < 5: return None
    return {'cmd': decoded[2], 'seq': decoded[1], 'plen': decoded[3],
            'payload': decoded[4:4+decoded[3]],
            'crc_ok': decoded[4+decoded[3]] == crc8(decoded[:4+decoded[3]]) if len(decoded) > 4+decoded[3] else False}

class USBFlasher:
    def __init__(self):
        self.dev = None
        self.EP_OUT = 0x01
        self.EP_IN = 0x81

    def connect(self):
        self.dev = usb.core.find(idVendor=0x2367, idProduct=0x1701)
        if not self.dev:
            print("Device not found!")
            return False
        print(f"Found: {self.dev.product}")
        # Detach kernel drivers
        for i in range(2):
            try:
                if self.dev.is_kernel_driver_active(i):
                    self.dev.detach_kernel_driver(i)
            except: pass
        self.dev.set_configuration()
        for i in range(2):
            try: usb.util.claim_interface(self.dev, i)
            except: pass
        # Set CDC line coding and control
        lc = struct.pack('<IBBB', 115200, 0, 0, 8)
        self.dev.ctrl_transfer(0x21, 0x20, 0, 0, lc)
        self.dev.ctrl_transfer(0x21, 0x22, 0x03, 0)
        time.sleep(0.2)
        return True

    def drain(self):
        """Drain any pending data from the IN endpoint"""
        drained = 0
        while True:
            try:
                r = self.dev.read(self.EP_IN, 64, timeout=100)
                drained += len(r)
            except:
                break
        if drained > 0:
            print(f"  Drained {drained} stale bytes")

    def send_cmd(self, cmd, payload=b'', seq=1, timeout=5.0):
        """Send command and wait for response with proper framing"""
        pkt = build_packet(cmd, payload, seq)
        self.dev.write(self.EP_OUT, pkt, timeout=1000)
        buf = bytearray()
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                r = self.dev.read(self.EP_IN, 64, timeout=500)
                buf.extend(bytes(r))
                if 0x00 in buf:
                    # Find the complete packet (ends with 0x00)
                    idx = buf.index(0x00)
                    decoded = cobs_decode(bytes(buf[:idx+1]))
                    return parse_response(decoded)
            except:
                time.sleep(0.01)
        return None

    def flash(self, fw_path):
        with open(fw_path, 'rb') as f:
            fw = f.read()

        max_size = FLASH_END - FLASH_START
        if len(fw) == 0 or len(fw) > max_size:
            print(f"Error: firmware size {len(fw)} invalid (max {max_size})")
            return False

        # Validate vector table
        sp = struct.unpack_from('<I', fw, 0)[0]
        reset = struct.unpack_from('<I', fw, 4)[0]
        if (sp & 0x2FFE0000) != 0x20000000:
            print(f"Error: Invalid SP 0x{sp:08X} - bootloader will reject")
            return False
        if reset < FLASH_START or reset > FLASH_END:
            print(f"Warning: Reset handler 0x{reset:08X} outside flash range")

        num_pages = (len(fw) + PAGE_SIZE - 1) // PAGE_SIZE
        print(f"Firmware: {len(fw)} bytes, {num_pages} pages")
        print(f"  SP: 0x{sp:08X}, Reset: 0x{reset:08X}")

        seq = 1

        # Phase 0: Status
        print("Checking device...")
        self.drain()
        r = self.send_cmd(0x52, seq=seq); seq += 1
        if not r or not r['crc_ok'] or r['cmd'] != 0x53:
            print(f"Device not responding properly: {r}")
            return False
        print(f"  Status OK (cmd=0x{r['cmd']:02X})")

        # Phase 1: Format
        print("Formatting...")
        self.drain()
        r = self.send_cmd(0x46, seq=seq, timeout=5); seq += 1
        if not r or not r['crc_ok'] or r['cmd'] != 0x47:
            print(f"Format failed: {r}")
            return False
        print("  Format OK")

        # Phase 2: Write chunks (fire-and-forget like the website does)
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
                self.dev.write(self.EP_OUT, pkt, timeout=1000)
                # Timing: match the website's behavior
                if offset == 0 and pg_idx > 0:
                    time.sleep(0.1)  # Page boundary pause
                else:
                    time.sleep(0.005)
            pct = (pg_idx + 1) * 100 // num_pages
            print(f"  Page {pg_idx+1}/{num_pages} ({pct}%)")
        print(f"  Sent {e_seq} chunks")

        # CRITICAL: Drain all stale responses before commit
        print("Draining write responses...")
        time.sleep(0.5)  # Wait for device to process last chunks
        self.drain()

        # Phase 3: Commit
        print("Committing...")
        time.sleep(0.15)
        last_page = FIRST_PAGE + num_pages - 1
        r = self.send_cmd(0x48, le32(last_page), seq=seq, timeout=5); seq += 1
        if not r:
            print("Commit: no response!")
            return False
        print(f"  Commit response: cmd=0x{r['cmd']:02X}, crc_ok={r['crc_ok']}, payload={r['payload'].hex() if r['payload'] else 'none'}")
        if r['cmd'] != 0x49:
            print(f"  ERROR: Expected cmd=0x49 (I), got cmd=0x{r['cmd']:02X}")
            print("  Commit FAILED - bootloader did not accept firmware")
            return False
        if not r['crc_ok']:
            print("  ERROR: CRC check failed on commit response")
            return False

        counter_val = e_seq
        if len(r['payload']) >= 4:
            counter_val = struct.unpack_from('<I', r['payload'], 0)[0]
            print(f"  Counter: {counter_val}")

        # Phase 4: Finalize
        print("Finalizing...")
        self.drain()
        r = self.send_cmd(0x48, le32(counter_val), seq=seq, timeout=5); seq += 1
        if not r or not r['crc_ok']:
            print(f"Finalize failed: {r}")
            return False

        if r['cmd'] == 0x49:
            if len(r['payload']) >= 4:
                print(f"  Finalize counter: {struct.unpack_from('<I', r['payload'], 0)[0]}")
            print("\n=== FIRMWARE UPDATE COMPLETE! ===")
            print("Unplug and replug your Stem Player to boot the new firmware.")
            return True
        else:
            print(f"Unexpected finalize response: cmd=0x{r['cmd']:02X}")
            return False

if __name__ == '__main__':
    fw_path = sys.argv[1] if len(sys.argv) > 1 else '/private/tmp/TE-StemPlayer/firmware/armgcc/_build/nrf52840_xxaa.bin'
    flasher = USBFlasher()
    if not flasher.connect():
        sys.exit(1)
    if flasher.flash(fw_path):
        sys.exit(0)
    else:
        sys.exit(1)
