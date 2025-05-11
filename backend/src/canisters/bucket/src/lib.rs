mod cert;
mod http;
mod impls;
mod store;
mod types;

use crate::cert::update_certified_data;
use crate::http::{build_headers, create_token, streaming_strategy};
use crate::types::assets::AssetHashes;
use crate::types::http::{
    HttpRequest, HttpResponse, StreamingCallbackHttpResponse, StreamingCallbackToken,
};
use crate::types::interface::{CommitBatch, Del, InitUpload, UploadChunk};
use crate::types::state::{RuntimeState, StableState, State};
use crate::types::store::{AssetKey, Chunk};
use candid::Principal;
use ic_cdk::api::{canister_cycle_balance, msg_caller, trap};
use ic_cdk::export_candid;
use ic_cdk::storage::{stable_restore, stable_save};
use ic_cdk::{init, post_upgrade, pre_upgrade, query, update};
use std::{cell::RefCell, collections::HashMap};
use store::{get_asset, get_asset_for_url, get_len};
use types::store::Asset;

use crate::store::{commit_batch, create_batch, create_chunk, delete_asset, get_keys};

thread_local! {
  static STATE: RefCell<State> = RefCell::default();
}

#[init]
fn init() {
    STATE.with(|state| {
        *state.borrow_mut() = State {
            stable: StableState { user: None, assets: HashMap::new() },
            runtime: RuntimeState {
                chunks: HashMap::new(),
                batches: HashMap::new(),
                asset_hashes: AssetHashes::default(),
            },
        };
    });
}

#[pre_upgrade]
fn pre_upgrade() {
    STATE.with(|state| stable_save((&state.borrow().stable,)).unwrap());
}

#[post_upgrade]
fn post_upgrade() {
    let (stable,): (StableState,) = stable_restore().unwrap();

    let asset_hashes = AssetHashes::from(&stable.assets);

    // Populate state
    STATE.with(|state| {
        *state.borrow_mut() = State {
            stable,
            runtime: RuntimeState {
                chunks: HashMap::new(),
                batches: HashMap::new(),
                asset_hashes: asset_hashes.clone(),
            },
        }
    });

    update_certified_data(&asset_hashes);
}

//
// Http
//

#[query]
fn http_request(HttpRequest { method, url, .. }: HttpRequest) -> HttpResponse {
    if method != "GET" {
        return HttpResponse {
            body: b"Method Not Allowed".to_vec(),
            headers: Vec::new(),
            status_code: 405,
            streaming_strategy: None,
        };
    }

    let result = get_asset_for_url(&url);

    match result {
        Ok(asset) => {
            let headers = build_headers(&asset);

            let encoding = asset.encoding_raw();
            let Asset { key, .. } = &asset;

            match headers {
                Ok(headers) => HttpResponse {
                    body: encoding.content_chunks[0].clone(),
                    headers: headers.clone(),
                    status_code: 200,
                    streaming_strategy: streaming_strategy(key, encoding, &headers),
                },
                Err(err) => HttpResponse {
                    body: ["Permission denied. Invalid headers. ", err]
                        .join("")
                        .as_bytes()
                        .to_vec(),
                    headers: Vec::new(),
                    status_code: 405,
                    streaming_strategy: None,
                },
            }
        }
        Err(err) => HttpResponse {
            body: ["Permission denied. Could not perform this operation. ", err]
                .join("")
                .as_bytes()
                .to_vec(),
            headers: Vec::new(),
            status_code: 405,
            streaming_strategy: None,
        },
    }
}

#[query]
fn http_request_streaming_callback(
    StreamingCallbackToken { token, headers, index, full_path, .. }: StreamingCallbackToken,
) -> StreamingCallbackHttpResponse {
    let result = get_asset(&full_path, token);

    match result {
        Err(err) => trap(["Streamed asset not found: ", err].join("")),
        Ok(asset) => {
            let encoding = asset.encoding_raw();

            StreamingCallbackHttpResponse {
                token: create_token(&asset.key, index, encoding, &headers),
                body: encoding.content_chunks[index].clone(),
            }
        }
    }
}

#[update]
fn init_upload(key: AssetKey) -> InitUpload {
    println!("{:?}", "upload starts...");
    // let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

    // if principal_not_equal(caller(), user) {
    //     trap("User does not have the permission to upload data.");
    // }

    let batch_id = create_batch(key);
    println!("{batch_id:?}");
    InitUpload { batch_id }
}

#[update]
fn upload_chunk(chunk: Chunk) -> UploadChunk {
    println!("{:?}", "chunks upload...");
    // let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

    // if principal_not_equal(caller(), user) {
    //     trap("User does not have the permission to a upload any chunks of content.");
    // }

    let result = create_chunk(chunk);

    match result {
        Ok(chunk_id) => UploadChunk { chunk_id },
        Err(error) => trap(error),
    }
}

#[update]
fn commit_upload(commit: CommitBatch) {
    println!("{:?}", "commit upload...");
    // let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

    // if principal_not_equal(caller(), user) {
    //     trap("User does not have the permission to commit an upload.");
    // }

    let result = commit_batch(commit);
    println!("{result:?}");
    match result {
        Ok(_) => (),
        Err(error) => trap(error),
    }
}

#[query]
fn list(folder: Option<String>) -> Vec<AssetKey> {
    // let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

    // if principal_not_equal(caller(), user) {
    //     trap("User does not have the permission to list the assets.");
    // }

    get_keys(folder)
}

#[query]
fn len() -> usize {
    get_len()
}

#[query]
const fn test() -> u8 {
    2
}

#[update]
fn del(param: Del) {
    let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

    // if principal_not_equal(caller(), user) {
    //     trap("User does not have the permission to delete an asset.");
    // }

    let result = delete_asset(param);

    match result {
        Ok(_) => (),
        Err(error) => trap(["Asset cannot be deleted: ", error].join("")),
    }
}

#[query]
fn cycles_balance() -> u128 {
    let _caller = msg_caller();
    let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

    // if !is_manager(caller) && principal_not_equal(caller, user) {
    //     trap("No permission to read the balance of the cycles.");
    // }

    canister_cycle_balance()
}

export_candid!();
