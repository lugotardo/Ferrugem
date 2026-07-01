use super::ipv4::checksum;

pub const TYPE_ECHO_REQUEST: u8 = 8;
pub const TYPE_ECHO_REPLY:   u8 = 0;

pub fn parse_echo(buf: &[u8]) -> Option<(u16, u16, &[u8])> {
    if buf.len() < 8 {
        return None;
    }
    let id  = u16::from_be_bytes([buf[4], buf[5]]);
    let seq = u16::from_be_bytes([buf[6], buf[7]]);
    Some((id, seq, &buf[8..]))
}

pub fn build_reply(id: u16, seq: u16, data: &[u8], out: &mut [u8]) -> usize {
    let len = 8 + data.len();
    if out.len() < len {
        return 0;
    }
    out[0] = TYPE_ECHO_REPLY;
    out[1] = 0; // code
    out[2..4].copy_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    out[4..6].copy_from_slice(&id.to_be_bytes());
    out[6..8].copy_from_slice(&seq.to_be_bytes());
    out[8..len].copy_from_slice(data);
    let csum = checksum(&out[..len]);
    out[2..4].copy_from_slice(&csum.to_be_bytes());
    len
}
