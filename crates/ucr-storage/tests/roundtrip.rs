#[test]
fn contact_group_message_roundtrip() {
    let store = ucr_storage::Store::in_memory().unwrap();
    let peer = ucr_protocol::NodeId([7u8; 16]);
    let c = ucr_storage::Contact {
        node_id: peer,
        callsign: "X".into(),
        desc: "d".into(),
        class: ucr_protocol::station::StationClass::Individual,
        pubkey: Some([1u8; 32]),
        verified: true,
        blocked: false,
        muted_until_ms: None,
        favourite: false,
        trusted: false,
        first_seen_ms: 1,
        last_seen_ms: 2,
        source: ucr_storage::ContactSource::OverTheAir,
    };
    store.upsert_contact(&c).unwrap();
    let back = store.contact(&peer).unwrap();
    assert_eq!(back.as_ref().map(|b| b.callsign.as_str()), Some("X"));
    assert!(!store.is_blocked(&peer).unwrap());

    let g = ucr_storage::StoredGroup {
        group_id: ucr_protocol::GroupId([8u8; 8]),
        name: "g".into(),
        desc: "gd".into(),
        creator: ucr_protocol::NodeId([9u8; 16]),
        epoch: 1,
        key: [7u8; 32],
        members: vec![],
        muted_until_ms: None,
    };
    store.upsert_group(&g).unwrap();
    let gb = store.group(&g.group_id).unwrap();
    assert_eq!(gb.as_ref().map(|x| x.name.as_str()), Some("g"));

    store
        .put_message(&ucr_storage::StoredMessage {
            msg_id: 3,
            peer,
            channel: None,
            direction: ucr_storage::Direction::Outbound,
            text: "m".into(),
            at_ms: 1,
            delivery: ucr_storage::DeliveryState::Queued,
        })
        .unwrap();
    store
        .set_delivery(None, &peer, 3, ucr_storage::DeliveryState::Delivered)
        .unwrap();
    let h = store.history(None, &peer, 10).unwrap();
    assert_eq!(h[0].delivery, ucr_storage::DeliveryState::Delivered);
}
