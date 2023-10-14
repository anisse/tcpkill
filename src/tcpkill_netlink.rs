use std::net::IpAddr;

use netlink_packet_core::{
    NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_ACK, NLM_F_REQUEST,
};
use netlink_packet_sock_diag::{
    inet::{ExtensionFlags, InetRequest, SocketId, StateFlags},
    SockDiagDestroy, SockDiagMessage, AF_INET, AF_INET6, IPPROTO_TCP, SOCK_DESTROY,
};
use netlink_sys::{protocols::NETLINK_SOCK_DIAG, Socket, SocketAddr};

pub fn netlink_kill(saddr: IpAddr, sport: u16, daddr: IpAddr, dport: u16) -> Result<(), String> {
    let mut socket = Socket::new(NETLINK_SOCK_DIAG).unwrap();
    let _port_number = socket.bind_auto().unwrap().port_number();
    socket.connect(&SocketAddr::new(0, 0)).unwrap();

    let mut hdr = NetlinkHeader::default();
    hdr.flags = NLM_F_REQUEST | NLM_F_ACK;
    hdr.message_type = SOCK_DESTROY;
    let mut packet = NetlinkMessage::new(
        hdr,
        SockDiagDestroy::new(SockDiagMessage::InetRequest(InetRequest {
            family: match saddr {
                IpAddr::V4(_) => AF_INET,
                IpAddr::V6(_) => AF_INET6,
            },
            protocol: IPPROTO_TCP,
            extensions: ExtensionFlags::empty(),
            states: StateFlags::all(),
            socket_id: SocketId {
                source_port: sport,
                destination_port: dport,
                source_address: saddr,
                destination_address: daddr,
                interface_id: 0,
                cookie: [0xff; 8],
            },
        }))
        .into(),
    );

    packet.finalize();

    let mut buf = vec![0; packet.header.length as usize];

    // Before calling serialize, it is important to check that the buffer in which we're emitting is big
    // enough for the packet, other `serialize()` panics.
    assert_eq!(buf.len(), packet.buffer_len());

    packet.serialize(&mut buf[..]);

    //println!(">>> {:?}", packet);
    if let Err(e) = socket.send(&buf[..], 0) {
        println!("SEND ERROR {}", e);
        return Err(format!("SEND ERROR {}", e));
    }

    let mut receive_buffer = vec![0; 4096];
    let mut offset = 0;
    while let Ok(size) = socket.recv(&mut &mut receive_buffer[..], 0) {
        loop {
            let bytes = &receive_buffer[offset..];
            let rx_packet = <NetlinkMessage<SockDiagMessage>>::deserialize(bytes).unwrap();
            //println!("<<< {:?}", rx_packet);
            //println!("payload {:?}", rx_packet.payload);

            match rx_packet.payload {
                NetlinkPayload::Noop => {}
                NetlinkPayload::InnerMessage(SockDiagMessage::InetResponse(response)) => {
                    println!("{:#?}", response);
                }
                NetlinkPayload::Done(_) => {
                    println!("Done!");
                    return Ok(());
                }
                NetlinkPayload::Error(_) | NetlinkPayload::Overrun(_) | _ => {
                    return Err("wat".to_string())
                }
            }

            offset += rx_packet.header.length as usize;
            if offset == size || rx_packet.header.length == 0 {
                offset = 0;
                break;
            }
        }
    }
    Ok(())
}
