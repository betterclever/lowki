//! Differential tests against `vectors/lowki-keys/vectors.json` (generated from the Kohaku
//! TypeScript sources, @scure/*, @noble/*, viem and Node WebCrypto; see gen.ts).

use lowki_keys::*;
use serde::Deserialize;

const VECTORS: &str = include_str!("../../../vectors/lowki-keys/vectors.json");

#[derive(Deserialize)]
struct Vectors {
    bip39_generate: Vec<Gen>,
    bip39_validate: Vec<Validate>,
    paths: Vec<PathCase>,
    derive: Vec<Derive>,
    scope: Vec<Scope>,
    app: Vec<App>,
    eip55: Vec<Eip55>,
    wrap: Vec<Wrap>,
    unwrap_tamper: Vec<Tamper>,
    sign: Vec<Sign>,
}
#[derive(Deserialize)]
struct Gen {
    entropy: String,
    mnemonic: String,
    seed: String,
}
#[derive(Deserialize)]
struct Validate {
    mnemonic: String,
    valid: bool,
    seed: Option<String>,
    note: Option<String>,
}
#[derive(Deserialize)]
struct PathCase {
    path: String,
    indices: Option<Vec<u32>>,
    error: Option<bool>,
}
#[derive(Deserialize)]
struct Derive {
    mnemonic: String,
    seed: String,
    path: String,
    key: Option<String>,
    error: Option<bool>,
}
#[derive(Deserialize)]
struct Scope {
    prefixes: Vec<String>,
    paths: Vec<String>,
    path: String,
    result: String,
}
#[derive(Deserialize)]
struct App {
    seed: String,
    origin: String,
    index: u32,
    path: String,
    private_key: String,
    address: String,
}
#[derive(Deserialize)]
struct Eip55 {
    address: String,
    checksummed: String,
}
#[derive(Deserialize)]
struct Wrap {
    prf: String,
    entropy: String,
    nonce: String,
    kek: String,
    blob: String,
}
#[derive(Deserialize)]
struct Tamper {
    prf: String,
    blob: String,
    error: String,
    note: String,
}
#[derive(Deserialize)]
struct Sign {
    private_key: String,
    digest: String,
    r: String,
    s: String,
    v: u8,
}

fn vectors() -> Vectors {
    serde_json::from_str(VECTORS).unwrap()
}
fn arr<const N: usize>(h: &str) -> [u8; N] {
    hex::decode(h).unwrap().try_into().unwrap()
}

#[test]
fn bip39_generate() {
    let v = vectors().bip39_generate;
    assert!(v.len() >= 64);
    for c in &v {
        let e: [u8; 32] = arr(&c.entropy);
        assert_eq!(*entropy_to_mnemonic(&e), c.mnemonic);
        assert!(validate_mnemonic(&c.mnemonic));
        assert_eq!(*mnemonic_to_entropy(&c.mnemonic).unwrap(), e);
        assert_eq!(hex::encode(*mnemonic_to_seed(&c.mnemonic).unwrap()), c.seed);
        assert_eq!(hex::encode(*entropy_to_seed(&e)), c.seed);
    }
}

#[test]
fn bip39_validate() {
    let v = vectors().bip39_validate;
    assert!(v.iter().any(|c| c.valid) && v.iter().any(|c| !c.valid));
    for c in &v {
        assert_eq!(
            validate_mnemonic(&c.mnemonic),
            c.valid,
            "{:?} {:?}",
            c.note,
            c.mnemonic
        );
        match &c.seed {
            Some(seed) => assert_eq!(&hex::encode(*mnemonic_to_seed(&c.mnemonic).unwrap()), seed),
            None => assert_eq!(
                mnemonic_to_seed(&c.mnemonic).unwrap_err(),
                Error::InvalidMnemonic
            ),
        }
        // only 24-word phrases yield 32-byte entropy
        let is24 = c.valid && c.mnemonic.split(' ').count() == 24;
        assert_eq!(
            mnemonic_to_entropy(&c.mnemonic).is_ok(),
            is24,
            "{:?}",
            c.note
        );
    }
}

#[test]
fn derivation_paths() {
    let v = vectors().paths;
    let invalid = v.iter().filter(|c| c.error == Some(true)).count();
    assert!(invalid >= 50, "want many invalid paths, got {invalid}");
    for c in &v {
        let got = parse_derivation_path(&c.path);
        match &c.indices {
            Some(idx) => assert_eq!(got.as_ref().unwrap(), idx, "{:?}", c.path),
            None => assert_eq!(
                got,
                Err(Error::InvalidPath),
                "{:?} should be rejected",
                c.path
            ),
        }
    }
}

#[test]
fn keystore_derive_at() {
    let v = vectors().derive;
    for c in &v {
        let seed: [u8; 64] = arr(&c.seed);
        assert_eq!(*mnemonic_to_seed(&c.mnemonic).unwrap(), seed);
        let ks = Keystore::from_mnemonic(&c.mnemonic).unwrap();
        let got = derive_at(&seed, &c.path);
        assert_eq!(got.is_ok(), ks.derive_at(&c.path).is_ok());
        match &c.key {
            Some(k) => {
                assert_eq!(&hex::encode(*got.unwrap()), k, "{}", c.path);
                assert_eq!(&hex::encode(*ks.derive_at(&c.path).unwrap()), k);
            }
            None => {
                assert_eq!(c.error, Some(true));
                assert!(got.is_err(), "{} should fail", c.path);
            }
        }
    }
    // the RAILGUN branch and its non-hardened error are covered
    assert!(
        v.iter()
            .any(|c| c.path.starts_with("m/44'/1984'/") && c.key.is_some())
    );
    let seed: [u8; 64] = arr(&v[0].seed);
    assert_eq!(
        derive_at(&seed, "m/44'/1984'/0").unwrap_err(),
        Error::RailgunNonHardened
    );
}

#[test]
fn scoped_keystore() {
    let v = vectors().scope;
    let ks = Keystore::from_seed(&[7u8; 64]);
    for c in &v {
        let prefixes: Vec<&str> = c.prefixes.iter().map(String::as_str).collect();
        let paths: Vec<&str> = c.paths.iter().map(String::as_str).collect();
        let got = match ks.scope(&prefixes, &paths) {
            Err(Error::InvalidScope) => "invalid_scope",
            Err(e) => panic!("{e}"),
            Ok(s) => match s.derive_at(&c.path) {
                Ok(k) => {
                    assert_eq!(*k, *ks.derive_at(&c.path).unwrap());
                    "ok"
                }
                Err(Error::PathNotAllowed) => "not_allowed",
                Err(Error::InvalidPath) => "invalid_path",
                Err(e) => panic!("{e}"),
            },
        };
        assert_eq!(got, c.result, "{:?} {:?} {}", c.prefixes, c.paths, c.path);
    }
    // allow_path grows the scope like Kohaku's live Set
    let mut s = ks.scope(&["m/28784'/1'"], &[]).unwrap();
    assert_eq!(
        s.derive_at("m/44'/60'/0'/0/0").unwrap_err(),
        Error::PathNotAllowed
    );
    s.allow_path("m/44h/60h/0h/0/0");
    assert!(s.derive_at("m/44'/60'/0'/0/0").is_ok());
}

#[test]
fn app_eoa() {
    let v = vectors().app;
    for c in &v {
        let seed: [u8; 64] = arr(&c.seed);
        assert_eq!(app_index(&c.origin), c.index, "{:?}", c.origin);
        assert_eq!(app_path(&c.origin), c.path);
        assert_eq!(
            hex::encode(*app_private_key(&seed, &c.origin)),
            c.private_key
        );
        let addr = app_address(&seed, &c.origin);
        assert_eq!(to_checksum_address(&addr), c.address);
        assert_eq!(
            *derive_at(&seed, &c.path).unwrap(),
            arr::<32>(&c.private_key)
        );
    }
}

#[test]
fn eip55() {
    for c in &vectors().eip55 {
        assert_eq!(to_checksum_address(&arr(&c.address)), c.checksummed);
    }
}

#[test]
fn seed_wrap_webcrypto() {
    for c in &vectors().wrap {
        let prf: [u8; 32] = arr(&c.prf);
        let entropy: [u8; 32] = arr(&c.entropy);
        assert_eq!(hex::encode(*derive_kek(&prf)), c.kek);
        let blob = wrap_entropy(&prf, &entropy, &arr(&c.nonce));
        assert_eq!(hex::encode(blob), c.blob);
        assert_eq!(
            *unwrap_entropy(&prf, &hex::decode(&c.blob).unwrap()).unwrap(),
            entropy
        );
    }
}

#[test]
fn seed_unwrap_tamper() {
    let v = vectors().unwrap_tamper;
    assert!(v.len() >= 50);
    for c in &v {
        let want = match c.error.as_str() {
            "bad_version" => Error::BadVersion,
            "bad_length" => Error::BadLength,
            "tag" => Error::Decrypt,
            other => panic!("{other}"),
        };
        let got = unwrap_entropy(&arr(&c.prf), &hex::decode(&c.blob).unwrap());
        assert_eq!(got.err(), Some(want), "{}", c.note);
    }
}

#[test]
fn sign() {
    for c in &vectors().sign {
        let sig = sign_digest(&arr(&c.private_key), &arr(&c.digest)).unwrap();
        assert_eq!(hex::encode(sig.r), c.r, "{} {}", c.private_key, c.digest);
        assert_eq!(hex::encode(sig.s), c.s);
        assert_eq!(sig.v, c.v);
        assert_eq!(sig.y_parity(), c.v - 27);
    }
    assert_eq!(
        sign_digest(&[0u8; 32], &[1u8; 32]).unwrap_err(),
        Error::InvalidPrivateKey
    );
    assert_eq!(
        sign_digest(&[0xffu8; 32], &[1u8; 32]).unwrap_err(),
        Error::InvalidPrivateKey
    );
}
