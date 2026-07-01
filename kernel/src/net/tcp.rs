/// TCP stub state machine and segments (v0.3+).

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

pub const FLAG_FIN: u8 = 0x01;
pub const FLAG_SYN: u8 = 0x02;
pub const FLAG_RST: u8 = 0x04;
pub const FLAG_PSH: u8 = 0x08;
pub const FLAG_ACK: u8 = 0x10;

pub fn parse(buf: &[u8]) -> Option<(u16, u16, u32, u32, u8, &[u8])> {
    if buf.len() < 20 {
        return None;
    }
    let src   = u16::from_be_bytes([buf[0], buf[1]]);
    let dst   = u16::from_be_bytes([buf[2], buf[3]]);
    let seq   = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let ack   = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let off   = ((buf[12] >> 4) as usize) * 4;
    let flags = buf[13];
    if buf.len() < off {
        return None;
    }
    Some((src, dst, seq, ack, flags, &buf[off..]))
}
