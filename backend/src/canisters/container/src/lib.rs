use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_cdk::api::management_canister::main::{
    canister_info, canister_status as ic_canister_status, create_canister,
    install_code as ic_install_code, CanisterIdRecord, CanisterInfoRequest, CanisterInstallMode,
    CanisterSettings, CreateCanisterArgument, InstallCodeArgument,
};
use ic_cdk::{caller, id, println};
use ic_cdk::{init, query, update};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};

pub const DEFAULT_CYCLES: u128 = 4_000_000_000_000;

type Memory = VirtualMemory<DefaultMemoryImpl>;

use std::{borrow::Cow, cell::RefCell};

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum ApiErrorType {
    Unauthorized,
    BadRequest,
    NotFound,
}

#[derive(CandidType, Clone, Deserialize)]
pub struct ApiError {
    err_type: ApiErrorType,
    err_msg: String,
}

pub type Result<T = (), E = ApiError> = std::result::Result<T, E>;

#[must_use]
pub const fn api_error(err_type: ApiErrorType, err_msg: String) -> ApiError {
    ApiError { err_type, err_msg }
}

const MAX_VALUE_SIZE: u32 = 100;
const MAX_KEY_SIZE: u32 = 30;

#[derive(Eq, PartialEq, PartialOrd, Ord, Clone)]
struct Key(String);

impl Storable for Key {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        self.0.to_bytes()
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Self(String::from_bytes(bytes))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: MAX_KEY_SIZE,
        is_fixed_size: false,
    };
}

#[derive(CandidType, Deserialize, Clone, Debug)]
struct SpawnCanister {
    id: Principal,
    hash: Option<Vec<u8>>,
    version: u64,
}

impl Storable for SpawnCanister {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: MAX_VALUE_SIZE,
        is_fixed_size: false,
    };
}

thread_local! {
  static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

  static OWNER: RefCell<StableCell<Key, Memory>> = RefCell::new(
    StableCell::init(
      MEMORY_MANAGER.with_borrow(|this| this.get(MemoryId::new(0))),
      Key(String::new()),
      ).expect("failed ...")
  );

  static CDN_CANISTERS: RefCell<StableBTreeMap<Key, SpawnCanister, Memory>> = RefCell::new(
    StableBTreeMap::    init(
      MEMORY_MANAGER.with_borrow(|this| this.get(MemoryId::new(1)))
    )
  )
}

#[init]
pub fn init() {
    println!("{:?}", "called init");

    OWNER.with_borrow_mut(|owner| {
        owner
            .set(Key(caller().to_string()))
            .expect("Failed to set the owner");
    });
}

const BUCKET_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/canister_cdn_bucket.wasm");

#[update]
async fn upgrade_canister(
    canister_principal: Principal,
    status: Option<CanisterInstallMode>,
) -> Result<(), ApiError> {
    let caller = caller();
    println!("upgrading cdn canister with the id {canister_principal} called by {caller}");

    let owner = OWNER.with_borrow(|ow| ow.get().clone());

    // caller should be owner
    if caller.to_string() != owner.0 {
        return Err(api_error(
            ApiErrorType::Unauthorized,
            format!("principal {caller} not authorized"),
        ));
    }

    let install_status = status.unwrap_or(CanisterInstallMode::Upgrade(None));

    let existing_canister = CDN_CANISTERS
        .with(|p| p.borrow().get(&Key(canister_principal.to_string())))
        .ok_or_else(|| ApiError {
            err_type: ApiErrorType::NotFound,
            err_msg: format!("canister {canister_principal} not found",),
        })?;

    let canister_id = existing_canister.id;

    //
    // START UPGRADE
    //
    println!("upgrading cdn canister with the id {canister_principal}");

    let arg = InstallCodeArgument {
        mode: install_status,
        canister_id,
        wasm_module: BUCKET_WASM.to_vec(),
        arg: vec![],
    };

    ic_install_code(arg).await.map_err(|e| ApiError {
        err_type: ApiErrorType::BadRequest,
        err_msg: e.1,
    })?;

    println!(
        "upgrade successful for canister with the id {}",
        existing_canister.id
    );

    //
    // END UPGRADE
    //

    // canister status
    let c_status = ic_canister_status(CanisterIdRecord { canister_id })
        .await
        .map_err(|_| {
            api_error(
                ApiErrorType::BadRequest,
                String::from("canister status failed..."),
            )
        })?;

    let pd = SpawnCanister {
        id: existing_canister.id,
        hash: c_status.0.module_hash,
        version: existing_canister.version + 1,
    };

    CDN_CANISTERS.with(|pc| {
        pc.borrow_mut()
            .insert(Key(existing_canister.id.to_string()), pd);
        Ok(())
    })
}

#[update]
async fn spawn_bucket() -> Result<SpawnCanister, ApiError> {
    let caller = caller();
    let owner = OWNER.with(|ow| ow.borrow().get().clone());

    if caller.to_string() != owner.0 {
        return Err(api_error(ApiErrorType::Unauthorized, caller.to_string()));
    }
    let owner_principal = owner.0;
    println!("{owner_principal:?}");
    let canister_settings = CreateCanisterArgument {
        settings: Some(CanisterSettings {
            controllers: Some(vec![caller, id()]),
            ..Default::default()
        }),
    };
    let new_canister = create_canister(canister_settings, DEFAULT_CYCLES).await;
    let canister = new_canister.map_err(|_| {
        api_error(
            ApiErrorType::BadRequest,
            String::from("canister creation failed..."),
        )
    })?;

    println!("{:?}", "works1");
    let new_canister_principal = canister.0.canister_id;
    println!("{:?}", new_canister_principal.to_string());
    let arg = InstallCodeArgument {
        mode: CanisterInstallMode::Install,
        canister_id: new_canister_principal,
        wasm_module: BUCKET_WASM.to_vec(),
        arg: vec![],
    };

    ic_install_code(arg).await.map_err(|_| {
        api_error(
            ApiErrorType::BadRequest,
            String::from("canister installation failed..."),
        )
    })?;

    println!("{:?}", "works2");

    let c_status = ic_canister_status(CanisterIdRecord {
        canister_id: new_canister_principal,
    })
    .await
    .map_err(|_| {
        api_error(
            ApiErrorType::BadRequest,
            String::from("canister status failed..."),
        )
    })?;

    println!("{:?}", "works3");
    let sc = SpawnCanister {
        id: new_canister_principal,
        hash: c_status.0.module_hash,
        version: 1,
    };
    println!("{:?}", "works1");
    println!("{:?}", sc);
    CDN_CANISTERS.with(|cc| {
        cc.borrow_mut()
            .insert(Key(new_canister_principal.to_string()), sc.clone())
    });

    Ok(sc)
}

//
// list_buckets
// CDN
//

#[query]
fn list_buckets() -> Vec<SpawnCanister> {
    CDN_CANISTERS.with(|cc| {
        let mut vs: Vec<SpawnCanister> = vec![];
        for (_k, v) in cc.borrow().iter() {
            vs.push(v.clone());
        }
        vs
    })
}

#[update]
async fn get_controllers(cid: Principal) -> Vec<Principal> {
    let canister_info = canister_info(CanisterInfoRequest {
        canister_id: cid,
        num_requested_changes: None,
    })
    .await
    .unwrap();

    canister_info.0.controllers
}

#[query]
#[allow(clippy::unnecessary_wraps)]
fn test_cdn() -> Result<String, ApiError> {
    let network = option_env!("DFX_NETWORK");
    println!("{network:?}");
    Ok(network.unwrap().to_string())
    // let clr = caller();
    // println!("{clr}");

    // CDN_CANISTERS
    //     .with(|p| {
    //         p.borrow()
    //             .get(&Key("5ezkp-5yaaa-aaaae-aajeq-cai".to_string()))
    //     })
    //     .ok_or_else(|| ApiError {
    //         err_type: ApiErrorType::NotFound,
    //         err_msg: clr.to_string(),
    //     })
}

// #[update]
// fn manual_add(id: Principal) -> String {
//     let sc = SpawnCanister {
//         id,
//         hash: None,
//         version: 1,
//     };

//     CDN_CANISTERS.with(|cc| cc.borrow_mut().insert(Key(id.to_string()), sc));

//     String::from("done")
// }

// #[query]
// fn cycles_balance() -> u128 {
//     let _caller = caller();
//     let _user: Principal = STATE.with(|state| state.borrow().stable.user).unwrap();

//     // if !is_manager(caller) && principal_not_equal(caller, user) {
//     //     trap("No permission to read the balance of the cycles.");
//     // }

//     canister_balance128()
// }

ic_cdk::export_candid!();
