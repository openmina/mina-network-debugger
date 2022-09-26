use std::{
    net::{SocketAddr, IpAddr, Ipv6Addr, Ipv4Addr},
    time::{SystemTime, Duration},
};

use radiation::{Absorb, Emit, nom, ParseError};

pub fn addr_absorb(input: &[u8]) -> nom::IResult<&[u8], SocketAddr, ParseError<&[u8]>> {
    let pair = nom::sequence::pair(<[u8; 16]>::absorb::<()>, u16::absorb::<()>);
    nom::combinator::map(pair, |(ip, port)| {
        let ipv6 = Ipv6Addr::from(ip);
        if let [0, 0, 0, 0, 0, 0xFFFF, ..] = ipv6.segments() {
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15])),
                port,
            )
        } else {
            SocketAddr::new(IpAddr::V6(ipv6), port)
        }
    })(input)
}

pub fn addr_emit<W>(value: &SocketAddr, buffer: W) -> W
where
    W: Extend<u8>,
{
    let ip = match value.ip() {
        IpAddr::V6(ip) => ip.octets(),
        IpAddr::V4(ip) => ip.to_ipv6_mapped().octets(),
    };
    value.port().emit(ip.emit(buffer))
}

pub fn duration_absorb(input: &[u8]) -> nom::IResult<&[u8], Duration, ParseError<&[u8]>> {
    nom::combinator::map(
        nom::sequence::pair(u64::absorb::<()>, u32::absorb::<()>),
        |(secs, nanos)| Duration::new(secs, nanos),
    )(input)
}

pub fn duration_emit<W>(value: &Duration, buffer: W) -> W
where
    W: Extend<u8>,
{
    value.subsec_nanos().emit(value.as_secs().emit(buffer))
}

pub fn time_absorb(input: &[u8]) -> nom::IResult<&[u8], SystemTime, ParseError<&[u8]>> {
    nom::combinator::map(duration_absorb, |d| SystemTime::UNIX_EPOCH + d)(input)
}

pub fn time_emit<W>(value: &SystemTime, buffer: W) -> W
where
    W: Extend<u8>,
{
    duration_emit(
        &value
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("after unix epoch"),
        buffer,
    )
}
