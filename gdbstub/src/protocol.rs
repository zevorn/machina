// GDB Remote Serial Protocol packet framing.
//
// Handles the wire format: $<data>#<checksum> with
// '+'/'-' acknowledgment.

use std::io::{self, Read, Write};

/// Receive one RSP packet, returning the data payload.
/// Skips until '$', reads until '#', validates checksum.
/// Sends '+' ACK after successful receive.
pub fn recv_packet<R: Read + Write>(stream: &mut R) -> io::Result<String> {
    let mut byte = [0u8; 1];

    // Skip until '$' start marker.
    loop {
        stream.read_exact(&mut byte)?;
        match byte[0] {
            b'$' => break,
            b'+' | b'-' => continue,
            0x03 => return Ok("\x03".to_string()),
            _ => continue,
        }
    }

    // Read data until '#' end marker.
    let mut data = Vec::new();
    loop {
        stream.read_exact(&mut byte)?;
        if byte[0] == b'#' {
            break;
        }
        data.push(byte[0]);
    }

    // Read 2-char hex checksum.
    let mut cksum_buf = [0u8; 2];
    stream.read_exact(&mut cksum_buf)?;

    // Verify checksum.
    let expected = (hex_val(cksum_buf[0]) << 4) | hex_val(cksum_buf[1]);
    let actual: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    if expected != actual {
        // Send NAK.
        let _ = stream.write_all(b"-");
        let _ = stream.flush();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "checksum mismatch",
        ));
    }

    // Send ACK.
    stream.write_all(b"+")?;
    stream.flush()?;

    String::from_utf8(data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Send one RSP packet: $<data>#<checksum>.
pub fn send_packet<W: Write>(stream: &mut W, data: &str) -> io::Result<()> {
    let bytes = data.as_bytes();
    let cksum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    write!(stream, "${}#{:02x}", data, cksum)?;
    stream.flush()?;
    Ok(())
}

/// Send packet and wait for '+' ACK.
pub fn send_packet_wait_ack<R: Read, W: Write>(
    rx: &mut R,
    tx: &mut W,
    data: &str,
    no_ack: bool,
) -> io::Result<()> {
    send_packet(tx, data)?;
    if !no_ack {
        let mut byte = [0u8; 1];
        rx.read_exact(&mut byte)?;
        if byte[0] != b'+' {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected ACK '+'",
            ));
        }
    }
    Ok(())
}

/// Parse a hex nibble from an ASCII byte.
fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// Parse a hex string to u64.
pub fn parse_hex(s: &str) -> u64 {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(s, 16).unwrap_or(0)
}

/// Encode a u64 as hex string (lowercase, no prefix).
pub fn encode_hex_u64(val: u64) -> String {
    format!("{:x}", val)
}

/// Encode a byte slice as hex string.
pub fn encode_hex_bytes(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Parse a hex-encoded byte slice.
pub fn decode_hex_bytes(hex: &str) -> io::Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "odd hex length",
        ));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let hi = hex_val(hex.as_bytes()[i]);
        let lo = hex_val(hex.as_bytes()[i + 1]);
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Encode bytes as little-endian register value in hex
/// (GDB RSP convention: target endianness, RISC-V is LE).
pub fn encode_reg_hex(val: u64) -> String {
    val.to_le_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Parse a little-endian hex register value.
pub fn decode_reg_hex(hex: &str) -> u64 {
    let bytes = decode_hex_bytes(hex).unwrap_or_default();
    let mut arr = [0u8; 8];
    let len = bytes.len().min(8);
    arr[..len].copy_from_slice(&bytes[..len]);
    u64::from_le_bytes(arr)
}
