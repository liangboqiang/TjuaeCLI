use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Provider-neutral reference to the exact asset definition used at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeAssetRef {
    pub local_asset_id: String,
    pub kind: String,
    pub local_definition_digest: String,
    pub runtime_content_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_revision: Option<String>,
}

/// Process-local input that binds a managed skill asset to its materialized root.
///
/// This type deliberately does not implement `Serialize`. The root is a local
/// implementation detail and must never be included in protocol or trace data.
#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeSkillRef {
    pub asset: RuntimeAssetRef,
    pub root: PathBuf,
}

/// Process-local binding between an MCP asset and the configured server name.
///
/// The server name is used only to match the receipt against an MCP connection
/// that completed initialize and tool discovery. It is deliberately excluded
/// from the serialized receipt.
#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeMcpRef {
    pub asset: RuntimeAssetRef,
    pub server_name: String,
}

impl fmt::Debug for RuntimeMcpRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeMcpRef")
            .field("asset", &self.asset)
            .field("server_name", &self.server_name)
            .finish()
    }
}

impl fmt::Debug for RuntimeSkillRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeSkillRef")
            .field("asset", &self.asset)
            .field("root", &"<redacted>")
            .finish()
    }
}

/// Receipt for the exact asset definitions that were accepted by bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeAssetSnapshot {
    pub runtime_snapshot_id: String,
    pub assets: Vec<RuntimeAssetRef>,
}

/// Version of the process-local runtime-boundary record contract.
///
/// Boundary records are deliberately smaller than logs: they contain only a
/// lifecycle phase, outcome, timestamps, a safe asset identity, and a stable
/// error code. Commands, paths, environment variables, credentials, protocol
/// payloads, and human-readable error messages are not representable here.
pub const RUNTIME_BOUNDARY_RECORD_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeBoundaryPhase {
    Resolve,
    Project,
    Spawn,
    Handshake,
    Inject,
    Connect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeBoundaryStatus {
    Succeeded,
    Failed,
}

/// One event emitted at the operation that can actually prove the boundary.
///
/// This is not a receipt and must never be reconstructed from a final receipt.
/// A failed operation carries a stable code; a successful one never does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeBoundaryRecord {
    pub version: u32,
    pub phase: RuntimeBoundaryPhase,
    pub status: RuntimeBoundaryStatus,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl RuntimeBoundaryRecord {
    pub fn succeeded(
        phase: RuntimeBoundaryPhase,
        started_at_ms: i64,
        ended_at_ms: i64,
        asset: Option<&RuntimeAssetRef>,
    ) -> Self {
        Self::new(
            phase,
            RuntimeBoundaryStatus::Succeeded,
            started_at_ms,
            ended_at_ms,
            asset,
            None,
        )
    }

    pub fn failed(
        phase: RuntimeBoundaryPhase,
        started_at_ms: i64,
        ended_at_ms: i64,
        asset: Option<&RuntimeAssetRef>,
        error_code: impl Into<String>,
    ) -> Self {
        Self::new(
            phase,
            RuntimeBoundaryStatus::Failed,
            started_at_ms,
            ended_at_ms,
            asset,
            Some(error_code.into()),
        )
    }

    fn new(
        phase: RuntimeBoundaryPhase,
        status: RuntimeBoundaryStatus,
        started_at_ms: i64,
        ended_at_ms: i64,
        asset: Option<&RuntimeAssetRef>,
        error_code: Option<String>,
    ) -> Self {
        Self {
            version: RUNTIME_BOUNDARY_RECORD_VERSION,
            phase,
            status,
            started_at_ms,
            ended_at_ms: ended_at_ms.max(started_at_ms),
            asset_kind: asset.map(|asset| asset.kind.clone()),
            local_asset_id: asset.map(|asset| asset.local_asset_id.clone()),
            error_code,
        }
    }
}

#[cfg(test)]
#[path = "runtime_asset_test.rs"]
mod runtime_asset_test;
