use ic_cdk::api::time;
use sha2::{Digest, Sha256};
use std::fmt;

use crate::types::assets::AssetHashes;
use crate::types::state::Assets;
use crate::types::store::{Asset, AssetEncoding};

pub static ASSET_ENCODING_KEY_RAW: &str = "raw";

impl From<&Assets> for AssetHashes {
    fn from(assets: &Assets) -> Self {
        let mut asset_hashes = Self::default();

        for asset in assets.values() {
            asset_hashes.insert(asset);
        }

        asset_hashes
    }
}

impl AssetHashes {
    pub(crate) fn insert(&mut self, asset: &Asset) {
        self.tree
            .insert(asset.key.full_path.clone(), asset.encoding_raw().sha256);
    }

    pub(crate) fn delete(&mut self, full_path: &String) {
        self.tree.delete(full_path.as_bytes());
    }
}

#[derive(Debug)]
pub struct AssetEncodingError {
    description: String,
}

impl std::error::Error for AssetEncodingError {}

impl fmt::Display for AssetEncodingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AssetEncodingError: {}", self.description)
    }
}

impl TryFrom<&Vec<Vec<u8>>> for AssetEncoding {
    type Error = AssetEncodingError;

    fn try_from(content_chunks: &Vec<Vec<u8>>) -> Result<Self, Self::Error> {
        let mut total_length: u128 = 0;
        let mut hasher = Sha256::new();

        for chunk in content_chunks {
            match u128::try_from(chunk.len()) {
                Ok(len) => total_length += len,
                Err(_) => {
                    return Err(AssetEncodingError {
                        description: "Failed to convert chunk length to u128".to_string(),
                    })
                }
            }

            hasher.update(chunk);
        }

        let sha256 = hasher.finalize().into();

        Ok(Self {
            modified: time(), // Replace with the actual function that returns time
            content_chunks: content_chunks.clone(),
            total_length,
            sha256,
        })
    }
}
impl Asset {
    pub(crate) fn encoding_raw(&self) -> &AssetEncoding {
        // We only use raw at the moment and it cannot be None
        self.encodings.get(ASSET_ENCODING_KEY_RAW).unwrap()
    }
}
