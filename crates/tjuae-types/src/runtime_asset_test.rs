use std::path::PathBuf;

use serde_json::json;

use super::{
    RUNTIME_BOUNDARY_RECORD_VERSION, RuntimeAssetRef, RuntimeAssetSnapshot, RuntimeBoundaryPhase,
    RuntimeBoundaryRecord, RuntimeBoundaryStatus, RuntimeMcpRef, RuntimeSkillRef,
};

fn asset_ref() -> RuntimeAssetRef {
    RuntimeAssetRef {
        local_asset_id: "local-skill-1".to_string(),
        kind: "skill".to_string(),
        local_definition_digest: "sha256-deadbeef".to_string(),
        runtime_content_digest: "sha256-cafebabe".to_string(),
        upstream_package: Some("official-skills".to_string()),
        upstream_asset_id: Some("review".to_string()),
        upstream_version: Some("1.2.3".to_string()),
        upstream_revision: Some("abc123".to_string()),
    }
}

#[test]
fn runtime_snapshot_uses_frozen_camel_case_contract() {
    let snapshot = RuntimeAssetSnapshot {
        runtime_snapshot_id: "snapshot-1".to_string(),
        assets: vec![asset_ref()],
    };

    let value = serde_json::to_value(snapshot).expect("runtime snapshot should serialize");

    assert_eq!(value["runtimeSnapshotId"], "snapshot-1");
    assert_eq!(value["assets"][0]["localAssetId"], "local-skill-1");
    assert_eq!(value["assets"][0]["localDefinitionDigest"], "sha256-deadbeef");
    assert_eq!(value["assets"][0]["runtimeContentDigest"], "sha256-cafebabe");
    assert_eq!(value["assets"][0]["upstreamPackage"], "official-skills");
    assert_eq!(value["assets"][0]["upstreamAssetId"], "review");
    assert_eq!(value["assets"][0]["upstreamVersion"], "1.2.3");
    assert_eq!(value["assets"][0]["upstreamRevision"], "abc123");
}

#[test]
fn v2_fixture_round_trips_through_the_public_contract() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/runtime-asset-snapshot.v2.json"
    ));
    let snapshot: RuntimeAssetSnapshot = serde_json::from_str(fixture).expect("v2 fixture should deserialize");

    assert_eq!(
        snapshot.runtime_snapshot_id,
        "sha256-b4bd3526544e3fee922ee78bc47887ad1df2e47f3f81a32cb88031686ea8d462"
    );
    assert_eq!(snapshot.assets.len(), 4);
    assert_eq!(
        snapshot
            .assets
            .iter()
            .map(|asset| asset.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["assistant", "engineAdapter", "mcp", "skill"]
    );
    assert_eq!(
        serde_json::to_value(snapshot).expect("v2 fixture should serialize"),
        serde_json::from_str::<serde_json::Value>(fixture).expect("fixture should be valid JSON")
    );
}

#[test]
fn v2_contract_rejects_unknown_fields() {
    let value = json!({
        "runtimeSnapshotId": "snapshot-1",
        "assets": [],
        "localRoot": "must-never-cross-the-contract"
    });

    assert!(serde_json::from_value::<RuntimeAssetSnapshot>(value).is_err());
}

#[test]
fn absent_upstream_fields_are_omitted() {
    let mut asset = asset_ref();
    asset.upstream_package = None;
    asset.upstream_asset_id = None;
    asset.upstream_version = None;
    asset.upstream_revision = None;

    let value = serde_json::to_value(asset).expect("runtime asset should serialize");

    assert_eq!(
        value,
        json!({
            "localAssetId": "local-skill-1",
            "kind": "skill",
            "localDefinitionDigest": "sha256-deadbeef",
            "runtimeContentDigest": "sha256-cafebabe"
        })
    );
}

#[test]
fn managed_skill_debug_output_redacts_local_root() {
    let secret_root = PathBuf::from("private").join("workspace").join("managed-skill");
    let managed = RuntimeSkillRef {
        asset: asset_ref(),
        root: secret_root.clone(),
    };

    let debug = format!("{managed:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(&secret_root.to_string_lossy().to_string()));
}

#[test]
fn mcp_process_binding_does_not_change_the_serialized_receipt() {
    let binding = RuntimeMcpRef {
        asset: asset_ref(),
        server_name: "docs".to_owned(),
    };

    let value = serde_json::to_value(&binding.asset).expect("asset should serialize");

    assert!(value.get("serverName").is_none());
    assert!(!value.to_string().contains("docs"));
}

#[test]
fn runtime_boundary_record_has_a_closed_safe_contract() {
    let asset = asset_ref();
    let record = RuntimeBoundaryRecord::failed(
        RuntimeBoundaryPhase::Connect,
        10,
        12,
        Some(&asset),
        "TJUAE_RUNTIME_MCP_CONNECT_FAILED",
    );

    assert_eq!(record.version, RUNTIME_BOUNDARY_RECORD_VERSION);
    assert_eq!(record.status, RuntimeBoundaryStatus::Failed);
    assert_eq!(record.asset_kind.as_deref(), Some("skill"));
    assert_eq!(record.local_asset_id.as_deref(), Some("local-skill-1"));
    let value = serde_json::to_value(record).expect("runtime boundary should serialize");
    assert_eq!(
        value,
        json!({
            "version": RUNTIME_BOUNDARY_RECORD_VERSION,
            "phase": "connect",
            "status": "failed",
            "startedAtMs": 10,
            "endedAtMs": 12,
            "assetKind": "skill",
            "localAssetId": "local-skill-1",
            "errorCode": "TJUAE_RUNTIME_MCP_CONNECT_FAILED"
        })
    );
    let serialized = value.to_string();
    assert!(!serialized.contains("private"));
    assert!(!serialized.contains("environment"));
    assert!(!serialized.contains("credential"));
}
