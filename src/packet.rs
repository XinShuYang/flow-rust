use crate::types::*;
use crate::flow::*;
use std::mem;

/* For packet type */
const OFPHTN_ONF: u16 = 0;
const OFPHTN_ETHERTYPE: u16 = 1;

pub const ETH_ADDR_SIZE: usize = mem::size_of::<EtherAddr>();
pub const ETH_TYPE_SIZE: usize = 2;
pub const ETH_HEADER_SIZE: usize = 14;
pub const MAX_VLAN_HEADERS: usize = 2;
pub const VLAN_HEADER_SIZE: usize = 4;
pub const MAX_MPLS_LABELS: usize = 3;

pub const LLC_SNAP_HEADER_SIZE: usize = mem::size_of::<LlcSnapHeader>();
pub const LLC_DSAP_SNAP: u8 = 0xaa;
pub const LLC_SSAP_SNAP: u8 = 0xaa;
pub const LLC_CNTL_SNAP: u8 = 3;

pub const MPLS_HEADER_SIZE: usize = mem::size_of::<MplsHeader>();
pub const MPLS_BOS_SHIFT: u16 = 8;

#[derive(PartialEq, PartialOrd, Debug, Clone, Copy)]
pub enum EtherType {
    NotEth      = 0x05ff,
    Min         = 0x0600,
    Ip          = 0x0800,
    Arp         = 0x0806,
    Erspan2     = 0x22eb,   /* version 2 type III */
    Rarp        = 0x8035,
    Vlan8021Q   = 0x8100,
    Ipv6        = 0x86dd,
    Lacp        = 0x8809,
    Mpls        = 0x8847,
    MplsMcast   = 0x8848,
    Vlan8021AD  = 0x88a8,
    Erspan1     = 0x88be,   /* version 1 type II */
    Nsh         = 0x894f,
}

macro_rules! PACKET_TYPE {
    ($NS: tt, $NS_TYPE: tt) => {
        (($NS as u32) << 16 | ($NS_TYPE as u32))
    };
}

pub const PT_ETH: u32 = PACKET_TYPE!(OFPHTN_ONF, 0x0000);
pub const PT_USE_NEXT_PROTO: u32  = PACKET_TYPE!(OFPHTN_ONF, 0xfffe);  /* Pseudo PT for decap. */
pub const PT_IPV4: u32 = PACKET_TYPE!(OFPHTN_ETHERTYPE, (EtherType::Ip as u16));
pub const PT_IPV6: u32  = PACKET_TYPE!(OFPHTN_ETHERTYPE, (EtherType::Ipv6 as u16));
pub const PT_MPLS: u32 = PACKET_TYPE!(OFPHTN_ETHERTYPE, (EtherType::Mpls as u16));
pub const PT_MPLS_MC: u32 = PACKET_TYPE!(OFPHTN_ETHERTYPE, (EtherType::MplsMcast as u16));
pub const PT_NSH: u32 = PACKET_TYPE!(OFPHTN_ETHERTYPE, (EtherType::Nsh as u16));
pub const PT_UNKNOWN: u32 = PACKET_TYPE!(0xffff, 0xffff);  /* Unknown packet type. */

#[repr(C)]
pub struct EtherAddr(pub [u8; 6]);

impl EtherType {
    pub fn from_u16(value: u16) -> Option<EtherType> {
        match value {
            0x0600 => Some(EtherType::Min),
            0x0800 => Some(EtherType::Ip),
            0x0806 => Some(EtherType::Arp),
            0x22eb => Some(EtherType::Erspan2),
            0x8035 => Some(EtherType::Rarp),
            0x8100 => Some(EtherType::Vlan8021Q),
            0x86dd => Some(EtherType::Ipv6),
            0x8809 => Some(EtherType::Lacp),
            0x8847 => Some(EtherType::Mpls),
            0x8848 => Some(EtherType::MplsMcast),
            0x88a8 => Some(EtherType::Vlan8021AD),
            0x88be => Some(EtherType::Erspan1),
            0x894f => Some(EtherType::Nsh),
            _ => None
        }
    }

    pub fn to_be16(&self) -> u16 {
        return (*self as u16).to_be();
    }
}

#[derive(Copy,Clone,Default)]
#[repr(C)]
pub struct VlanHeader_ {
    pub tpid_be: u16,  /* Vlan8021Q (0x8100) or Vlan8021AD (0x81a8) */
    pub tci_be: u16,
}

#[derive(Copy,Clone)]
pub union VlanHeader {
    pub qtag_be: u32,
    pub qtag2: VlanHeader_,
}

#[derive(Default)]
#[repr(C,packed)]
pub struct LlcHeader {
    pub llc_dsap: u8,
    pub llc_ssap: u8,
    pub llc_cntl: u8,
}

#[derive(Default)]
#[repr(C,packed)]
pub struct SnapHeader {
    pub snap_org: [u8; 3],
    pub snap_type: u16,
}

#[derive(Default)]
#[repr(C,packed)]
pub struct LlcSnapHeader {
    pub llc_header: LlcHeader,
    pub snap_header: SnapHeader,
}

#[derive(Default)]
#[repr(C,packed)]
pub struct MplsHeader {
    pub mpls_lse_hi_be: u16,
    pub mpls_lse_lo_be: u16,
}

pub struct conn {
    // TODO: Add all fields if needed
}

#[derive(Clone,Copy,Default)]
pub struct ovs_key_ct_tuple_ipv4 {
    pub ipv4_src_be: u32,
    pub ipv4_dst_be: u32,
    pub src_port_be: u16,
    pub dst_port_be: u16,
    pub ipv4_proto: u8,
}

#[derive(Clone,Copy,Default)]
pub struct ovs_key_ct_tuple_ipv6 {
    pub ipv6_src: in6_addr,
    pub ipv6_dst: in6_addr,
    pub src_port_be: u16,
    pub dst_port_be: u16,
    pub ipv6_proto: u8
}

#[derive(Clone,Copy)]
#[repr(C)]
pub union ct_orig_tuple {
    pub ipv4: ovs_key_ct_tuple_ipv4,
    pub ipv6: ovs_key_ct_tuple_ipv6,
}

impl Default for ct_orig_tuple {
    fn default() -> ct_orig_tuple {
        ct_orig_tuple {
            ipv4: Default::default(),
        }
    }
}

#[derive(Clone,Copy)]
#[repr(C)]
pub struct pkt_metadata {
    pub recirc_id: u32,
    pub dp_hash: u32,
    pub skb_priority: u32,
    pub pkt_mark: u32,
    pub ct_state: u8,
    pub ct_orig_tuple_ipv6: bool,
    pub ct_zone: u16,
    pub ct_mark: u32,
    pub ct_label: ovs_u128,
    pub in_port: flow_in_port,
    pub conn: *mut conn,
    pub reply: bool,
    pub icmp_related: bool,
    pub pad_to_cacheline_64_1: [u8; 4],

    pub ct_orig_tuple: ct_orig_tuple,
    pub pad_to_cacheline_64_2: [u8; 24],

    pub tunnel: flow_tnl,
}

#[cfg(test)]
mod tests {
    use std::mem;
    use super::*;
    use crate::*;

    #[test]
    fn pkt_metadata_alignment() {
        assert_eq!(offsetOf!(pkt_metadata, icmp_related), 57);
        assert_eq!(offsetOf!(pkt_metadata, ct_orig_tuple), 64);
        assert_eq!(offsetOf!(pkt_metadata, tunnel), 128);
    }

    #[test]
    fn vlan_header() {
        let mut vlan = VlanHeader { qtag_be: 0x11223344 };
        unsafe {
            assert_eq!((0x3344_u16), vlan.qtag2.tpid_be);
        }
    }

    #[test]
    fn llc_snap_header() {
        assert_eq!(mem::size_of::<LlcHeader>(), 3);
        assert_eq!(mem::size_of::<SnapHeader>(), 5);
        assert_eq!(mem::size_of::<LlcSnapHeader>(), 8);
    }
}
