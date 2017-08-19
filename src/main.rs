#[macro_use]
extern crate log;
extern crate env_logger;

extern crate pcap;
extern crate pnet;

use std::collections::HashMap;

extern crate argparse;
use argparse::{ArgumentParser, StoreTrue, Store};

use pnet::packet::PacketSize;
use pnet::packet::Packet;
//use pnet::packet::ethernet::EthernetPacket;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::tcp::TcpPacket;
use pnet::packet::udp::UdpPacket;

use pnet::packet::ip::IpNextHeaderProtocols;

#[macro_use]
extern crate nom;
use nom::HexDisplay;

extern crate rusticata;
use rusticata::RParser;

mod registry;
use registry::ParserRegistry;

mod five_tuple;
use five_tuple::{FiveTuple,ToFiveTuple};

mod pcap_nflog;
use pcap_nflog::get_data_nflog;

struct GlobalState {
    registry: ParserRegistry,
    sessions: HashMap<FiveTuple, Box<RParser>>,
}

impl GlobalState {
    pub fn new() -> GlobalState {
        GlobalState{
            registry: ParserRegistry::new(),
            sessions: HashMap::new(),
        }
    }
}

fn parse_data_as(parser: &mut RParser, i: &[u8], direction: u8)
{
    if i.len() == 0 {
        return;
    }

    // let mut state = TLS_STATE.lock().unwrap();
    // state.parse_tcp_level(i);
    parser.parse(i, direction);
}

fn callback(data:&[u8], ptype: &String, globalstate: &mut GlobalState)
{
    debug!("----------------------------------------");
    debug!("raw packet:\n{}", data.to_hex(16));

    //let ref ether = EthernetPacket::new(packet.data).unwrap();
    let ref ipv4 = Ipv4Packet::new(data).unwrap();
    // debug!("next level proto: {:?}", ipv4.get_next_level_protocol());

    let mut five_t = ipv4.get_five_tuple();
    let mut direction : u8 = 0;
    debug!("5T: {:?}", five_t);

    if !globalstate.sessions.contains_key(&five_t) {
        // not found, lookup reverse hash
        let rev_five_t = five_t.get_reverse();
        debug!("rev 5T: {:?}", rev_five_t);
        if globalstate.sessions.contains_key(&rev_five_t) {
            debug!("found reverse hash");
            five_t = rev_five_t;
            direction = 1;
        } else {
            debug!("Creating new session");
            globalstate.sessions.insert(five_t.clone(), globalstate.registry.create(ptype).unwrap());
        }
    }
    let pp  = globalstate.sessions.get_mut(&five_t).unwrap();
    let p = &mut (**pp);

    if ipv4.get_next_level_protocol() == IpNextHeaderProtocols::Tcp {
        match TcpPacket::new(ipv4.payload()) {
            Some(ref tcp) => {
                debug!("    TCP {:?}:{} -> {:?}:{}",
                       ipv4.get_source(), tcp.get_source(),
                       ipv4.get_destination(), tcp.get_destination());
                //debug!("tcp payload: {:?}", tcp.payload());
                let mut payload = tcp.payload();
                // heuristic to catch vss-monitoring extra bytes
                if ipv4.packet_size() + tcp.packet().len() != (ipv4.get_total_length() as usize) {
                    let extra = (ipv4.packet_size() + tcp.packet().len()) - (ipv4.get_total_length() as usize);
                    info!("Removing {} extra bytes",extra);
                    let new_len = payload.len() - extra;
                    payload = &payload[0..new_len];
                };

                // XXX check if data is indeed TLS/...
                parse_data_as(p, payload, direction);
            },
            None => (), // not a TCP packet, ignore
        }
    }
    if ipv4.get_next_level_protocol() == IpNextHeaderProtocols::Udp {
        match UdpPacket::new(ipv4.payload()) {
            Some(ref udp) => {
                debug!("    UDP {:?}:{} -> {:?}:{}",
                       ipv4.get_source(), udp.get_source(),
                       ipv4.get_destination(), udp.get_destination());
                let mut payload = udp.payload();
                // heuristic to catch vss-monitoring extra bytes
                if ipv4.packet_size() + udp.packet().len() != (ipv4.get_total_length() as usize) {
                    let extra = (ipv4.packet_size() + udp.packet().len()) - (ipv4.get_total_length() as usize);
                    info!("Removing {} extra bytes",extra);
                    let new_len = payload.len() - extra;
                    payload = &payload[0..new_len];
                };
                // XXX check if data is indeed IPsec/NTP/...
                parse_data_as(p, payload, direction);
            },
            None => (), // not a UDP packet, ignore
        }
    }
}

fn get_data_ethernet<'a>(packet: &'a pcap::Packet) -> &'a[u8] {
    &packet.data[14..]
}

/// See http://www.tcpdump.org/linktypes/LINKTYPE_LINUX_SLL.html
fn get_data_linux_cooked<'a>(packet: &'a pcap::Packet) -> &'a[u8] {
    &packet.data[16..]
}

fn main() {
    let _ = env_logger::init().unwrap();

    let mut verbose = false;
    let mut parser = "tls".to_string();
    let mut filename = "".to_string();

    {  // this block limits scope of borrows by ap.refer() method
        let mut ap = ArgumentParser::new();
        ap.set_description("Greet somebody.");
        ap.refer(&mut verbose)
            .add_option(&["-v", "--verbose"], StoreTrue,
            "Be verbose");
        ap.refer(&mut parser)
            .add_option(&["-p","--parser"], Store,
            "Parser to use");
        ap.refer(&mut filename)
            .add_option(&["-f","--file"], Store,
            "File to parse");
        ap.parse_args_or_exit();
    }

    let mut globalstate = GlobalState::new();

    let mut cap = pcap::Capture::from_file(filename).unwrap();
    println!("datalink: {:?}",cap.get_datalink());

    // See http://www.tcpdump.org/linktypes.html
    let get_data = match cap.get_datalink() {
        pcap::Linktype(1) => get_data_ethernet,
        pcap::Linktype(113) => get_data_linux_cooked,
        pcap::Linktype(239) => get_data_nflog,
        _ => panic!("unsupported data link type"),
    };

    while let Ok(packet) = cap.next() {
        let data = get_data(&packet);
        callback(data, &parser, &mut globalstate);
    }
}
