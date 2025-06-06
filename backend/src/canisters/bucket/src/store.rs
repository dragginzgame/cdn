use ic_cdk::{api::time, println};
use std::collections::HashMap;

use crate::cert::update_certified_data;
use crate::impls::ASSET_ENCODING_KEY_RAW;
use crate::types::interface::{CommitBatch, Del};
use crate::types::state::{RuntimeState, StableState, State};
use crate::types::store::{Asset, AssetEncoding, AssetKey, Batch, Chunk};
use crate::STATE;

//
// Getter, list and delete
//

pub fn get_asset_for_url(url: &str) -> Result<Asset, &'static str> {
    if url.is_empty() {
        return Err("No url provided.");
    }

    let split: Vec<&str> = url.split("?token=").collect();
    let full_path: &str = &["/", split[0].trim_start_matches('/')].join("");

    // Token protected assets
    if split.len() > 1 {
        let token = split[1];
        return get_asset(full_path, Some(token.to_string()));
    }

    // Map /index.html to / because we are using / as root
    if full_path == "/index.html" {
        return get_asset("/", None);
    };

    get_asset(full_path, None)
}

pub fn get_asset(full_path: &str, token: Option<String>) -> Result<Asset, &'static str> {
    STATE.with(|state| get_asset_impl(full_path, token, &state.borrow().stable))
}

pub fn delete_asset(param: Del) -> Result<Asset, &'static str> {
    STATE.with(|state| delete_asset_impl(param, &mut state.borrow_mut()))
}

pub fn get_keys(folder: Option<String>) -> Vec<AssetKey> {
    STATE.with(|state| get_keys_impl(folder, &state.borrow().stable))
}

pub fn get_len() -> usize {
    println!("{:?}", "len of all cdn files...");
    STATE.with(|state| state.borrow().stable.assets.len())
}

fn get_asset_impl(
    full_path: &str,
    token: Option<String>,
    state: &StableState,
) -> Result<Asset, &'static str> {
    let asset = state.assets.get(full_path);

    match asset {
        None => Err("No asset."),
        Some(asset) => match &asset.key.id {
            None => Ok(asset.clone()),
            Some(asset_token) => get_protected_asset(asset, asset_token, token),
        },
    }
}

fn get_protected_asset(
    asset: &Asset,
    asset_token: &String,
    token: Option<String>,
) -> Result<Asset, &'static str> {
    match token {
        None => Err("No token provided."),
        Some(token) => {
            if &token == asset_token {
                return Ok(asset.clone());
            }

            Err("Invalid token.")
        }
    }
}

fn get_keys_impl(folder: Option<String>, state: &StableState) -> Vec<AssetKey> {
    fn map_key(asset: &Asset) -> AssetKey {
        asset.key.clone()
    }

    let all_keys: Vec<AssetKey> = state.assets.values().map(map_key).collect();

    match folder {
        Some(folder) => all_keys
            .into_iter()
            .filter(|key| key.folder == folder)
            .collect(),
        None => all_keys,
    }
}

fn delete_asset_impl(
    Del { full_path, token }: Del,
    state: &mut State,
) -> Result<Asset, &'static str> {
    let result = get_asset_impl(&full_path, token, &state.stable);

    match result {
        Err(err) => Err(err),
        Ok(asset) => {
            state.stable.assets.remove(&*full_path);
            delete_certified_asset(state, &full_path);
            Ok(asset)
        }
    }
}

//
// Upload batch and chunks
//

const BATCH_EXPIRY_NANOS: u64 = 300_000_000_000;

static mut NEXT_BACK_ID: u128 = 0;
static mut NEXT_CHUNK_ID: u128 = 0;

pub fn create_batch(key: AssetKey) -> u128 {
    STATE.with(|state| create_batch_impl(key, &mut state.borrow_mut().runtime))
}

pub fn create_chunk(chunk: Chunk) -> Result<u128, &'static str> {
    STATE.with(|state| create_chunk_impl(chunk, &mut state.borrow_mut().runtime))
}

pub fn commit_batch(commit_batch: CommitBatch) -> Result<&'static str, &'static str> {
    STATE.with(|state| commit_batch_impl(commit_batch, &mut state.borrow_mut()))
}

fn create_batch_impl(key: AssetKey, state: &mut RuntimeState) -> u128 {
    let now = time();
    println!("{key:?}");
    unsafe {
        clear_expired_batches(state);

        NEXT_BACK_ID += 1;

        state.batches.insert(
            NEXT_BACK_ID,
            Batch {
                key,
                expires_at: now + BATCH_EXPIRY_NANOS,
            },
        );

        NEXT_BACK_ID
    }
}

fn create_chunk_impl(
    Chunk { batch_id, content }: Chunk,
    state: &mut RuntimeState,
) -> Result<u128, &'static str> {
    let batch = state.batches.get(&batch_id);

    match batch {
        None => Err("Batch not found."),
        Some(b) => {
            let now = time();

            state.batches.insert(
                batch_id,
                Batch {
                    key: b.key.clone(),
                    expires_at: now + BATCH_EXPIRY_NANOS,
                },
            );

            unsafe {
                NEXT_CHUNK_ID += 1;

                state
                    .chunks
                    .insert(NEXT_CHUNK_ID, Chunk { batch_id, content });

                Ok(NEXT_CHUNK_ID)
            }
        }
    }
}

fn commit_batch_impl(
    commit_batch: CommitBatch,
    state: &mut State,
) -> Result<&'static str, &'static str> {
    let batches = state.runtime.batches.clone();
    let batch = batches.get(&commit_batch.batch_id);

    match batch {
        None => Err("No batch to commit."),
        Some(b) => {
            let asset = commit_chunks(commit_batch, b, state);
            match asset {
                Err(err) => Err(err),
                Ok(asset) => {
                    update_certified_asset(state, &asset);
                    Ok("Batch committed and certified assets updated.")
                }
            }
        }
    }
}

fn commit_chunks(
    CommitBatch {
        chunk_ids,
        batch_id,
        headers,
    }: CommitBatch,
    batch: &Batch,
    state: &mut State,
) -> Result<Asset, &'static str> {
    let now = time();

    if now > batch.expires_at {
        clear_expired_batches(&mut state.runtime);
        return Err("Batch did not complete in time. Chunks cannot be committed.");
    }

    let mut content_chunks: Vec<Vec<u8>> = vec![];

    for chunk_id in &chunk_ids {
        let chunk = state.runtime.chunks.get(chunk_id);

        match chunk {
            None => {
                return Err("Chunk does not exist.");
            }
            Some(c) => {
                if batch_id != c.batch_id {
                    return Err("Chunk not included in the provided batch.");
                }

                content_chunks.push(c.clone().content);
            }
        }
    }

    if content_chunks.is_empty() {
        return Err("No chunk to commit.");
    }

    let key = batch.clone().key;

    // We only use raw at the moment
    let mut encodings = HashMap::new();
    encodings.insert(
        ASSET_ENCODING_KEY_RAW.to_string(),
        AssetEncoding::try_from(&content_chunks).unwrap(),
    );

    let asset: Asset = Asset {
        key,
        headers,
        encodings,
    };

    state
        .stable
        .assets
        .insert(batch.clone().key.full_path, asset.clone());

    clear_batch(batch_id, &chunk_ids, &mut state.runtime);

    Ok(asset)
}

fn clear_expired_batches(state: &mut RuntimeState) {
    let now = time();

    // Remove expired batches

    let batches = state.batches.clone();

    for (batch_id, batch) in &batches {
        if now > batch.expires_at {
            state.batches.remove(batch_id);
        }
    }

    // Remove chunk without existing batches (those we just deleted above)

    let chunks = state.chunks.clone();

    for (chunk_id, chunk) in &chunks {
        if !state.batches.contains_key(&chunk.batch_id) {
            state.chunks.remove(chunk_id);
        }
    }
}

fn clear_batch(batch_id: u128, chunk_ids: &[u128], state: &mut RuntimeState) {
    for chunk_id in chunk_ids {
        state.chunks.remove(chunk_id);
    }

    state.batches.remove(&batch_id);
}

fn update_certified_asset(state: &mut State, asset: &Asset) {
    // 1. Replace or insert the new asset in tree
    state.runtime.asset_hashes.insert(asset);

    // 2. Update the root hash and the canister certified data
    update_certified_data(&state.runtime.asset_hashes);
}

fn delete_certified_asset(state: &mut State, full_path: &String) {
    // 1. Remove the asset in tree
    state.runtime.asset_hashes.delete(full_path);

    // 2. Update the root hash and the canister certified data
    update_certified_data(&state.runtime.asset_hashes);
}
