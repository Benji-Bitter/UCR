//! Public channel definitions.

use alloc::string::String;
use serde::{Deserialize, Serialize};

use crate::header::Scope;
use crate::ids::ChannelId;

/// Channel category — used for directory grouping in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelCategory {
    General,
    Local,
    Marine,
    RoadTransport,
    Community,
    Technical,
    EmergencyInfo,
    Custom,
}

impl ChannelCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Local => "Local",
            Self::Marine => "Marine",
            Self::RoadTransport => "Road & Transport",
            Self::Community => "Community",
            Self::Technical => "Technical",
            Self::EmergencyInfo => "Emergency Information",
            Self::Custom => "Custom",
        }
    }
}

/// A public channel.
///
/// Channels have no central registry: the `ChannelId` is derived from the
/// (lowercased) name, so anyone typing the same name joins the same channel.
/// `scope` caps how far channel traffic floods across the mesh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub id: ChannelId,
    pub name: String,
    pub category: ChannelCategory,
    /// Maximum flood distance for messages on this channel.
    pub scope: Scope,
    /// Whether the channel was locally created/favourited rather than
    /// discovered over the air.
    pub user_created: bool,
}

impl ChannelInfo {
    pub fn new(name: &str, category: ChannelCategory, scope: Scope) -> Self {
        Self {
            id: ChannelId::from_name(name),
            name: name.into(),
            category,
            scope,
            user_created: false,
        }
    }
}

/// The built-in example channel set. Users can create additional channels;
/// this list only seeds a fresh node's directory.
pub fn default_channels() -> alloc::vec::Vec<ChannelInfo> {
    alloc::vec![
        ChannelInfo::new("ucr-general", ChannelCategory::General, Scope::Regional),
        ChannelInfo::new("ucr-local", ChannelCategory::Local, Scope::Local),
        ChannelInfo::new("marine", ChannelCategory::Marine, Scope::Regional),
        ChannelInfo::new("roadwatch", ChannelCategory::RoadTransport, Scope::Regional),
        ChannelInfo::new("community", ChannelCategory::Community, Scope::Local),
        ChannelInfo::new("tech-talk", ChannelCategory::Technical, Scope::Extended),
        ChannelInfo::new(
            "emergency-info",
            ChannelCategory::EmergencyInfo,
            Scope::Extended
        ),
    ]
}
