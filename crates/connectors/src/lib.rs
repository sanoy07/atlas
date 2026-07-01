use anyhow::Result;
use atlas_ir::EntityKind;

/// What capability a connector provides, described in domain terms.
///
/// A capability is independent of the implementation:
/// "Repository History" is provided by Git today, but could be
/// provided by something else tomorrow. The capability stays;
/// the implementation is swappable.
pub struct Capability {
    pub name:     &'static str,
    pub produces: Vec<EntityKind>,
}

/// Raw output from a connector — opaque text for the parser to consume.
/// The connector does not parse. The parser does not fetch. These are separate concerns.
pub struct RawPayload {
    pub data: String,
}

/// The lifecycle every Atlas data source must implement.
///
/// Responsibilities:
///   - Identify itself (name, capability)
///   - Fetch raw data from its source
///
/// Explicitly NOT responsible for:
///   - Parsing the data into IR types  (→ atlas-parser)
///   - Storing anything               (→ atlas-storage)
///   - Knowing about other connectors (→ atlas-core)
pub trait Connector {
    fn name(&self) -> &str;
    fn capability(&self) -> Capability;
    fn fetch_raw(&self) -> Result<RawPayload>;
}
