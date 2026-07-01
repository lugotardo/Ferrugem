pub fn parse(buf: &[u8]) -> Option<(u16, u16, &[u8])> {
    if buf.len() < 8 {
        return None;
    }
    let src_port = u16::from_be_bytes([buf[0], buf[1]]);
    let dst_port = u16::from_be_bytes([buf[2], buf[3]]);
    Some((src_port, dst_port, &buf[8..]))
}

pub fn build(src: u16, dst: u16, payload: &[u8], out: &mut [u8]) -> usize {
    let len = 8 + payload.len();
    if out.len() < len {
        return 0;
    }
    out[0..2].copy_from_slice(&src.to_be_bytes());
    out[2..4].copy_from_slice(&dst.to_be_bytes());
    out[4..6].copy_from_slice(&(len as u16).to_be_bytes());
    out[6..8].copy_from_slice(&0u16.to_be_bytes()); // checksum (optional for UDP)
    out[8..len].copy_from_slice(payload);
    len
}
