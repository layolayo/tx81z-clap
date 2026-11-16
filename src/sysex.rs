//! Yamaha TX81Z MIDI System Exclusive (SysEx) message builders and parsers.

pub const YAMAHA_ID: u8 = 0x43;

/// TX81Z SysEx Parameter Change subgroup bytes (`0ggggghh`, ggggg = 4 -> 0x10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Subgroup {
    /// Remote Switch / System / Effect / PCED base (`0x10`)
    SystemOrRemote = 0x10,
    /// Voice Edit Parameters (`VCED`, `0x12`)
    Vced = 0x12,
    /// Additional Voice Edit Parameters (`ACED`, `0x13`)
    Aced = 0x13,
}

/// Build a 7-byte Yamaha TX81Z Parameter Change SysEx message:
/// `F0 43 1n <subgroup> <param> <value> F7`
pub fn build_param_change(channel: u8, subgroup: Subgroup, param: u8, value: u8) -> [u8; 7] {
    let ch = channel & 0x0F;
    [
        0xF0,
        YAMAHA_ID,
        0x10 | ch,
        subgroup as u8,
        param & 0x7F,
        value & 0x7F,
        0xF7,
    ]
}

/// Build a Remote Switch press/release message (`F0 43 1n 10 <switch> <val> F7`).
pub fn build_remote_switch(channel: u8, switch_param: u8, on: bool) -> [u8; 7] {
    build_param_change(
        channel,
        Subgroup::SystemOrRemote,
        switch_param,
        if on { 0x7F } else { 0x00 },
    )
}

/// Build a Dump Request for the current VCED edit buffer (`F0 43 2n 03 F7`).
pub fn build_vced_dump_request(channel: u8) -> [u8; 5] {
    [0xF0, YAMAHA_ID, 0x20 | (channel & 0x0F), 0x03, 0xF7]
}

/// Build a 7E-header Dump Request (`F0 43 2n 7E <10-byte ASCII header> F7`).
pub fn build_7e_dump_request(channel: u8, header: &[u8; 10]) -> [u8; 15] {
    let mut msg = [0u8; 15];
    msg[0] = 0xF0;
    msg[1] = YAMAHA_ID;
    msg[2] = 0x20 | (channel & 0x0F);
    msg[3] = 0x7E;
    msg[4..14].copy_from_slice(header);
    msg[14] = 0xF7;
    msg
}

pub fn build_aced_dump_request(channel: u8) -> [u8; 15] {
    build_7e_dump_request(channel, b"LM  8976AE")
}

pub fn build_pced_dump_request(channel: u8) -> [u8; 15] {
    build_7e_dump_request(channel, b"LM  8976PE")
}

pub fn build_system_dump_request(channel: u8) -> [u8; 15] {
    build_7e_dump_request(channel, b"LM  8976S0")
}

pub fn build_prog_table_dump_request(channel: u8) -> [u8; 15] {
    build_7e_dump_request(channel, b"LM  8976S1")
}

pub fn build_effect_dump_request(channel: u8) -> [u8; 15] {
    build_7e_dump_request(channel, b"LM  8976S2")
}

/// Compute Yamaha 7-bit two's complement checksum over a bulk data payload.
pub fn yamaha_checksum(data: &[u8]) -> u8 {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    ((!sum).wrapping_add(1) & 0x7F) as u8
}

/// Build a 101-byte 1-Voice VCED Bulk Dump (`F0 43 0n 03 00 5D <93 bytes> <chk> F7`).
pub fn build_vced_bulk_dump(channel: u8, vced: &[u8; 93]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(101);
    msg.extend_from_slice(&[0xF0, YAMAHA_ID, channel & 0x0F, 0x03, 0x00, 0x5D]);
    msg.extend_from_slice(vced);
    msg.push(yamaha_checksum(vced));
    msg.push(0xF7);
    msg
}

/// Build a 7E-header Bulk Dump (`F0 43 0n 7E <len_msb> <len_lsb> <10-byte header> <data> <chk> F7`).
pub fn build_7e_bulk_dump(channel: u8, header: &[u8; 10], data: &[u8]) -> Vec<u8> {
    let payload_len = 10 + data.len();
    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(header);
    payload.extend_from_slice(data);

    let msb = ((payload_len >> 7) & 0x7F) as u8;
    let lsb = (payload_len & 0x7F) as u8;

    let mut msg = Vec::with_capacity(payload_len + 8);
    msg.extend_from_slice(&[0xF0, YAMAHA_ID, channel & 0x0F, 0x7E, msb, lsb]);
    msg.extend_from_slice(&payload);
    msg.push(yamaha_checksum(&payload));
    msg.push(0xF7);
    msg
}

pub fn build_aced_bulk_dump(channel: u8, aced: &[u8; 23]) -> Vec<u8> {
    build_7e_bulk_dump(channel, b"LM  8976AE", aced)
}

pub fn build_pced_bulk_dump(channel: u8, pced: &[u8; 110]) -> Vec<u8> {
    build_7e_bulk_dump(channel, b"LM  8976PE", pced)
}

/// Parsed bulk dump result from the TX81Z or a `.syx` file.
#[derive(Debug, Clone)]
pub enum ParsedDump {
    /// 93-byte VCED edit buffer
    Vced([u8; 93]),
    /// 23-byte ACED edit buffer
    Aced([u8; 23]),
    /// 110-byte PCED performance edit buffer
    Pced([u8; 110]),
    /// Any other raw Yamaha SysEx message (System S0, Prog Table S1, Effect S2, MicroTune, VMEM, etc.)
    Other(Vec<u8>),
}

/// Attempt to parse an incoming Yamaha TX81Z SysEx message.
pub fn parse_sysex_dump(msg: &[u8]) -> Option<ParsedDump> {
    if msg.len() < 8 || msg.first() != Some(&0xF0) || msg.last() != Some(&0xF7) {
        return None;
    }
    if msg[1] != YAMAHA_ID || (msg[2] & 0xF0) != 0x00 {
        return None;
    }

    // 1-Voice VCED Bulk Dump: F0 43 0n 03 00 5D <93 bytes> <checksum> F7 (total 101 bytes)
    if msg[3] == 0x03 && msg.len() >= 101 {
        let len = ((msg[4] as usize) << 7) | (msg[5] as usize);
        if len == 93 && msg.len() == 6 + 93 + 2 {
            let mut buf = [0u8; 93];
            buf.copy_from_slice(&msg[6..99]);
            return Some(ParsedDump::Vced(buf));
        }
    }

    // 7E Bulk Dumps: F0 43 0n 7E <msb> <lsb> "LM  8976.." <data> <checksum> F7
    if msg[3] == 0x7E && msg.len() >= 18 {
        if &msg[6..16] == b"LM  8976AE" && msg.len() >= 16 + 23 + 2 {
            let mut buf = [0u8; 23];
            buf.copy_from_slice(&msg[16..39]);
            return Some(ParsedDump::Aced(buf));
        }
        if &msg[6..16] == b"LM  8976PE" && msg.len() >= 16 + 110 + 2 {
            let mut buf = [0u8; 110];
            buf.copy_from_slice(&msg[16..126]);
            return Some(ParsedDump::Pced(buf));
        }
    }

    Some(ParsedDump::Other(msg.to_vec()))
}

/// Split a `.syx` byte buffer into individual `F0 ... F7` messages.
pub fn split_syx_messages(data: &[u8]) -> Vec<Vec<u8>> {
    let mut messages = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0xF0 {
            if let Some(end_rel) = data[i..].iter().position(|&b| b == 0xF7) {
                messages.push(data[i..=i + end_rel].to_vec());
                i += end_rel + 1;
                continue;
            }
        }
        i += 1;
    }
    messages
}

/// Format a byte slice as uppercase hex for the SysEx monitor.
pub fn format_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}
