use std::{
    net::{SocketAddr, IpAddr},
    time::{SystemTime, Duration},
};

use radiation::{Absorb, Emit, nom, ParseError};

use super::database::{StreamKind, StreamId};

pub fn addr_absorb(input: &[u8]) -> nom::IResult<&[u8], SocketAddr, ParseError<&[u8]>> {
    let pair = nom::sequence::pair(<[u8; 16]>::absorb::<()>, u16::absorb::<()>);
    nom::combinator::map(pair, |(ip, port)| {
        SocketAddr::new(IpAddr::V6(ip.into()), port)
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
        &value.duration_since(SystemTime::UNIX_EPOCH).expect("after unix epoch"),
        buffer,
    )
}

pub fn stream_kind_emit<W>(value: &StreamKind, buffer: W) -> W
where
    W: Extend<u8>,
{
    (*value as u16).emit(buffer)
}

pub fn stream_kind_absorb(input: &[u8]) -> nom::IResult<&[u8], StreamKind, ParseError<&[u8]>> {
    nom::combinator::map(u16::absorb::<()>, |d| {
        for v in StreamKind::iter() {
            if d == v as u16 {
                return v;
            }
        }
        StreamKind::Unknown
    })(input)
}

pub fn stream_meta_emit<W>(value: &StreamId, buffer: W) -> W
where
    W: Extend<u8>,
{
    let d = match value {
        StreamId::Handshake => i64::MIN,
        StreamId::Forward(s) => *s as i64,
        StreamId::Backward(s) => -((*s + 1) as i64),
    };
    d.emit(buffer)
}

pub fn stream_meta_absorb(input: &[u8]) -> nom::IResult<&[u8], StreamId, ParseError<&[u8]>> {
    nom::combinator::map(i64::absorb::<()>, |d| match d {
        i64::MIN => StreamId::Handshake,
        d => {
            if d >= 0 {
                StreamId::Forward(d as u64)
            } else {
                StreamId::Backward((-d - 1) as u64)
            }
        }
    })(input)
}
