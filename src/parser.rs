use byteorder::*;
use crate::flow::*;
use crate::miniflow::*;
use crate::packet::*;
use crate::tun_metadata::*;
use crate::types::*;
use std::mem;
use super::*;

pub struct Parser {}

#[derive(PartialEq, Debug)]
pub enum ParseError {
    BadLength,
    DefaultError,
}

fn is_vlan(eth_type: u16) -> bool {
    if (eth_type == EtherType::Vlan8021Q as u16) || (eth_type == EtherType::Vlan8021AD as u16) {
        return true;
    }
    return false;
}

fn is_mpls(eth_type: u16) -> bool {
    if (eth_type  == EtherType::Mpls as u16) || (eth_type == EtherType::MplsMcast as u16) {
        return true;
    }
    return false;
}

fn parse_mpls (data: &[u8], mpls_labels: &mut [u32; MAX_MPLS_LABELS]) -> (usize, usize) {
    let mut count: usize = 0;
    let mut offset: usize = 0;

    while (data.len() - offset) >= MPLS_HEADER_SIZE {
        let mpls_header = MplsHeader {
            mpls_lse_hi_be: NativeEndian::read_u16(&data[offset..offset+2]),
            mpls_lse_lo_be: NativeEndian::read_u16(&data[offset+2..offset+4]),
        };
        offset += 4;

        if count < MAX_MPLS_LABELS {
            mpls_labels[count] = unsafe {
                mem::transmute_copy::<MplsHeader, u32>(&mpls_header)
            };
        }
        count += 1;

        if (mpls_header.mpls_lse_lo_be & (1_u16 << MPLS_BOS_SHIFT).to_be()) != 0  {
            break;
        }
    }

    return (offset, std::cmp::min(count, MAX_MPLS_LABELS));
}

fn parse_ethertype(data: &[u8]) -> (usize, u16) {
    let mut offset: usize = 0;

    let eth_type = BigEndian::read_u16(data);
    offset += ETH_TYPE_SIZE;

    if eth_type >= EtherType::Min as u16 {
        return (offset, eth_type);
    }

    if data[offset..].len() < LLC_SNAP_HEADER_SIZE {
        return (offset, EtherType::NotEth as u16);
    }

    let mut llc: LlcSnapHeader = Default::default();
    llc.llc_header.llc_dsap = data[offset];
    llc.llc_header.llc_ssap = data[offset+1];
    llc.llc_header.llc_cntl = data[offset+2];
    llc.snap_header.snap_org[0] = data[offset+3];
    llc.snap_header.snap_org[1] = data[offset+4];
    llc.snap_header.snap_org[2] = data[offset+5];
    llc.snap_header.snap_type = BigEndian::read_u16(&data[offset+6..offset+8]);

    if llc.llc_header.llc_dsap != LLC_DSAP_SNAP
        || llc.llc_header.llc_ssap != LLC_SSAP_SNAP
        || llc.llc_header.llc_cntl != LLC_CNTL_SNAP
        || llc.snap_header.snap_org[0] != 0
        || llc.snap_header.snap_org[1] != 0
        || llc.snap_header.snap_org[2] != 0 {
        return (offset, EtherType::NotEth as u16);
    }

    offset += LLC_SNAP_HEADER_SIZE;
    if llc.snap_header.snap_type >= EtherType::Min as u16 {
        return (offset, llc.snap_header.snap_type);
    }

    return (offset, EtherType::NotEth as u16);
}

fn parse_vlan(data: &[u8], vlan_hdrs: &mut [u32; MAX_VLAN_HEADERS]) -> (usize, usize) {
    let mut eth_type = BigEndian::read_u16(data);
    let mut offset : usize = 0;
    let mut n : usize = 0;

    while  is_vlan(eth_type) && n < MAX_VLAN_HEADERS {
        if data.len() < ETH_TYPE_SIZE + VLAN_HEADER_SIZE {
            break;
        }

        let mut vlan_hdr = VlanHeader { qtag_be: 0 };
        vlan_hdr.qtag2.tpid_be = NativeEndian::read_u16(&data[offset..offset+2]);
        offset += 2;
        vlan_hdr.qtag2.tci_be = NativeEndian::read_u16(&data[offset..offset+2]);
        offset += 2;

        unsafe {
            vlan_hdrs[n] = vlan_hdr.qtag_be;
        }
        eth_type = BigEndian::read_u16(&data[offset..offset+2]);
        n += 1;
    }

    return (offset, n);
}

pub fn parse_l2(data: &[u8], mf: &mut miniflow::mf_ctx, packet_type_be: u32)
    -> Result<(usize, u16), ParseError> {
    let mut offset: usize = 0;
    let mut dl_type: u16 = std::u16::MAX;
    let mut l2_5_ofs: u16 = std::u16::MAX;

    if packet_type_be == PT_ETH.to_be() {
        if data.len() < ETH_HEADER_SIZE {
            return Err(ParseError::BadLength);
        }

        miniflow_push_macs!(mf, dl_dst, &data);
        offset += 2 * ETH_ADDR_SIZE;

        /* Parse VLAN */
        let mut vlan_hdrs: [u32; MAX_VLAN_HEADERS] = [0; MAX_VLAN_HEADERS];
        let (used, n_vlans) = parse_vlan(&data[offset..], &mut vlan_hdrs);
        offset += used;

        /* Parse ether type, LLC + SNAP. */
        let (used, eth_type) = parse_ethertype(&data[offset..]);
        offset += used;
        dl_type = eth_type;
        miniflow_push_be16!(mf, dl_type, dl_type.to_be());
        miniflow_pad_to_64!(mf, dl_type);

        if n_vlans > 0 {
            miniflow_push_words_32!(mf, vlans, &vlan_hdrs , n_vlans);
        }
    } else {
        dl_type = u32::from_be(packet_type_be) as u16;
        miniflow_pad_from_64!(mf, dl_type);
        miniflow_push_be16!(mf, dl_type, dl_type.to_be());
        miniflow_pad_to_64!(mf, dl_type);
    }

    /* Parse MPLS */
    if is_mpls(dl_type) {
        l2_5_ofs = offset as u16;
        let mut mpls_labels: [u32; MAX_MPLS_LABELS] = [0; MAX_MPLS_LABELS];
        let (used, count) = parse_mpls(&data[offset..], &mut mpls_labels);
        offset += used;
        miniflow_push_words_32!(mf, mpls_lse, &mpls_labels, count);
    }

    // TODO:L3
    // TODO:L4
    return Ok((offset, l2_5_ofs));
}

pub fn parse_metadata(md: &pkt_metadata, packet_type_be: u32, mf: &mut mf_ctx) {

    if md.tunnel.dst_is_set() {
        let md_size = offsetOf!(flow_tnl, metadata) / mem::size_of::<u64>();
        miniflow_push_words!(mf, tunnel, md.tunnel.as_u64_slice(), md_size);

        if md.tunnel.flags & FLOW_TNL_F_UDPIF == 0 {
            if unsafe {md.tunnel.metadata.present.map != 0} {
                let tun_md_size = mem::size_of::<Tun_metadata>() / mem::size_of::<u64>();
                let offset = offsetOf!(Flow, tunnel) + offsetOf!(flow_tnl, metadata);
                mf.miniflow_push_words_(offset, md.tunnel.metadata.as_u64_slice(), tun_md_size);
            }
        } else {
            if unsafe {md.tunnel.metadata.present.len != 0} {
                let offset = offsetOf!(Flow, tunnel) + offsetOf!(flow_tnl, metadata)
                                + offsetOf!(Tun_metadata, present);
                mf.miniflow_push_words_(offset, md.tunnel.metadata.present.as_u64_slice(), 1);

                let offset = offsetOf!(Flow, tunnel) + offsetOf!(flow_tnl, metadata)
                                + offsetOf!(Tun_metadata, opts) + offsetOf!(tun_md_opts, gnv);
                mf.miniflow_push_words_(offset, md.tunnel.metadata.opts.as_u64_slice(),
                                        DIV_ROUND_UP!(unsafe{(md.tunnel.metadata.present.len as usize)}, mem::size_of::<u64>()));
            }
        }
    }

    if md.skb_priority != 0 || md.pkt_mark != 0 {
        miniflow_push_uint32!(mf, skb_priority, md.skb_priority);
        miniflow_push_uint32!(mf, pkt_mark, md.pkt_mark);
    }

    miniflow_push_uint32!(mf, dp_hash, md.dp_hash);
    miniflow_push_uint32!(mf, in_port, unsafe {md.in_port.odp_port} );

    if md.ct_state != 0 {
        miniflow_push_uint32!(mf, recirc_id, md.recirc_id);
        miniflow_push_uint8!(mf, ct_state, md.ct_state);
        //TODO: ct_nw_proto_p = miniflow_pointer(mf, ct_nw_proto);

        miniflow_push_uint8!(mf, ct_nw_proto, 0);
        miniflow_push_uint16!(mf, ct_zone, md.ct_zone);
        miniflow_push_uint32!(mf, ct_mark, md.ct_mark);
        miniflow_push_be32!(mf, packet_type, packet_type_be);

        if !md.ct_label.is_zero() {
            mf.miniflow_push_words_(offsetOf!(Flow, ct_label), md.ct_label.as_u64_slice(),
                    mem::size_of::<ovs_u128>() / mem::size_of::<u64>());
        }
    } else {
        if md.recirc_id != 0 {
            miniflow_push_uint32!(mf, recirc_id, md.recirc_id);
            miniflow_pad_to_64!(mf, recirc_id);
        }
        miniflow_pad_from_64!(mf, packet_type);
        miniflow_push_be32!(mf, packet_type, packet_type_be);
    }
}

fn miniflow_extract() {

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn metadata() {
        let md: pkt_metadata = pkt_metadata {
            recirc_id: 0x11,
            dp_hash: 0x22,
            skb_priority : 0x33,
            pkt_mark: 0x44,
            ct_state: 0x5,
            ct_orig_tuple_ipv6: false,
            ct_zone: 0x66,
            ct_mark: 0x77,
            ct_label: ovs_u128 {
                u64_0: C2RustUnnamed {
                    lo: 0x1111,
                    hi: 0x2222,
                }
            },
            in_port: flow_in_port {
                odp_port: 0x99,
            },
            conn: ptr::null_mut(),
            reply: false,
            icmp_related: false,
            pad_to_cacheline_64_1: [0_u8; 4],
            ct_orig_tuple: Default::default(),
            pad_to_cacheline_64_2: [0_u8; 24],
            tunnel: Default::default(),
        };

        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        parse_metadata(&md, 0x0800, &mut mfx);
        let expected: &mut [u64] =
            &mut [0x0000004400000033, 0x0000009900000022, 0x0066000500000011, 0x0000080000000077,
                    0x1111, 0x2222, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
    }

    #[test]
    fn metadata_no_ct_state() {
        let md: pkt_metadata = pkt_metadata {
            recirc_id: 0x11,
            dp_hash: 0x22,
            skb_priority : 0x33,
            pkt_mark: 0x44,
            ct_state: 0x0,
            ct_orig_tuple_ipv6: false,
            ct_zone: 0x0,
            ct_mark: 0x0,
            ct_label: ovs_u128 {
                u64_0: C2RustUnnamed {
                    lo: 0x0,
                    hi: 0x0,
                }
            },
            in_port: flow_in_port {
                odp_port: 0x99,
            },
            conn: ptr::null_mut(),
            reply: false,
            icmp_related: false,
            pad_to_cacheline_64_1: [0_u8; 4],
            ct_orig_tuple: Default::default(),
            pad_to_cacheline_64_2: [0_u8; 24],
            tunnel: Default::default(),
        };

        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        parse_metadata(&md, 0x0800, &mut mfx);
        let expected: &mut [u64] =
            &mut [0x0000004400000033, 0x0000009900000022, 0x11, 0x0000080000000000,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
    }

    #[test]
    fn l2_bad_length() {
        let mut mf: Miniflow = Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        let data = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(parse_l2(&data, &mut mfx, PT_ETH.to_be()).err(), Some(ParseError::BadLength));
    }

    #[test]
    fn l2_ethernet() {
        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, /* dst MAC */
                    0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, /* src MAC */
                    0x08, 0x00];                        /* EtherType */
        assert_eq!(parse_l2(&data, &mut mfx, PT_ETH.to_be()).is_ok(), true);

        let expected: &mut [u64] =
            &mut [0x7766554433221100, 0x0008bbaa9988, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
        assert_eq!(mfx.map.bits, [0x1800000000000000, 0]);
    }

    #[test]
    fn l2_vlan() {
        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, /* dst MAC */
                    0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, /* src MAC */
                    0x81, 0x00,                         /* vlan: TPID */
                    0x01, 0xFF,                         /* vlan: TCI */
                    0x08, 0x00];                        /* EtherType */
        assert_eq!(parse_l2(&data, &mut mfx, PT_ETH.to_be()).is_ok(), true);

        let expected: &mut [u64] =
            &mut [0x7766554433221100, 0x0008bbaa9988, 0xFF010081, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
    }

    #[test]
    fn l2_vlan_double_tagging() {
        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, /* dst MAC */
                    0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, /* src MAC */
                    0x88, 0xA8,                         /* vlan: TPID */
                    0x01, 0xFF,                         /* vlan: TCI */
                    0x81, 0x00,                         /* vlan: TPID */
                    0x02, 0xFF,                         /* vlan: TCI */
                    0x08, 0x00];                        /* EtherType */
        assert_eq!(parse_l2(&data, &mut mfx, PT_ETH.to_be()).is_ok(), true);

        let expected: &mut [u64] =
            &mut [0x7766554433221100, 0x0008bbaa9988, 0xFF020081FF01A888, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
    }

    #[test]
    fn l2_mpls() {
        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, /* dst MAC */
                    0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, /* src MAC */
                    0x88, 0x47,                         /* EtherType (MPLS) */
                    0x00, 0x11, 0x00, 0x22,
                    0x00, 0x11, 0x00, 0x33,
                    0x00, 0x11, 0x00, 0x44,
                    0x00, 0x11, 0x01, 0x55];
        assert_eq!(parse_l2(&data, &mut mfx, PT_ETH.to_be()).is_ok(), true);

        let expected: &mut [u64] =
            &mut [0x7766554433221100, 0x4788bbaa9988, 0x3300110022001100, 0x44001100,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
    }

    #[test]
    fn l2_llc_snap() {
        let mut mf: miniflow::Miniflow = miniflow::Miniflow::new();
        let mut mfx = &mut mf_ctx::from_mf(mf.map, &mut mf.values);

        let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, /* dst MAC */
                    0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, /* src MAC */
                    0x00, 0x10,                         /* Length */
                    0xaa, 0xaa, 0x03,                   /* LLC */
                    0x00, 0x00, 0x00, 0x09, 0x00,       /* SNAP */
                    0x00, 0x11, 0x00, 0x44,
                    0x00, 0x11, 0x01, 0x55];
        assert_eq!(parse_l2(&data, &mut mfx, PT_ETH.to_be()).is_ok(), true);

        let expected: &mut [u64] =
            &mut [0x7766554433221100, 0x0009bbaa9988, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(mfx.data, expected);
    }

}
