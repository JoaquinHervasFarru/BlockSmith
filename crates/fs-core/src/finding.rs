use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Cosmetic,
    Minor,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectType {
    Node,
    Folder,
    File,
    Extent,
    Bitmap,
    Header,
    Encryption,
}

/// Problema detectado durante un escaneo. `code` usa prefijos por formato:
/// `HFS_*`, `APFS_*`, `NTFS_*`, `EXT4_*`, `CRYPT_*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub object_type: ObjectType,
    pub object_id: Option<u64>,
    pub node_number: Option<u64>,
    pub path: Option<String>,
    pub message: String,
    #[serde(default)]
    pub details: serde_json::Value,
}
