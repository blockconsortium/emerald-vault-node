#[macro_use]
extern crate neon;
extern crate emerald_rs;
extern crate uuid;
extern crate hex;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

mod accounts;
mod access;
mod js;

use neon::prelude::*;
use accounts::*;
use access::{VaultConfig, Vault};
use emerald_rs::{
    Address,
    Transaction,
    rpc::common::{
        SignTxTransaction
    },
    storage::{
        KeyfileStorage, StorageController, default_path,
        keyfile::{KeystoreError}
    },
    keystore::{
        KeyFile, CryptoType, KdfDepthLevel, Kdf, os_random
    },
    util::{
        ToHex
    },
    mnemonic::{
        Mnemonic, HDPath, Language, generate_key
    },
    rpc::common::NewMnemonicAccount
};
use std::path::{Path, PathBuf};
use js::*;
use std::str::FromStr;

fn list_accounts(mut cx: FunctionContext) -> JsResult<JsArray> {
    let guard = cx.lock();
    let cfg = VaultConfig::get_config(&mut cx);
    let vault = Vault::new(cfg);
    let accounts = vault.list_accounts();

    let result = JsArray::new(&mut cx, accounts.len() as u32);
    for (i, e) in accounts.iter().map(|acc| AccountData::from(acc)).enumerate() {
        let account_js = e.as_js_object(&mut cx);
        result.set(&mut cx, i as u32, account_js).unwrap();
    }

    Ok(result)
}

fn import_account(mut cx: FunctionContext) -> JsResult<JsObject> {
    let guard = cx.lock();
    let cfg = VaultConfig::get_config(&mut cx);
    let vault = Vault::new(cfg);

    let raw = cx.argument::<JsString>(1).expect("Input JSON is not provided").value();
    let pk = KeyFile::decode(raw.as_str()).expect("Invalid JSON");
    vault.put(&pk);

    let result = JsObject::new(&mut cx);
    let id_handle = cx.string(pk.uuid.to_string());
    result.set(&mut cx, "id", id_handle);

    Ok(result)
}

fn export_account(mut cx: FunctionContext) -> JsResult<JsString> {
    let guard = cx.lock();
    let cfg = VaultConfig::get_config(&mut cx);
    let vault = Vault::new(cfg);

    let address_js = cx.argument::<JsString>(1).unwrap().value();
    let address = Address::from_str(address_js.as_str()).expect("Invalid address");

    let kf= vault.get(&address);
    let value = serde_json::to_value(&kf).expect("Failed to encode JSON");
    let value_js = cx.string(format!("{}", value));

    Ok(value_js)
}

fn update_account(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let guard = cx.lock();
    let cfg = VaultConfig::get_config(&mut cx);
    let vault = Vault::new(cfg);

    let address_str = cx.argument::<JsString>(1).unwrap().value();
    let address = Address::from_str(address_str.as_str()).expect("Invalid address");
    let mut kf = vault.get(&address);

    let update_js = cx.argument::<JsString>(2).unwrap().value();
    let update = serde_json::from_str::<UpdateAccount>(update_js.as_str())
        .expect("Invalid update JSON");

    kf.name = update.name.or(kf.name);
    kf.description = update.description.or(kf.description);
    vault.put(&kf);

    let result = cx.boolean(true);
    Ok(result)
}

fn sign_tx(mut cx: FunctionContext) -> JsResult<JsString> {
    let guard = cx.lock();
    let cfg = VaultConfig::get_config(&mut cx);
    let chain_id = cfg.chain.get_chain_id();
    let vault = Vault::new(cfg);

    let sign_js = cx.argument::<JsString>(1).unwrap().value();
    let sign = serde_json::from_str::<SignTxTransaction>(sign_js.as_str())
        .expect("Invalid sign JSON");

    let address = Address::from_str(sign.from.as_str()).expect("Invalid from address");
    let kf= vault.get(&address);

    let raw_hex = match kf.crypto {
        CryptoType::Core(_) => {
            let pass = cx.argument::<JsString>(2).unwrap().value();

            let tr = sign.try_into().expect("Invalid sign JSON");
            let pk = kf.decrypt_key(&pass).expect("Invalid password");
            let raw_tx = tr.to_signed_raw(pk, chain_id).expect("Expect to sign a transaction");
            format!("0x{}", raw_tx.to_hex())
        }
        _ => panic!("Unsupported crypto")
    };

    let value_js = cx.string(format!("{}", raw_hex));

    Ok(value_js)
}

fn import_mnemonic(mut cx: FunctionContext) -> JsResult<JsObject> {
    let guard = cx.lock();
    let cfg = VaultConfig::get_config(&mut cx);
    let vault = Vault::new(cfg);

    let raw = cx.argument::<JsString>(1).expect("Input JSON is not provided").value();
    let account: NewMnemonicAccount = serde_json::from_str(&raw).expect("Invalid JSON");

    if account.password.is_empty() {
        panic!("Empty password");
    }

    let mnemonic = Mnemonic::try_from(Language::English, &account.mnemonic).expect("Mnemonic is not valid");
    let hd_path = HDPath::try_from(&account.hd_path).expect("HDPath is not valid");
    let pk = generate_key(&hd_path, &mnemonic.seed("")).expect("Unable to generate private key");

    let kdf = if cfg!(target_os = "windows") {
        Kdf::from_str("pbkdf").expect("PBKDF not available")
    } else {
        Kdf::from(KdfDepthLevel::Normal)
    };

    let mut rng = os_random();
    let kf = KeyFile::new_custom(
        pk,
        &account.password,
        kdf,
        &mut rng,
        Some(account.name),
        Some(account.description),
    ).expect("Unable to generate KeyFile");

    let addr = kf.address.to_string();
    vault.put(&kf);

    let result = JsObject::new(&mut cx);
    let id_handle = cx.string(kf.uuid.to_string());
    result.set(&mut cx, "id", id_handle);

    Ok(result)
}

register_module!(mut cx, {
    cx.export_function("listAccounts", list_accounts);
    cx.export_function("importAccount", import_account);
    cx.export_function("exportAccount", export_account);
    cx.export_function("updateAccount", update_account);
    cx.export_function("signTx", sign_tx);
    cx.export_function("importMnemonic", import_mnemonic);
    Ok(())
});
