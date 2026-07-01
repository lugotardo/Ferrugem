/// ARP Address Resolution Protocol (IPv4 ↔ MAC).

use super::ethernet::MacAddr;

pub type Ipv4Addr = [u8; 4];

const HTYPE_ETHERNET: u16 = 1;
const PTYPE_IPV4:     u16 = 0x0800;
const HLEN: u8 = 6;
const PLEN: u8 = 4;
pub const OP_REQUEST: u16 = 1;
pub const OP_REPLY:   u16 = 2;

#[derive(Clone, Copy)]
pub struct ArpPacket {
    pub op:        u16,
    pub sender_mac: MacAddr,
    pub sender_ip:  Ipv4Addr,
    pub target_mac: MacAddr,
    pub target_ip:  Ipv4Addr,
}

pub fn parse(buf: &[u8]) -> Option<ArpPacket> {
    if buf.len() < 28 {
        return None;
    }
    let op = u16::from_be_bytes([buf[6], buf[7]]);
    let mut sender_mac = [0u8; 6];
    let mut sender_ip  = [0u8; 4];
    let mut target_mac = [0u8; 6];
    let mut target_ip  = [0u8; 4];
    sender_mac.copy_from_slice(&buf[8..14]);
    sender_ip.copy_from_slice(&buf[14..18]);
    target_mac.copy_from_slice(&buf[18..24]);
    target_ip.copy_from_slice(&buf[24..28]);
    Some(ArpPacket {
        op,
        sender_mac: MacAddr(sender_mac),
        sender_ip,
        target_mac: MacAddr(target_mac),
        target_ip,
    })
}

pub fn build(pkt: &ArpPacket, out: &mut [u8]) -> usize {
    if out.len() < 28 {
        return 0;
    }
    out[0..2].copy_from_slice(&HTYPE_ETHERNET.to_be_bytes());
    out[2..4].copy_from_slice(&PTYPE_IPV4.to_be_bytes());
    out[4] = HLEN;
    out[5] = PLEN;
    out[6..8].copy_from_slice(&pkt.op.to_be_bytes());
    out[8..14].copy_from_slice(&pkt.sender_mac.0);
    out[14..18].copy_from_slice(&pkt.sender_ip);
    out[18..24].copy_from_slice(&pkt.target_mac.0);
    out[24..28].copy_from_slice(&pkt.target_ip);
    28
}
