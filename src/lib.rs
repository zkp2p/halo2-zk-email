#[cfg(not(target_arch = "wasm32"))]
mod helpers;
pub mod regex_sha2;
pub mod regex_sha2_base64;
pub mod sign_verify;
pub mod utils;
pub use crate::helpers::*;
use crate::regex_sha2::RegexSha2Config;
use crate::sign_verify::*;
use crate::utils::*;
use cfdkim::{canonicalize_signed_email, SignerBuilder};
use fancy_regex::Regex;
use halo2_base::halo2_proofs::circuit::{AssignedCell, Cell, Region, SimpleFloorPlanner, Value};
use halo2_base::halo2_proofs::plonk::{Circuit, Column, ConstraintSystem, Instance};
use halo2_base::halo2_proofs::{circuit::Layouter, plonk::Error};
use halo2_base::utils::{decompose, decompose_fe_to_u64_limbs, value_to_option};
use halo2_base::{gates::range::RangeStrategy::Vertical, ContextParams, SKIP_FIRST_PASS};
use halo2_base::{
    gates::{flex_gate::FlexGateConfig, range::RangeConfig, GateInstructions, RangeInstructions},
    utils::PrimeField,
    Context,
};
use halo2_base::{AssignedValue, QuantumCell};
use halo2_dynamic_sha256::{AssignedHashResult, Sha256DynamicConfig};
use halo2_regex::defs::RegexDefs;
pub use halo2_regex::vrm;
use halo2_regex::{
    defs::{AllstrRegexDef, SubstrRegexDef},
    AssignedRegexResult,
};
use halo2_rsa::{AssignedRSAPublicKey, AssignedRSASignature, RSAConfig, RSAInstructions, RSAPubE, RSAPublicKey, RSASignature};
use itertools::Itertools;
use mailparse::parse_mail;
use num_bigint::BigUint;
use num_traits::FromPrimitive;
use rand::{thread_rng, CryptoRng, Rng, RngCore};
use regex_sha2_base64::RegexSha2Base64Config;
use rsa::{PublicKeyParts, RsaPrivateKey};
use serde_json;
use sha2::{Digest, Sha256};
use snark_verifier::loader::LoadedScalar;

use snark_verifier_sdk::CircuitExt;
use std::fs::File;

// #[derive(Debug, Clone)]
// pub struct EmailVerifyResult<'a, F: PrimeField> {
//     pub assigned_headerhash: Vec<AssignedValue<'a, F>>,
//     pub assigned_bodyhash: Vec<AssignedCell<F, F>>,
//     pub header_result: AssignedRegexResult<'a, F>,
//     pub body_result: AssignedRegexResult<'a, F>,
//     pub headerhash_value: Vec<u8>,
//     pub bodyhash_value: Vec<u8>,
// }

// #[derive(Debug, Clone)]
// pub struct EmailVerifyConfig<F: PrimeField> {
//     header_processer: RegexSha2Config<F>,
//     body_processer: RegexSha2Base64Config<F>,
//     rsa_config: RSAConfig<F>,
// }

// impl<F: PrimeField> EmailVerifyConfig<F> {
//     pub fn configure(
//         meta: &mut ConstraintSystem<F>,
//         range_config: RangeConfig<F>,
//         header_max_byte_size: usize,
//         bodyhash_defs: RegexDefs,
//         header_regex_defs: Vec<RegexDefs>,
//         body_max_byte_size: usize,
//         body_regex_defs: Vec<RegexDefs>,
//         public_key_bits: usize,
//     ) -> Self {
//         let header_defs = [vec![bodyhash_defs], header_regex_defs].concat();
//         let header_processer = RegexSha2Config::configure(meta, header_max_byte_size, range_config.clone(), header_defs);
//         let body_processer = RegexSha2Base64Config::configure(meta, body_max_byte_size, range_config.clone(), body_regex_defs);
//         let biguint_config = halo2_rsa::BigUintConfig::construct(range_config, 64);
//         let rsa_config = RSAConfig::construct(biguint_config, public_key_bits, 5);
//         Self {
//             header_processer,
//             body_processer,
//             rsa_config,
//         }
//     }

//     pub fn assign_public_key<'v>(&self, ctx: &mut Context<'v, F>, public_key: RSAPublicKey<F>) -> Result<AssignedRSAPublicKey<'v, F>, Error> {
//         self.rsa_config.assign_public_key(ctx, public_key)
//     }

//     pub fn assign_signature<'v>(&self, ctx: &mut Context<'v, F>, signature: RSASignature<F>) -> Result<AssignedRSASignature<'v, F>, Error> {
//         self.rsa_config.assign_signature(ctx, signature)
//     }

//     pub fn verify_email<'v: 'a, 'a>(
//         &self,
//         ctx: &mut Context<'v, F>,
//         sha256_config: &mut Sha256DynamicConfig<F>,
//         header_bytes: &[u8],
//         body_bytes: &[u8],
//         public_key: &AssignedRSAPublicKey<'v, F>,
//         signature: &AssignedRSASignature<'v, F>,
//     ) -> Result<EmailVerifyResult<'a, F>, Error> {
//         let gate = sha256_config.range().gate.clone();

//         // 1. Extract sub strings in the body and compute the base64 encoded hash of the body.
//         let body_result = self.body_processer.match_hash_and_base64(ctx, sha256_config, body_bytes)?;

//         // 2. Extract sub strings in the header, which includes the body hash, and compute the raw hash of the header.
//         let header_result = self.header_processer.match_and_hash(ctx, sha256_config, header_bytes)?;

//         // 3. Verify the rsa signature.
//         let mut hashed_bytes = header_result.hash_bytes;
//         hashed_bytes.reverse();
//         let bytes_bits = hashed_bytes.len() * 8;
//         let limb_bits = self.rsa_config.biguint_config().limb_bits;
//         let limb_bytes = limb_bits / 8;
//         let mut hashed_u64s = vec![];
//         let bases = (0..limb_bytes)
//             .map(|i| F::from((1u64 << (8 * i)) as u64))
//             .map(QuantumCell::Constant)
//             .collect::<Vec<QuantumCell<F>>>();
//         for i in 0..(bytes_bits / limb_bits) {
//             let left = hashed_bytes[limb_bytes * i..limb_bytes * (i + 1)]
//                 .iter()
//                 .map(QuantumCell::Existing)
//                 .collect::<Vec<QuantumCell<F>>>();
//             let sum = gate.inner_product(ctx, left, bases.clone());
//             hashed_u64s.push(sum);
//         }
//         let is_sign_valid = self.rsa_config.verify_pkcs1v15_signature(ctx, public_key, &hashed_u64s, signature)?;
//         gate.assert_is_const(ctx, &is_sign_valid, F::one());
//         hashed_bytes.reverse();
//         // [IMPORTANT] Here, we don't verify that the encoded hash value is equal to the value in the email header.
//         // To constraint their equivalences, you should put these values in the instance column and specify the same hash bytes.

//         // 4. Check that the encoded hash value is equal to the value in the email header.
//         // let hash_body_substr = &header_result.regex.substrs_bytes[0];
//         // let body_encoded_hash = body_result.encoded_hash;
//         // debug_assert_eq!(hash_body_substr.len(), body_encoded_hash.len());
//         // for (substr_byte, encoded_byte) in
//         //     hash_body_substr.iter().zip(body_encoded_hash.into_iter())
//         // {
//         //     ctx.region
//         //         .constrain_equal(substr_byte.cell(), encoded_byte.cell())?;
//         // }
//         // gate.assert_is_const(ctx, &header_result.substrs.substrs_length[0], F::from(44));
//         Ok(EmailVerifyResult {
//             assigned_headerhash: hashed_bytes,
//             assigned_bodyhash: body_result.encoded_hash,
//             header_result: header_result.regex,
//             body_result: body_result.regex,
//             headerhash_value: header_result.hash_value,
//             bodyhash_value: body_result.encoded_hash_value,
//         })
//     }

//     pub fn load(&self, layouter: &mut impl Layouter<F>) -> Result<(), Error> {
//         self.header_processer.load(layouter)?;
//         self.body_processer.load(layouter)?;
//         Ok(())
//     }
// }

pub const EMAIL_VERIFY_CONFIG_ENV: &'static str = "EMAIL_VERIFY_CONFIG";
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sha2ConfigParams {
    pub num_bits_lookup: usize,
    pub num_advice_columns: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct HeaderConfigParams {
    pub bodyhash_allstr_filepath: String,
    pub bodyhash_substr_filepath: String,
    pub allstr_filepathes: Vec<String>,
    pub substr_filepathes: Vec<Vec<String>>,
    pub max_byte_size: usize,
    pub substr_regexes: Vec<Vec<String>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct BodyConfigParams {
    pub allstr_filepathes: Vec<String>,
    pub substr_filepathes: Vec<Vec<String>>,
    pub max_byte_size: usize,
    pub substr_regexes: Vec<Vec<String>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SignVerifyConfigParams {
    pub public_key_bits: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DefaultEmailVerifyConfigParams {
    pub degree: u32,
    pub num_flex_advice: usize,
    pub num_range_lookup_advice: usize,
    pub num_flex_fixed: usize,
    pub range_lookup_bits: usize,
    pub sha256_config: Sha2ConfigParams,
    pub sign_verify_config: SignVerifyConfigParams,
    pub header_config: HeaderConfigParams,
    pub body_config: BodyConfigParams,
}

#[derive(Debug, Clone)]
pub struct DefaultEmailVerifyConfig<F: PrimeField> {
    sha256_config: Sha256DynamicConfig<F>,
    sign_verify_config: SignVerifyConfig<F>,
    header_config: RegexSha2Config<F>,
    body_config: RegexSha2Base64Config<F>,
    public_hash: Column<Instance>,
}

#[derive(Debug, Clone)]
pub struct DefaultEmailVerifyCircuit<F: PrimeField> {
    pub header_bytes: Vec<u8>,
    pub body_bytes: Vec<u8>,
    pub public_key: RSAPublicKey<F>,
    pub signature: RSASignature<F>,
}

impl<F: PrimeField> Circuit<F> for DefaultEmailVerifyCircuit<F> {
    type Config = DefaultEmailVerifyConfig<F>;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self {
            header_bytes: vec![],
            body_bytes: vec![],
            public_key: self.public_key.clone(),
            signature: self.signature.clone(),
        }
    }

    fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
        let params = Self::read_config_params();
        let range_config = RangeConfig::configure(
            meta,
            Vertical,
            &[params.num_flex_advice],
            &[params.num_range_lookup_advice],
            params.num_flex_advice,
            params.range_lookup_bits,
            0,
            params.degree as usize,
        );
        assert_eq!(params.header_config.allstr_filepathes.len(), params.header_config.substr_filepathes.len());
        assert_eq!(params.body_config.allstr_filepathes.len(), params.body_config.substr_filepathes.len());

        let sha256_config = Sha256DynamicConfig::configure(
            meta,
            vec![
                params.body_config.max_byte_size,
                params.header_config.max_byte_size,
                128 + params.sign_verify_config.public_key_bits / 8 + 2 * (params.header_config.max_byte_size + params.body_config.max_byte_size) + 64, // (header hash, base64 body hash, padding, RSA public key, masked chars, substr ids)
            ],
            range_config.clone(),
            params.sha256_config.num_bits_lookup,
            params.sha256_config.num_advice_columns,
            false,
        );

        let sign_verify_config = SignVerifyConfig::configure(meta, range_config.clone(), params.sign_verify_config.public_key_bits);

        // assert_eq!(params.body_regex_filepathes.len(), params.body_substr_filepathes.len());
        let bodyhash_allstr_def = AllstrRegexDef::read_from_text(&params.header_config.bodyhash_allstr_filepath);
        let bodyhash_substr_def = SubstrRegexDef::read_from_text(&params.header_config.bodyhash_substr_filepath);
        let bodyhash_defs = RegexDefs {
            allstr: bodyhash_allstr_def,
            substrs: vec![bodyhash_substr_def],
        };
        let header_regex_defs = params
            .header_config
            .allstr_filepathes
            .iter()
            .zip(params.header_config.substr_filepathes.iter())
            .map(|(allstr_path, substr_pathes)| {
                let allstr = AllstrRegexDef::read_from_text(&allstr_path);
                let substrs = substr_pathes.into_iter().map(|path| SubstrRegexDef::read_from_text(&path)).collect_vec();
                RegexDefs { allstr, substrs }
            })
            .collect_vec();
        let header_config = RegexSha2Config::configure(
            meta,
            params.header_config.max_byte_size,
            range_config.clone(),
            vec![vec![bodyhash_defs], header_regex_defs].concat(),
        );

        let body_regex_defs = params
            .body_config
            .allstr_filepathes
            .iter()
            .zip(params.body_config.substr_filepathes.iter())
            .map(|(allstr_path, substr_pathes)| {
                let allstr = AllstrRegexDef::read_from_text(&allstr_path);
                let substrs = substr_pathes.into_iter().map(|path| SubstrRegexDef::read_from_text(&path)).collect_vec();
                RegexDefs { allstr, substrs }
            })
            .collect_vec();
        let body_config = RegexSha2Base64Config::configure(meta, params.body_config.max_byte_size, range_config, body_regex_defs);

        let public_hash = meta.instance_column();
        meta.enable_equality(public_hash);
        DefaultEmailVerifyConfig {
            sha256_config,
            sign_verify_config,
            header_config,
            body_config,
            public_hash,
        }
    }

    fn synthesize(&self, mut config: Self::Config, mut layouter: impl Layouter<F>) -> Result<(), Error> {
        config.sha256_config.range().load_lookup_table(&mut layouter)?;
        config.sha256_config.load(&mut layouter)?;
        config.header_config.load(&mut layouter)?;
        config.body_config.load(&mut layouter)?;
        let mut first_pass = SKIP_FIRST_PASS;
        let mut public_hash_cell = vec![];
        let params = Self::read_config_params();
        layouter.assign_region(
            || "zkemail",
            |region| {
                if first_pass {
                    first_pass = false;
                    return Ok(());
                }
                let ctx = &mut config.sha256_config.new_context(region);
                // 1. Extract sub strings in the body and compute the base64 encoded hash of the body.
                let body_result = config.body_config.match_hash_and_base64(ctx, &mut config.sha256_config, &self.body_bytes)?;

                // 2. Extract sub strings in the header, which includes the body hash, and compute the raw hash of the header.
                let header_result = config.header_config.match_and_hash(ctx, &mut config.sha256_config, &self.header_bytes)?;

                // 3. Verify the rsa signature.
                let (assigned_public_key, _) = config
                    .sign_verify_config
                    .verify_signature(ctx, &header_result.hash_bytes, self.public_key.clone(), self.signature.clone())?;

                let public_hash_input = {
                    let header_str = String::from_utf8(self.header_bytes.clone()).unwrap();
                    let body_str = String::from_utf8(self.body_bytes.clone()).unwrap();
                    let params = Self::read_config_params();
                    let (header_substrs, body_substrs) = get_email_substrs(&header_str, &body_str, params.header_config.substr_regexes, params.body_config.substr_regexes);
                    get_email_circuit_public_hash_input(
                        &header_result.hash_value,
                        &value_to_option(self.public_key.n.clone()).unwrap().to_bytes_le(),
                        header_substrs,
                        body_substrs,
                        params.header_config.max_byte_size,
                        params.body_config.max_byte_size,
                    )
                };
                let public_hash_result: AssignedHashResult<F> = config.sha256_config.digest(ctx, &public_hash_input)?;
                // for (idx, v) in public_hash_result.input_bytes[128..(128 + 256)].iter().enumerate() {
                //     v.value().map(|v| println!("idx {} code {}", idx, v.get_lower_32()));
                // }
                let range = config.sha256_config.range().clone();
                let gate = range.gate.clone();
                // for (idx, v) in result.header_result.masked_characters.iter().enumerate() {
                //     v.value()
                //         .map(|v| println!("idx {} code {} char {}", idx, v.get_lower_32(), (v.get_lower_32() as u8) as char));
                // }
                // for (idx, v) in result.body_result.masked_characters.iter().enumerate() {
                //     v.value()
                //         .map(|v| println!("idx {} code {} char {}", idx, v.get_lower_32(), (v.get_lower_32() as u8) as char));
                // }
                let assigned_public_key_bytes = assigned_public_key
                    .n
                    .limbs()
                    .into_iter()
                    .flat_map(|limb| {
                        let limb_val = value_to_option(limb.value()).unwrap();
                        let bytes = decompose_fe_to_u64_limbs(limb_val, 64 / 8, 8);
                        let mut sum = gate.load_zero(ctx);
                        let assigned = bytes
                            .into_iter()
                            .enumerate()
                            .map(|(idx, byte)| {
                                let assigned = gate.load_witness(ctx, Value::known(F::from(byte)));
                                range.range_check(ctx, &assigned, 8);
                                sum = gate.mul_add(
                                    ctx,
                                    QuantumCell::Existing(&assigned),
                                    QuantumCell::Constant(F::from(1u64 << (8 * idx))),
                                    QuantumCell::Existing(&sum),
                                );
                                assigned
                            })
                            .collect_vec();
                        gate.assert_equal(ctx, QuantumCell::Existing(&sum), QuantumCell::Existing(limb));
                        assigned
                    })
                    .collect_vec();
                // for (idx, v) in assigned_public_key_bytes.iter().enumerate() {
                //     v.value().map(|v| println!("idx {} byte {}", 128 + idx, v.get_lower_32()));
                // }
                let assigned_public_hash_input = vec![
                    header_result.hash_bytes.into_iter().map(|v| v.cell()).collect_vec(),
                    body_result.encoded_hash.into_iter().map(|v| v.cell()).collect_vec(),
                    vec![gate.load_zero(ctx).cell(); 128 - 32 - 44],
                    assigned_public_key_bytes.into_iter().map(|v| v.cell()).collect_vec(),
                    vec![header_result.regex.masked_characters, body_result.regex.masked_characters]
                        .concat()
                        .into_iter()
                        .map(|v| v.cell())
                        .collect_vec(),
                    vec![header_result.regex.all_substr_ids, body_result.regex.all_substr_ids]
                        .concat()
                        .into_iter()
                        .map(|v| v.cell())
                        .collect_vec(),
                ]
                .concat();
                for (a, b) in public_hash_result.input_bytes[0..assigned_public_hash_input.len()]
                    .into_iter()
                    .map(|v| v.cell())
                    .collect_vec()
                    .into_iter()
                    .zip(assigned_public_hash_input.into_iter())
                {
                    ctx.region.constrain_equal(a, b)?;
                }
                debug_assert_eq!(public_hash_result.output_bytes.len(), 32);
                let mut packed_public_hash = gate.load_zero(ctx);
                let mut coeff = F::from(1u64);
                for byte in public_hash_result.output_bytes[0..31].iter() {
                    packed_public_hash = gate.mul_add(ctx, QuantumCell::Existing(byte), QuantumCell::Constant(coeff), QuantumCell::Existing(&packed_public_hash));
                    coeff *= F::from(256u64);
                }
                config.sha256_config.range().finalize(ctx);
                public_hash_cell.push(packed_public_hash.cell());
                Ok(())
            },
        )?;
        layouter.constrain_instance(public_hash_cell[0], config.public_hash, 0)?;
        Ok(())
    }
}

impl<F: PrimeField> CircuitExt<F> for DefaultEmailVerifyCircuit<F> {
    fn num_instance(&self) -> Vec<usize> {
        vec![1]
    }

    fn instances(&self) -> Vec<Vec<F>> {
        let headerhash_value = Sha256::digest(&self.header_bytes).to_vec();
        // println!("header hash {}", hex::encode(&headerhash_value));
        let header_str = String::from_utf8(self.header_bytes.clone()).unwrap();
        let body_str = String::from_utf8(self.body_bytes.clone()).unwrap();
        let params = Self::read_config_params();
        let (header_substrs, body_substrs) = get_email_substrs(&header_str, &body_str, params.header_config.substr_regexes, params.body_config.substr_regexes);
        let public_hash_input = get_email_circuit_public_hash_input(
            &headerhash_value,
            &value_to_option(self.public_key.n.clone()).unwrap().to_bytes_le(),
            header_substrs,
            body_substrs,
            params.header_config.max_byte_size,
            params.body_config.max_byte_size,
        );
        let public_hash: Vec<u8> = Sha256::digest(&public_hash_input).to_vec();
        // println!("public hash {}", hex::encode(public_hash.clone()));
        let public_fr = {
            let lo = F::from_u128(u128::from_le_bytes(public_hash[0..16].try_into().unwrap()));
            let mut hi_bytes = [0; 16];
            for idx in 0..15 {
                hi_bytes[idx] = public_hash[16 + idx];
            }
            let hi = F::from_u128(u128::from_le_bytes(hi_bytes));
            hi * F::from(2).pow_const(128) + lo
        };
        // println!("public fr {:?}", public_fr.to_repr(),);
        vec![vec![public_fr]]
    }
}

impl<F: PrimeField> DefaultEmailVerifyCircuit<F> {
    pub const DEFAULT_E: u128 = 65537;

    pub fn read_config_params() -> DefaultEmailVerifyConfigParams {
        let path = std::env::var(EMAIL_VERIFY_CONFIG_ENV).expect("You should set the configure file path to EMAIL_VERIFY_CONFIG.");
        let params: DefaultEmailVerifyConfigParams =
            serde_json::from_reader(File::open(path.as_str()).expect(&format!("{} does not exist.", path))).expect("File is found but invalid.");
        params
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use cfdkim::{canonicalize_signed_email, resolve_public_key, SignerBuilder};
    use halo2_base::halo2_proofs::{
        circuit::Value,
        dev::{CircuitCost, FailureLocation, MockProver, VerifyFailure},
        halo2curves::bn256::{Fr, G1},
    };
    use halo2_regex::vrm::DecomposedRegexConfig;
    use halo2_rsa::RSAPubE;
    use mailparse::parse_mail;
    use num_bigint::BigUint;
    use rand::thread_rng;
    use rsa::{PublicKeyParts, RsaPrivateKey};
    use snark_verifier_sdk::CircuitExt;
    use std::{io::Read, path::Path};
    use temp_env;

    #[test]
    fn test_generated_email1() {
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test1_email_verify.config"), || {
            let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
            regex_bodyhash_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
            regex_from_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test1_email_body_defs.json").unwrap()).unwrap();
            regex_body_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/test1_email_body_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/test1_email_body_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let mut rng = thread_rng();
            let _private_key = RsaPrivateKey::new(&mut rng, params.sign_verify_config.public_key_bits).expect("failed to generate a key");
            let public_key = rsa::RsaPublicKey::from(&_private_key);
            let private_key = cfdkim::DkimPrivateKey::Rsa(_private_key);
            let message = concat!("From: alice@zkemail.com\r\n", "\r\n", "email was meant for @zkemailverify.",).as_bytes();
            let email = parse_mail(message).unwrap();
            let logger = slog::Logger::root(slog::Discard, slog::o!());
            let signer = SignerBuilder::new()
                .with_signed_headers(&["From"])
                .unwrap()
                .with_private_key(private_key)
                .with_selector("default")
                .with_signing_domain("zkemail.com")
                .with_logger(&logger)
                .with_header_canonicalization(cfdkim::canonicalization::Type::Relaxed)
                .with_body_canonicalization(cfdkim::canonicalization::Type::Relaxed)
                .build()
                .unwrap();
            let signature = signer.sign(&email).unwrap();
            let new_msg = vec![signature.as_bytes(), b"\r\n", message].concat();
            println!("email: {}", String::from_utf8(new_msg.clone()).unwrap());
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&new_msg).unwrap();

            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());

            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };
            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert_eq!(prover.verify(), Ok(()));
        });
    }

    #[test]
    fn test_generated_email2() {
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test2_email_verify.config"), || {
            let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
            regex_bodyhash_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
            regex_from_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_to_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/to_defs.json").unwrap()).unwrap();
            regex_to_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/to_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/to_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test2_email_body_defs.json").unwrap()).unwrap();
            regex_body_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/test2_email_body_allstr.txt").to_path_buf(),
                    &[
                        Path::new("./test_data/test2_email_body_substr_0.txt").to_path_buf(),
                        Path::new("./test_data/test2_email_body_substr_1.txt").to_path_buf(),
                    ],
                )
                .unwrap();
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let mut rng = thread_rng();
            let _private_key = RsaPrivateKey::new(&mut rng, params.sign_verify_config.public_key_bits).expect("failed to generate a key");
            let public_key = rsa::RsaPublicKey::from(&_private_key);
            let private_key = cfdkim::DkimPrivateKey::Rsa(_private_key);
            let message = concat!(
                "From: alice@zkemail.com\r\n",
                "To: bob@example.com\r\n",
                "\r\n",
                "email was meant for @zkemailverify and halo.",
            )
            .as_bytes();
            let email = parse_mail(message).unwrap();
            let logger = slog::Logger::root(slog::Discard, slog::o!());
            let signer = SignerBuilder::new()
                .with_signed_headers(&["To", "From"])
                .unwrap()
                .with_private_key(private_key)
                .with_selector("default")
                .with_signing_domain("zkemail.com")
                .with_logger(&logger)
                .with_header_canonicalization(cfdkim::canonicalization::Type::Relaxed)
                .with_body_canonicalization(cfdkim::canonicalization::Type::Relaxed)
                .build()
                .unwrap();
            let signature = signer.sign(&email).unwrap();
            println!("signature {}", signature);
            let new_msg = vec![signature.as_bytes(), b"\r\n", message].concat();
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&new_msg).unwrap();

            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());

            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert_eq!(prover.verify(), Ok(()));
        });
    }

    #[test]
    fn test_generated_email3() {
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test3_email_verify.config"), || {
            let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
            regex_bodyhash_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
            regex_from_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                    &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
                )
                .unwrap();
            let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test3_email_body_defs.json").unwrap()).unwrap();
            regex_body_decomposed
                .gen_regex_files(
                    &Path::new("./test_data/test3_email_body_allstr.txt").to_path_buf(),
                    &[
                        Path::new("./test_data/test3_email_body_substr_0.txt").to_path_buf()
                    ],
                )
                .unwrap();
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let mut rng = thread_rng();
            let _private_key = RsaPrivateKey::new(&mut rng, params.sign_verify_config.public_key_bits).expect("failed to generate a key");
            let public_key = rsa::RsaPublicKey::from(&_private_key);
            let private_key = cfdkim::DkimPrivateKey::Rsa(_private_key);
            let message = concat!(
                "From: alice@zkemail.com\r\n",
                "\r\n",
                "\r\ntps://venmo.com/code?user_id=3D123456789012345678&actor_id=3D"
            )
            .as_bytes();
            let email = parse_mail(message).unwrap();
            let logger = slog::Logger::root(slog::Discard, slog::o!());
            let signer = SignerBuilder::new()
                .with_signed_headers(&["From"])
                .unwrap()
                .with_private_key(private_key)
                .with_selector("default")
                .with_signing_domain("zkemail.com")
                .with_logger(&logger)
                .with_header_canonicalization(cfdkim::canonicalization::Type::Relaxed)
                .with_body_canonicalization(cfdkim::canonicalization::Type::Relaxed)
                .build()
                .unwrap();
            let signature = signer.sign(&email).unwrap();
            println!("signature {}", signature);
            let new_msg = vec![signature.as_bytes(), b"\r\n", message].concat();
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&new_msg).unwrap();

            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());

            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert_eq!(prover.verify(), Ok(()));
        });
    }

    #[tokio::test]
    async fn test_existing_email1() {
        let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
        regex_bodyhash_decomposed
            .gen_regex_files(
                &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
        regex_from_decomposed
            .gen_regex_files(
                &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_to_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/to_defs.json").unwrap()).unwrap();
        regex_to_decomposed
            .gen_regex_files(
                &Path::new("./test_data/to_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/to_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_subject_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/subject_defs.json").unwrap()).unwrap();
        regex_subject_decomposed
            .gen_regex_files(
                &Path::new("./test_data/subject_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/subject_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test_ex1_email_body_defs.json").unwrap()).unwrap();
        regex_body_decomposed
            .gen_regex_files(
                &Path::new("./test_data/test_ex1_email_body_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/test_ex1_email_body_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/test_ex1_email_body_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/test_ex1_email_body_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let email_bytes = {
            let mut f = File::open("./test_data/test_email1.eml").unwrap();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            buf
        };

        let logger = slog::Logger::root(slog::Discard, slog::o!());
        let public_key = resolve_public_key(&logger, &email_bytes).await.unwrap();
        let public_key = match public_key {
            cfdkim::DkimPublicKey::Rsa(pk) => pk,
            _ => panic!("not supportted public key type."),
        };
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test_ex1_email_verify.config"), move || {
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&email_bytes).unwrap();
            println!("header len\n {}", canonicalized_header.len());
            println!("body len\n {}", canonicalized_body.len());
            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());
            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert_eq!(prover.verify(), Ok(()));
        });
    }

    #[tokio::test]
    async fn test_existing_email2() {
        let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
        regex_bodyhash_decomposed
            .gen_regex_files(
                &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
        regex_from_decomposed
            .gen_regex_files(
                &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_to_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/to_defs.json").unwrap()).unwrap();
        regex_to_decomposed
            .gen_regex_files(
                &Path::new("./test_data/to_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/to_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_subject_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/subject_defs.json").unwrap()).unwrap();
        regex_subject_decomposed
            .gen_regex_files(
                &Path::new("./test_data/subject_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/subject_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test_ex2_email_body_defs.json").unwrap()).unwrap();
        regex_body_decomposed
            .gen_regex_files(
                &Path::new("./test_data/test_ex2_email_body_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/test_ex2_email_body_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/test_ex2_email_body_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/test_ex2_email_body_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let email_bytes = {
            let mut f = File::open("./test_data/test_email2.eml").unwrap();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            buf
        };

        let logger = slog::Logger::root(slog::Discard, slog::o!());
        let public_key = resolve_public_key(&logger, &email_bytes).await.unwrap();
        let public_key = match public_key {
            cfdkim::DkimPublicKey::Rsa(pk) => pk,
            _ => panic!("not supportted public key type."),
        };
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test_ex2_email_verify.config"), move || {
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&email_bytes).unwrap();
            println!("header len\n {}", canonicalized_header.len());
            println!("body len\n {}", canonicalized_body.len());
            // println!("body\n{:?}", canonicalized_body);
            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());
            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert_eq!(prover.verify(), Ok(()));
        });
    }
    
    #[tokio::test]
    async fn test_existing_email3() {
        let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
        regex_bodyhash_decomposed
            .gen_regex_files(
                &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_timestamp_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/timestamp_defs.json").unwrap()).unwrap();
        regex_timestamp_decomposed
            .gen_regex_files(
                &Path::new("./test_data/timestamp_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/timestamp_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test_ex3_email_body_defs.json").unwrap()).unwrap();
        regex_body_decomposed
            .gen_regex_files(
                &Path::new("./test_data/test_ex3_email_body_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/test_ex3_email_body_substr_0.txt").to_path_buf()
                ],
            )
            .unwrap();
        let email_bytes = {
            let mut f = File::open("./build/demo.eml").unwrap();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            buf
        };

        let logger = slog::Logger::root(slog::Discard, slog::o!());
        let public_key = resolve_public_key(&logger, &email_bytes).await.unwrap();
        let public_key = match public_key {
            cfdkim::DkimPublicKey::Rsa(pk) => pk,
            _ => panic!("not supportted public key type."),
        };
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test_ex3_email_verify.config"), move || {
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&email_bytes).unwrap();
            println!("header len\n {}", canonicalized_header.len());
            println!("body len\n {}", canonicalized_body.len());
            // println!("body\n{:?}", canonicalized_body);
            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());
            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert_eq!(prover.verify(), Ok(()));
        });
    }

    #[tokio::test]
    async fn test_existing_email_invalid1() {
        let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
        regex_bodyhash_decomposed
            .gen_regex_files(
                &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
        regex_from_decomposed
            .gen_regex_files(
                &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_to_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/to_defs.json").unwrap()).unwrap();
        regex_to_decomposed
            .gen_regex_files(
                &Path::new("./test_data/to_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/to_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_subject_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/subject_defs.json").unwrap()).unwrap();
        regex_subject_decomposed
            .gen_regex_files(
                &Path::new("./test_data/subject_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/subject_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test_ex1_email_body_defs.json").unwrap()).unwrap();
        regex_body_decomposed
            .gen_regex_files(
                &Path::new("./test_data/test_ex1_email_body_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/test_ex1_email_body_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/test_ex1_email_body_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/test_ex1_email_body_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let email_bytes = {
            let mut f = File::open("./test_data/test_email1.eml").unwrap();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            buf
        };

        let logger = slog::Logger::root(slog::Discard, slog::o!());
        let public_key = resolve_public_key(&logger, &email_bytes).await.unwrap();
        let public_key = match public_key {
            cfdkim::DkimPublicKey::Rsa(pk) => pk,
            _ => panic!("not supportted public key type."),
        };
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test_ex1_email_verify.config"), move || {
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let (canonicalized_header, canonicalized_body, mut signature_bytes) = canonicalize_signed_email(&email_bytes).unwrap();
            println!("header len\n {}", canonicalized_header.len());
            println!("body len\n {}", canonicalized_body.len());
            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());
            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            signature_bytes[0] = 0;
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert!(prover.verify().is_err());
        });
    }

    #[tokio::test]
    async fn test_existing_email_invalid2() {
        let regex_bodyhash_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/bodyhash_defs.json").unwrap()).unwrap();
        regex_bodyhash_decomposed
            .gen_regex_files(
                &Path::new("./test_data/bodyhash_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/bodyhash_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_from_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/from_defs.json").unwrap()).unwrap();
        regex_from_decomposed
            .gen_regex_files(
                &Path::new("./test_data/from_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/from_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_to_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/to_defs.json").unwrap()).unwrap();
        regex_to_decomposed
            .gen_regex_files(
                &Path::new("./test_data/to_allstr.txt").to_path_buf(),
                &[Path::new("./test_data/to_substr_0.txt").to_path_buf()],
            )
            .unwrap();
        let regex_subject_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/subject_defs.json").unwrap()).unwrap();
        regex_subject_decomposed
            .gen_regex_files(
                &Path::new("./test_data/subject_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/subject_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/subject_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let regex_body_decomposed: DecomposedRegexConfig = serde_json::from_reader(File::open("./test_data/test_ex1_email_body_defs.json").unwrap()).unwrap();
        regex_body_decomposed
            .gen_regex_files(
                &Path::new("./test_data/test_ex1_email_body_allstr.txt").to_path_buf(),
                &[
                    Path::new("./test_data/test_ex1_email_body_substr_0.txt").to_path_buf(),
                    Path::new("./test_data/test_ex1_email_body_substr_1.txt").to_path_buf(),
                    Path::new("./test_data/test_ex1_email_body_substr_2.txt").to_path_buf(),
                ],
            )
            .unwrap();
        let email_bytes = {
            let mut f = File::open("./test_data/invalid_test_email1.eml").unwrap();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            buf
        };

        let logger = slog::Logger::root(slog::Discard, slog::o!());
        let public_key = resolve_public_key(&logger, &email_bytes).await.unwrap();
        let public_key = match public_key {
            cfdkim::DkimPublicKey::Rsa(pk) => pk,
            _ => panic!("not supportted public key type."),
        };
        temp_env::with_var(EMAIL_VERIFY_CONFIG_ENV, Some("./configs/test_ex1_email_verify.config"), move || {
            let params = DefaultEmailVerifyCircuit::<Fr>::read_config_params();
            let (canonicalized_header, canonicalized_body, signature_bytes) = canonicalize_signed_email(&email_bytes).unwrap();
            println!("header len\n {}", canonicalized_header.len());
            println!("body len\n {}", canonicalized_body.len());
            println!("canonicalized_header:\n{}", String::from_utf8(canonicalized_header.clone()).unwrap());
            println!("canonicalized_body:\n{}", String::from_utf8(canonicalized_body.clone()).unwrap());
            let e = RSAPubE::Fix(BigUint::from(DefaultEmailVerifyCircuit::<Fr>::DEFAULT_E));
            let n_big = BigUint::from_radix_le(&public_key.n().clone().to_radix_le(16), 16).unwrap();
            let public_key = RSAPublicKey::<Fr>::new(Value::known(BigUint::from(n_big)), e);
            let signature = RSASignature::<Fr>::new(Value::known(BigUint::from_bytes_be(&signature_bytes)));
            let circuit = DefaultEmailVerifyCircuit {
                header_bytes: canonicalized_header,
                body_bytes: canonicalized_body,
                public_key,
                signature,
            };

            let instances = circuit.instances();
            let prover = MockProver::run(params.degree, &circuit, instances).unwrap();
            assert!(prover.verify().is_err());
        });
    }
}
