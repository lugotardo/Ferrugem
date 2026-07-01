/// Ethernet II frame parser/builder no external deps.

pub const ETHERTYPE_ARP:  u16 = 0x0806;
pub const ETHERTYPE_IPV4: u16 = 0x0800;

#[derive(Clone, Copy)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: Self = MacAddr([0xFF; 6]);
    pub const ZERO:      Self = MacAddr([0x00; 6]);
}

#[repr(C, packed)]
pub struct EtherFrame<'a> {
    pub dst:      MacAddr,
    pub src:      MacAddr,
    pub ethertype: u16,
    pub payload:  &'a [u8],
}

pub fn parse(buf: &[u8]) -> Option<(MacAddr, MacAddr, u16, &[u8])> {
    if buf.len() < 14 {
        return None;
    }
    let mut dst = [0u8; 6];
    let mut src = [0u8; 6];
    dst.copy_from_slice(&buf[0..6]);
    src.copy_from_slice(&buf[6..12]);
    let etype = u16::from_be_bytes([buf[12], buf[13]]);
    Some((MacAddr(dst), MacAddr(src), etype, &buf[14..]))
}

pub fn build(dst: MacAddr, src: MacAddr, etype: u16, payload: &[u8], out: &mut [u8]) -> usize {
    if out.len() < 14 + payload.len() {
        return 0;
    }
    out[0..6].copy_from_slice(&dst.0);
    out[6..12].copy_from_slice(&src.0);
    out[12..14].copy_from_slice(&etype.to_be_bytes());
    out[14..14 + payload.len()].copy_from_slice(payload);
    14 + payload.len()
}
