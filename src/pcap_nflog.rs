//! See http://www.tcpdump.org/linktypes/LINKTYPE_NFLOG.html

use nom::{le_u8,le_u16,IResult};
use pcap::Packet;

// Defined in linux/netfilter/nfnetlink_log.h
const NFULA_PAYLOAD : u16 = 9;

#[derive(Debug)]
struct NflogTlv<'a> {
    pub l: u16,
    pub t: u16,
    pub v: &'a[u8],
}

named!(parse_nflog_tlv<NflogTlv>,
    do_parse!(
        l: le_u16 >>
        t: le_u16 >>
        v: take!(l-4) >>
        _padding: cond!(l % 4 != 0,take!(4-(l%4))) >>
        ( NflogTlv{l:l,t:t,v:v} )
    )
);

#[derive(Debug)]
struct NflogHdr<'a> {
    pub af: u8,
    pub vers: u8,
    pub res_id: u16,
    pub data: Vec<NflogTlv<'a>>,
}

named!(parse_nflog_header<NflogHdr>,
    dbg_dmp!(
    do_parse!(
        af: le_u8 >>
        v:  le_u8 >>
        id: le_u16 >>
        d:  many0!(parse_nflog_tlv) >>
        (
            NflogHdr{
                af: af,
                vers: v,
                res_id: id,
                data: d,
            }
        )
    )
    )
);

/// See http://www.tcpdump.org/linktypes/LINKTYPE_NFLOG.html
pub fn get_data_nflog<'a>(packet: &'a Packet) -> &'a[u8] {
    match parse_nflog_header(packet.data) {
        IResult::Done(_,res) => {
            match res.data.into_iter().find(|v| v.t == NFULA_PAYLOAD) {
                Some(v) => v.v,
                None    => panic!("packet with no payload data"),
            }
        },
        e @ _ => panic!("parsing nflog packet header failed: {:?}",e),
    }
}

