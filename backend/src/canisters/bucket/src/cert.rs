use base64::{engine::general_purpose, Engine};
use ic_cdk::api::{certified_data_set, data_certificate};
use ic_certified_map::{labeled, labeled_hash, AsHashTree};

use crate::types::assets::AssetHashes;
use crate::types::http::HeaderField;

const LABEL_ASSETS: &[u8] = b"http_assets";

pub fn update_certified_data(asset_hashes: &AssetHashes) {
    let prefixed_root_hash = &labeled_hash(LABEL_ASSETS, &asset_hashes.tree.root_hash());
    certified_data_set(&prefixed_root_hash[..]);
}

pub fn build_asset_certificate_header(
    asset_hashes: &AssetHashes,
    url: &str,
) -> Result<HeaderField, &'static str> {
    let certificate = data_certificate();

    match certificate {
        None => Err("No certificate found."),
        Some(certificate) => build_asset_certificate_header_impl(&certificate, asset_hashes, url),
    }
}

fn build_asset_certificate_header_impl(
    certificate: &Vec<u8>,
    asset_hashes: &AssetHashes,
    url: &str,
) -> Result<HeaderField, &'static str> {
    let witness = asset_hashes.tree.witness(url.as_bytes());
    let tree = labeled(LABEL_ASSETS, witness);

    //
    // @Gabriel
    // I changed this to use ciborium as its maintained and doesn't give us
    // cargo audit issues
    //

    let mut writer = Vec::<u8>::new();

    ciborium::ser::into_writer(&tree, &mut writer).map_err(|_| "failed to serialize hash tree")?;

    Ok(HeaderField(
        "IC-Certificate".to_string(),
        format!(
            "certificate=:{}:, tree=:{}:",
            general_purpose::STANDARD_NO_PAD.encode(certificate),
            general_purpose::STANDARD_NO_PAD.encode(writer)
        ),
    ))
}
