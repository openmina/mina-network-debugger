use std::{
    net::{SocketAddr, IpAddr, Ipv6Addr, Ipv4Addr},
    time::{SystemTime, Duration},
};

use radiation::{Absorb, Emit, nom, ParseError, RadiationBuffer};
use libp2p_core::PeerId;

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

pub fn addr_emit<W>(value: &SocketAddr, buffer: &mut W)
where
    W: for<'a> Extend<&'a u8>,
{
    let ip = match value.ip() {
        IpAddr::V6(ip) => ip.octets(),
        IpAddr::V4(ip) => ip.to_ipv6_mapped().octets(),
    };
    ip.emit(buffer);
    value.port().emit(buffer);
}

pub fn duration_absorb(input: &[u8]) -> nom::IResult<&[u8], Duration, ParseError<&[u8]>> {
    nom::combinator::map(
        nom::sequence::pair(u64::absorb::<()>, u32::absorb::<()>),
        |(secs, nanos)| Duration::new(secs, nanos),
    )(input)
}

pub fn duration_emit<W>(value: &Duration, buffer: &mut W)
where
    W: for<'a> Extend<&'a u8>,
{
    value.as_secs().emit(buffer);
    value.subsec_nanos().emit(buffer);
}

pub fn time_absorb(input: &[u8]) -> nom::IResult<&[u8], SystemTime, ParseError<&[u8]>> {
    nom::combinator::map(duration_absorb, |d| SystemTime::UNIX_EPOCH + d)(input)
}

pub fn time_emit<W>(value: &SystemTime, buffer: &mut W)
where
    W: for<'a> Extend<&'a u8>,
{
    duration_emit(
        &value
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("after unix epoch"),
        buffer,
    );
}

pub fn peer_id_absorb(input: &[u8]) -> nom::IResult<&[u8], PeerId, ParseError<&[u8]>> {
    nom::combinator::map_res(Vec::<u8>::absorb::<()>, |v| PeerId::from_bytes(&v))(input)
}

pub fn peer_id_emit<W>(value: &PeerId, buffer: &mut W)
where
    W: for<'a> Extend<&'a u8> + RadiationBuffer,
{
    value.to_bytes().emit(buffer);
}

pub fn serialize_peer_id<S>(v: &PeerId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;

    if serializer.is_human_readable() {
        v.to_base58().serialize(serializer)
    } else {
        v.to_bytes().serialize(serializer)
    }
}
