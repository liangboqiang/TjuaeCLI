/// JSON stream protocol version negotiated by the `ready` event.
///
/// This is intentionally independent from the TjuaeCLI package version.
pub const JSON_STREAM_PROTOCOL_VERSION: &str = "0.2.0";

#[cfg(test)]
#[path = "version_test.rs"]
mod version_test;
