# USB Protocol — TE Stem Player (SP-1)

## Device Identification

| Field | Value |
|-------|-------|
| Vendor ID | `0x2367` |
| Product ID | `0x1701` |
| Connector | USB-C |
| MCU | Nordic nRF52840 (ARM Cortex-M4 @ 64MHz) |
| USB Class | CDC ACM (Communications Device Class, Abstract Control Model) |

## Connection Sequence

```
1. Enumerate USB devices → filter by VID:PID 0x2367:0x1701
2. Open device handle
3. Claim CDC ACM interface (typically interface 0 or 1)
4. Configure serial parameters (if needed)
5. Device is ready for communication
```

### Platform-Specific Setup

**Linux**: Requires udev rule for non-root access:
```
# /etc/udev/rules.d/99-stemplayer.rules
SUBSYSTEM=="usb", ATTR{idVendor}=="2367", ATTR{idProduct}=="1701", MODE="0666"
```

**macOS**: No additional setup required (IOKit handles permissions).

**Windows**: May require WinUSB driver installation (or use libusb-win32).

## Communication Methods

### 1. CDC ACM (Serial)

Standard serial communication over USB. Used for device status queries and simple commands.

```rust
// Open serial channel
let port = serialport::new("/dev/ttyACM0", 115200)
    .timeout(Duration::from_millis(1000))
    .open()?;

// Send command
port.write(b"STATUS\n")?;

// Read response
let mut buf = [0u8; 256];
let n = port.read(&mut buf)?;
```

### 2. Vendor Control Transfers

Direct USB control transfers for eMMC read/write operations. Discovered via USB fuzzing.

#### Read (bRequest = 0x01)

```rust
// Read sector from eMMC
fn vendor_read(handle: &DeviceHandle, sector_offset: u32, buf: &mut [u8; 8192]) -> Result<()> {
    let request_type = 0xC0; // Direction: Device-to-Host, Type: Vendor, Recipient: Device
    let b_request = 0x01;    // Read command
    let w_value = (sector_offset & 0xFFFF) as u16;  // Low 16 bits of offset
    let w_index = (sector_offset >> 16) as u16; // High 16 bits of offset
    let timeout = Duration::from_secs(5);

    handle.read_control(request_type, b_request, w_value, w_index, buf, timeout)?;
    Ok(())
}
```

#### Write (bRequest = 0x02)

```rust
// Write sector to eMMC
fn vendor_write(handle: &DeviceHandle, sector_offset: u32, data: &[u8; 8192]) -> Result<()> {
    let request_type = 0x40; // Direction: Host-to-Device, Type: Vendor, Recipient: Device
    let b_request = 0x02;    // Write command
    let w_value = (sector_offset & 0xFFFF) as u16;  // Low 16 bits of offset
    let w_index = (sector_offset >> 16) as u16; // High 16 bits of offset
    let timeout = Duration::from_secs(5);

    handle.write_control(request_type, b_request, w_value, w_index, data, timeout)?;
    Ok(())
}
```

## eMMC Sector Layout

```
Sector 0: Album metadata header
Sector 1..N: Song 1 audio data (contiguous sectors)
Sector N+1..M: Song 2 audio data
...
Last sector: Album closing marker
```

### Album Metadata (Sector 0)

```
Offset  Size    Description
------  ----    -----------
0x0000  13      Magic bytes: "ALBUM_PRESENT" (ASCII)
0x000D  4       Album length (total sectors, u32 big-endian)
0x0011  4       Song count (u32 big-endian)
0x0015  64      Album title (null-terminated, X-padded)
0x0055  ...     Song entries (variable)

Song Entry (136 bytes each):
  +0x00  4      Start sector offset (u32 big-endian)
  +0x04  4      Length in sectors (u32 big-endian)
  +0x08  64     Artist name (null-terminated, X-padded)
  +0x48  64     Song title (null-terminated, X-padded)

Final 13 bytes of metadata: "ALBUM_PRESENT" (closing marker)
```

### Padding Convention
All string fields are 64 bytes. Content is null-terminated, remaining bytes filled with `'X'` (0x58).

```rust
fn pad_string(s: &str, len: usize) -> Vec<u8> {
    let mut buf = vec![0x58u8; len]; // Fill with 'X'
    let bytes = s.as_bytes();
    let copy_len = bytes.len().min(len - 1);
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
    buf[copy_len] = 0x00; // Null terminator
    buf
}
```

## Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| `LIBUSB_ERROR_NO_DEVICE` | Device disconnected | Re-enumerate, notify user |
| `LIBUSB_ERROR_BUSY` | Interface claimed by kernel | Detach kernel driver first |
| `LIBUSB_ERROR_TIMEOUT` | Device unresponsive | Retry up to 3 times, then abort |
| `LIBUSB_ERROR_PIPE` | Invalid request | Verify bRequest and wValue/wIndex |
| `LIBUSB_ERROR_ACCESS` | Permission denied | Check udev rules (Linux) |

## Safety Notes

1. **Always read before write**: Backup current eMMC content before any write operation
2. **Sector alignment**: All operations must be 8192-byte aligned
3. **No concurrent access**: Only one application should access the device at a time
4. **Watchdog**: The bootloader starts a 5-second watchdog timer — account for this during firmware operations
5. **Verify magic bytes**: Always check for `ALBUM_PRESENT` before parsing album data
