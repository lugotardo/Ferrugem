pub const PROTO_ICMP: u8 = 1;
pub const PROTO_UDP:  u8 = 17;
pub const PROTO_TCP:  u8 = 6;

pub type Ipv4Addr = [u8; 4];

#[derive(Clone, Copy)]
pub struct Ipv4Header {
    pub src:      Ipv4Addr,
    pub dst:      Ipv4Addr,
    pub protocol: u8,
    pub ttl:      u8,
    pub total_len: u16,
}

pub fn parse(buf: &[u8]) -> Option<(Ipv4Header, &[u8])> {
    if buf.len() < 20 {
        return None;
    }
    let ihl = (buf[0] & 0x0F) as usize * 4;
    if buf.len() < ihl {
        return None;
    }
    let mut src = [0u8; 4];
    let mut dst = [0u8; 4];
    src.copy_from_slice(&buf[12..16]);
    dst.copy_from_slice(&buf[16..20]);
    let hdr = Ipv4Header {
        src,
        dst,
        protocol:  buf[9],
        ttl:       buf[8],
        total_len: u16::from_be_bytes([buf[2], buf[3]]),
    };
    Some((hdr, &buf[ihl..]))
}

pub fn build(hdr: &Ipv4Header, payload: &[u8], out: &mut [u8]) -> usize {
    let total = 20 + payload.len();
    if out.len() < total {
        return 0;
    }
    out[0] = 0x45; // version=4, ihl=5
    out[1] = 0;
    out[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    out[4..6].copy_from_slice(&0u16.to_be_bytes()); // id
    out[6..8].copy_from_slice(&0u16.to_be_bytes()); // flags/frag
    out[8] = hdr.ttl;
    out[9] = hdr.protocol;
    out[10..12].copy_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    out[12..16].copy_from_slice(&hdr.src);
    out[16..20].copy_from_slice(&hdr.dst);
    out[20..20 + payload.len()].copy_from_slice(payload);
    // Compute checksum
    let csum = checksum(&out[..20]);
    out[10..12].copy_from_slice(&csum.to_be_bytes());
    total
}

pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
