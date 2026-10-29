//! TLS trust helpers for iroh endpoint builders.
//!
//! OS trust is applied ONLY for user-configured self-hosted HTTPS infra
//! (custom relay / custom pkarr). n0's public infra is covered by iroh's
//! embedded Mozilla roots.

use iroh::endpoint::Builder;
use iroh::tls::CaTlsConfig;

use crate::types::{DiscoveryModeOption, RelayModeOption};

/// True when the user configured self-hosted discovery and/or relay HTTPS.
pub fn uses_custom_infra(discovery: &DiscoveryModeOption, relay: &RelayModeOption) -> bool {
    matches!(discovery, DiscoveryModeOption::Custom { .. })
        || matches!(relay, RelayModeOption::Custom { .. })
}

/// Apply OS CA trust only when `custom_infra` is set; otherwise keep iroh's
/// embedded Mozilla roots.
pub fn with_system_ca_if_custom(builder: Builder, custom_infra: bool) -> Builder {
    if custom_infra {
        builder.ca_tls_config(CaTlsConfig::system())
    } else {
        builder
    }
}
