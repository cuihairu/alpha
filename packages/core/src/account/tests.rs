
use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn records(pairs: Vec<SyncRecord>) -> BTreeMap<String, SyncRecord> {
    pairs.into_iter().map(|r| (r.key.clone(), r)).collect()
}

#[test]
fn account_id_slug_is_stable_and_collision_free() {
    // 干净串原样返回
    assert_eq!(normalize_account_id("alice"), "alice");
    assert_eq!(normalize_account_id("  bob_1.2-x "), "bob_1.2-x");
    assert_eq!(normalize_account_id("ALICE"), "alice");
    // 空 → 本机缺省
    assert_eq!(normalize_account_id("   "), LOCAL_ACCOUNT_ID);
    // 折叠后追加摘要尾巴：不同原串不撞键
    let a = normalize_account_id("Alice@corp.com");
    let b = normalize_account_id("a l i c e");
    assert_ne!(a, b);
    assert!(a.starts_with("alice-corp.com-"), "got {}", a);
    assert!(b.starts_with("a-l-i-c-e-"), "got {}", b);
    // 幂等（同一输入两次得到同键）
    assert_eq!(a, normalize_account_id("Alice@corp.com"));
    // 全非安全字符也要有键
    assert!(normalize_account_id("!!!").starts_with("acct-"));
}

#[test]
fn key_validation_shapes() {
    assert!(validate_key("workspace:9f2c").is_ok());
    assert!(validate_key("alert-rule:x.y_z-1").is_ok());
    assert_eq!(
        validate_key("workspace"),
        Err(AccountError::InvalidKey("workspace".into()))
    );
    assert_eq!(
        validate_key(":abc"),
        Err(AccountError::InvalidKey(":abc".into()))
    );
    assert_eq!(
        validate_key("Workspace:abc"),
        Err(AccountError::InvalidKey("Workspace:abc".into()))
    );
    assert_eq!(
        validate_key("workspace:"),
        Err(AccountError::InvalidKey("workspace:".into()))
    );
    // 命名空间超长 / 本地 id 超长 / 总长超限
    assert!(validate_key("aaaaaaaaaaaaaaaaa:x").is_err());
    let long_id = format!("workspace:{}", "x".repeat(MAX_LOCAL_ID_LEN + 1));
    assert!(validate_key(&long_id).is_err());
    let long_ns = format!("{}:x", "n".repeat(MAX_NAMESPACE_LEN + 1));
    assert!(validate_key(&long_ns).is_err());
}

#[test]
fn record_validation_covers_rev_tombstone_and_size() {
    let ok = SyncRecord::live("workspace:a", 1, 10, json!({"name": "盯盘"}));
    assert_eq!(ok.validate(), Ok(()));
    // rev 从 1 起
    let bad_rev = SyncRecord::live("workspace:a", 0, 10, json!({}));
    assert_eq!(bad_rev.validate(), Err(AccountError::InvalidRev));
    // 墓碑必须空载荷
    let mut tomb = SyncRecord::tombstone("workspace:a", 2, 10);
    assert_eq!(tomb.validate(), Ok(()));
    tomb.payload = json!({"name": "x"});
    assert_eq!(tomb.validate(), Err(AccountError::TombstonePayloadNotNull));
    // 超大载荷（64 KiB 上限）
    let big = SyncRecord::live(
        "workspace:a",
        1,
        10,
        json!({ "blob": "x".repeat(MAX_PAYLOAD_BYTES) }),
    );
    assert!(matches!(
        big.validate(),
        Err(AccountError::PayloadTooLarge { limit, .. }) if limit == MAX_PAYLOAD_BYTES
    ));
}

#[test]
fn plan_sync_pushes_local_only_change() {
    let local = records(vec![SyncRecord::live(
        "workspace:a",
        1,
        100,
        json!({"n": 1}),
    )]);
    let plan = plan_sync(
        &local,
        &BTreeMap::new(),
        &BTreeMap::new(),
        ConflictPolicy::NewestWins,
    );
    assert_eq!(plan.push.len(), 1);
    assert!(plan.pull.is_empty());
    assert!(plan.conflicts.is_empty());
    assert!(!plan.is_empty());
}

#[test]
fn plan_sync_pulls_remote_only_change() {
    let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
    let remote = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        100,
        json!({"n": 2}),
    )]);
    let plan = plan_sync(&BTreeMap::new(), &remote, &base, ConflictPolicy::NewestWins);
    assert_eq!(plan.pull.len(), 1);
    assert!(plan.push.is_empty());
    assert!(plan.conflicts.is_empty());
}

#[test]
fn plan_sync_converges_when_both_sides_identical() {
    // 并发同改同值：内容一致 → 下发 rev 高者推进基线，不记冲突
    let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
    let local = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        100,
        json!({"n": 7}),
    )]);
    let remote = records(vec![SyncRecord::live(
        "workspace:a",
        3,
        110,
        json!({"n": 7}),
    )]);
    let plan = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
    assert!(plan.push.is_empty());
    assert_eq!(plan.pull.len(), 1);
    assert_eq!(plan.pull[0].rev, 3, "取 rev 高者推进基线");
    assert!(plan.conflicts.is_empty());
}

#[test]
fn plan_sync_conflict_newest_wins_in_both_directions() {
    let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
    let local = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        200,
        json!({"n": "local"}),
    )]);
    let remote = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        300,
        json!({"n": "remote"}),
    )]);
    let plan = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
    assert_eq!(plan.pull.len(), 1);
    assert!(plan.push.is_empty());
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].winner, ConflictWinner::Remote);

    // 本地更新 → 本地胜
    let local_newer = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        400,
        json!({"n": "local"}),
    )]);
    let plan = plan_sync(&local_newer, &remote, &base, ConflictPolicy::NewestWins);
    assert_eq!(plan.push.len(), 1);
    assert!(plan.pull.is_empty());
    assert_eq!(plan.conflicts[0].winner, ConflictWinner::Local);
}

#[test]
fn plan_sync_conflict_tie_breaks_to_remote_deterministically() {
    // 时间戳并列：两端必须算出同一裁决，否则反复互相覆盖
    let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
    let local = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        100,
        json!({"n": "local"}),
    )]);
    let remote = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        100,
        json!({"n": "remote"}),
    )]);
    let from_local = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
    let from_remote = plan_sync(&remote, &local, &base, ConflictPolicy::NewestWins);
    assert_eq!(from_local.pull[0].key, "workspace:a");
    assert_eq!(from_remote.pull[0].key, "workspace:a");
    assert_eq!(from_local.conflicts[0].winner, ConflictWinner::Remote);
    assert_eq!(from_remote.conflicts[0].winner, ConflictWinner::Remote);
}

#[test]
fn plan_sync_explicit_policies_override_recency() {
    let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
    let local = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        500,
        json!({"n": "local"}),
    )]);
    let remote = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        100,
        json!({"n": "remote"}),
    )]);
    let forced_local = plan_sync(&local, &remote, &base, ConflictPolicy::LocalWins);
    assert_eq!(forced_local.push.len(), 1);
    assert_eq!(forced_local.conflicts[0].winner, ConflictWinner::Local);
    let forced_remote = plan_sync(&local, &remote, &base, ConflictPolicy::RemoteWins);
    assert_eq!(forced_remote.pull.len(), 1);
    assert_eq!(forced_remote.conflicts[0].winner, ConflictWinner::Remote);
}

#[test]
fn plan_sync_tombstones_propagate_and_conflict_with_live() {
    let base = BTreeMap::from([("workspace:a".to_string(), 3u64)]);
    // 本端删除（较新）→ 墓碑上行
    let local = records(vec![SyncRecord::tombstone("workspace:a", 4, 500)]);
    let remote = records(vec![SyncRecord::live(
        "workspace:a",
        3,
        100,
        json!({"n": 1}),
    )]);
    let plan = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
    assert_eq!(plan.push.len(), 1);
    assert!(plan.push[0].deleted);
    assert!(plan.conflicts.is_empty());

    // 他端删除（较新）→ 下发墓碑，本地视图按墓碑移除
    let remote_tomb = records(vec![SyncRecord::tombstone("workspace:a", 4, 600)]);
    let plan = plan_sync(&local, &remote_tomb, &base, ConflictPolicy::NewestWins);
    assert!(plan.push.is_empty());
    assert_eq!(plan.pull.len(), 1);
    assert!(plan.pull[0].deleted);
}

#[test]
fn plan_sync_missing_key_is_no_opinion_not_deletion() {
    // 服务端删除该键（基线有、本地无、服务端无）→ 无传输（本地缺键只是
    // 「没这份数据」，不等于本地删过——删除必须走墓碑）
    let base = BTreeMap::from([("workspace:gone".to_string(), 2u64)]);
    let plan = plan_sync(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &base,
        ConflictPolicy::NewestWins,
    );
    assert!(plan.is_empty());
    assert!(plan.conflicts.is_empty());
}

#[test]
fn two_sided_conflict_converges_after_resolution() {
    // 端到端收敛模拟：A 与 B 都改了同一键，A 裁决后上行，服务端下发，
    // B 应用后与 A 视图逐字节一致
    let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
    let a_view = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        200,
        json!({"n": "A"}),
    )]);
    let b_view = records(vec![SyncRecord::live(
        "workspace:a",
        2,
        100,
        json!({"n": "B"}),
    )]);
    let a_base = base.clone();
    let b_base = base.clone();

    // A 侧：本地较新 → 上行
    let a_plan = plan_sync(&a_view, &b_view, &a_base, ConflictPolicy::NewestWins);
    let pushed = a_plan.push.clone();
    assert_eq!(pushed.len(), 1);

    // 服务端接受（rev 3）并回传权威副本
    let mut server = b_view.clone();
    for record in &pushed {
        server.insert(
            record.key.clone(),
            SyncRecord::live(record.key.clone(), 3, 400, record.payload.clone()),
        );
    }
    // A 应用服务端回执，基线推进
    let mut a_state = SyncState::new();
    a_state.base = base.clone();
    let mut a_local = a_view.clone();
    let response = SyncResponse {
        cursor: 10,
        accepted: server.values().cloned().collect(),
        rejected: vec![],
        changes: vec![],
    };
    let outcome = a_state.apply_response(&response);
    SyncState::apply_to_view(&mut a_local, &outcome.applied);

    // B 侧：拉到权威副本后与服务端一致
    let b_plan = plan_sync(&b_view, &server, &b_base, ConflictPolicy::NewestWins);
    let mut b_local = b_view.clone();
    SyncState::apply_to_view(&mut b_local, &b_plan.pull);
    assert_eq!(a_local, b_local, "两端收敛到同一份数据");
    // B 的本地改动被 A 覆盖 = 真冲突，须留痕（胜方服务端：NewestWins
    // 比 updated_at，A 的 400 > B 的 100）
    assert_eq!(b_plan.conflicts.len(), 1);
    assert_eq!(b_plan.conflicts[0].winner, ConflictWinner::Remote);
}

#[test]
fn sync_state_outbox_coalesces_and_bounds() {
    let mut state = SyncState::new();
    for i in 0..(MAX_PUSHES_PER_REQUEST + 10) {
        state.note_local_change(
            &format!("workspace:{:03}", i),
            json!({"i": i}),
            100 + i as i64,
        );
    }
    let pending = state.pending_pushes();
    assert_eq!(pending.len(), MAX_PUSHES_PER_REQUEST, "超出部分留队下一轮");
    // 同键后写覆盖前写（连续编辑不堆多条）
    state.note_local_change("workspace:000", json!({"i": 999}), 999);
    assert_eq!(state.outbox.len(), MAX_PUSHES_PER_REQUEST + 10);
    assert_eq!(state.outbox["workspace:000"].payload, json!({"i": 999}));
}

#[test]
fn sync_state_build_request_carries_base_revs() {
    let mut state = SyncState::new();
    state.base.insert("workspace:a".to_string(), 4);
    state.note_local_change("workspace:a", json!({"n": 1}), 10);
    state.note_local_delete("workspace:b", 11);
    let request = state.build_request();
    assert_eq!(request.cursor, 0);
    assert_eq!(request.base.get("workspace:a"), Some(&4));
    let a = request
        .pushes
        .iter()
        .find(|p| p.key == "workspace:a")
        .unwrap();
    assert_eq!(a.base_rev, 4, "基线 rev 随推送上行，乐观并发靠它比对");
    assert!(!a.deleted);
    let b = request
        .pushes
        .iter()
        .find(|p| p.key == "workspace:b")
        .unwrap();
    assert!(b.deleted && b.payload.is_null());
}

#[test]
fn sync_state_apply_response_advances_base_and_acks() {
    let mut state = SyncState::new();
    state.note_local_change("workspace:a", json!({"n": 1}), 10);
    state.note_local_change("workspace:b", json!({"n": 2}), 10);
    let response = SyncResponse {
        cursor: 42,
        accepted: vec![SyncRecord::live("workspace:a", 1, 20, json!({"n": 1}))],
        rejected: vec![SyncRejection {
            key: "workspace:b".into(),
            reason: RejectionReason::Conflict,
            server: Some(SyncRecord::live(
                "workspace:b",
                9,
                5,
                json!({"n": "server"}),
            )),
        }],
        changes: vec![
            SyncRecord::live("workspace:z", 7, 30, json!({"n": 3})),
            SyncRecord::tombstone("workspace:old", 8, 31),
        ],
    };
    let outcome = state.apply_response(&response);
    assert_eq!(state.cursor, 42);
    assert_eq!(state.base.get("workspace:a"), Some(&1));
    assert!(state.base.contains_key("workspace:z"));
    assert_eq!(outcome.acked, vec!["workspace:a"]);
    assert_eq!(outcome.applied.len(), 3, "1 接受 + 2 增量");
    assert_eq!(outcome.conflicts.len(), 1);
    assert_eq!(outcome.conflicts[0].server.as_ref().unwrap().rev, 9);
    assert!(state.outbox.contains_key("workspace:b"), "冲突项留队待裁决");

    // 墓碑落地即从本地视图移除
    let mut view = records(vec![SyncRecord::live(
        "workspace:old",
        8,
        31,
        json!({"n": 0}),
    )]);
    SyncState::apply_to_view(&mut view, &outcome.applied);
    assert!(!view.contains_key("workspace:old"), "墓碑 = 本地删除");
    assert!(view.contains_key("workspace:z"));
}

#[test]
fn sync_state_ignores_cursor_regression() {
    // 服务端重启可能从低水位重发（游标回退）；旧增量不得被当新数据应用
    let mut state = SyncState::new();
    state.cursor = 100;
    let response = SyncResponse {
        cursor: 5,
        accepted: vec![],
        rejected: vec![],
        changes: vec![SyncRecord::live(
            "workspace:a",
            1,
            1,
            json!({"stale": true}),
        )],
    };
    let outcome = state.apply_response(&response);
    assert!(outcome.applied.is_empty());
    assert_eq!(state.cursor, 100);
}

#[test]
fn sync_push_projection_validates_locally() {
    let push = SyncPush {
        key: "workspace:a".into(),
        base_rev: 2,
        deleted: false,
        payload: json!({"n": 1}),
        updated_at_ms: 10,
    };
    assert_eq!(push.validate(), Ok(()));
    assert_eq!(
        push.to_record().rev,
        3,
        "投影 rev = base_rev + 1（乐观占位）"
    );

    let mut bad = push.clone();
    bad.key = "nope".into();
    assert!(matches!(bad.validate(), Err(AccountError::InvalidKey(_))));

    let mut tomb_bad = push.clone();
    tomb_bad.deleted = true;
    assert_eq!(
        tomb_bad.validate(),
        Err(AccountError::TombstonePayloadNotNull)
    );
}

#[test]
fn profile_validation_and_defaults() {
    let profile = AccountProfile::new("alice", "Alice", 100);
    assert_eq!(profile.locale, "zh");
    assert_eq!(profile.rev, 0);
    assert_eq!(profile.validate(), Ok(()));

    let mut p = profile.clone();
    p.display_name = "   ".into();
    assert_eq!(p.validate(), Err(AccountError::EmptyDisplayName));
    p.display_name = "x".repeat(65);
    assert_eq!(p.validate(), Err(AccountError::DisplayNameTooLong));
    p.display_name = "Alice".into();
    p.email = Some("bad-at".into());
    assert_eq!(p.validate(), Err(AccountError::InvalidEmail));
    p.email = Some("a@b.com".into());
    assert_eq!(p.validate(), Ok(()));
    p.email = Some("a@ b.com".into());
    assert_eq!(p.validate(), Err(AccountError::InvalidEmail));
    p.email = None;
    p.locale = "zh_CN".into();
    assert_eq!(p.validate(), Err(AccountError::InvalidLocale));
    p.locale = "en-US".into();
    assert_eq!(p.validate(), Ok(()));
    p.locale = "e".into();
    assert_eq!(p.validate(), Err(AccountError::InvalidLocale));
    p.account_id = "".into();
    assert_eq!(p.validate(), Err(AccountError::InvalidAccountId));
}

#[test]
fn wire_shapes_round_trip_with_server_contract() {
    // wire 契约锁定：snake_case 字段、判别式拒绝原因原名（服务端与
    // 各端客户端共用同一份解析代码，形状变了就是协议破坏）
    let request = SyncRequest {
        cursor: 3,
        base: BTreeMap::from([("workspace:a".to_string(), 2u64)]),
        pushes: vec![SyncPush {
            key: "workspace:a".into(),
            base_rev: 2,
            deleted: false,
            payload: json!({"name": "盯盘"}),
            updated_at_ms: 100,
        }],
    };
    let text = serde_json::to_string(&request).unwrap();
    assert!(text.contains("\"base_rev\":2"), "got {}", text);
    assert_eq!(serde_json::from_str::<SyncRequest>(&text).unwrap(), request);

    let response = SyncResponse {
        cursor: 4,
        accepted: vec![SyncRecord::live(
            "workspace:a",
            3,
            110,
            json!({"name": "盯盘"}),
        )],
        rejected: vec![SyncRejection {
            key: "workspace:b".into(),
            reason: RejectionReason::Conflict,
            server: None,
        }],
        changes: vec![SyncRecord::tombstone("workspace:old", 4, 120)],
    };
    let text = serde_json::to_string(&response).unwrap();
    assert!(text.contains("\"reason\":\"conflict\""), "got {}", text);
    assert!(text.contains("\"deleted\":true"), "got {}", text);
    assert_eq!(
        serde_json::from_str::<SyncResponse>(&text).unwrap(),
        response
    );

    assert_eq!(
        serde_json::to_string(&ConflictPolicy::NewestWins).unwrap(),
        "\"newest_wins\""
    );
}
