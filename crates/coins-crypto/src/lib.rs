//! Cryptography layer – minimal-public-key BLS on the BN-254 curve.
//!
//! • Public key: G₁ point (32-byte arkworks compressed format)
//! • Signature : σ = H(msg)^{sk} ∈ G₂ (64-byte arkworks compressed format)
//!
//! Verify single:   e(G₁_gen, σ) == e(P, H(m))
//! Verify aggregate: e(G₁_gen, Σ) == ∏ e(Pᵢ, H(mᵢ))
//!
//! ## Implementation Details
//!
//! **Hash-to-Curve:** RFC 9380 compliant using SVDW (Shallue-van de Woestijne) method
//! with SHA-256 via the `bn254_hash2curve` crate (compatible with gnark-crypto).
//!
//! **Compression:** Uses arkworks canonical compressed serialization (not RFC 9380 format).
//! G1 points are 32 bytes, G2 points are 64 bytes.
//!
//! **Security:** BN-254 provides ~100-bit security (post-exTNFS). Sufficient for most
//! applications. Chosen over BLS12-381 for smaller keys (32B vs 48B) and signatures (64B vs 96B),
//! and better ecosystem compatibility (Ethereum precompiles, zkSNARKs).

use ark_bn254::{Bn254, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::{pairing::Pairing, CurveGroup, Group};
use ark_ff::{UniformRand, Zero, One};
use rand::rngs::OsRng;
use ark_serialize::CanonicalSerialize;
use std::io::Cursor;
use serde::{ser::{Serializer, SerializeTuple}, de::{Deserializer, Visitor, SeqAccess, Error as DeError}};
use bn254_hash2curve::hash2g2::HashToG2;

// RFC 9380 compliant domain separation tag for BN254 G2
const DST: &[u8] = b"QUUX-V01-CS02-with-BN254G2_XMD:SHA-256_SVDW_RO_";

/// Compressed G1 point size in bytes (arkworks BN-254 canonical format)
pub const G1_SIZE: usize = 32;
/// Compressed G2 point size in bytes (arkworks BN-254 canonical format)
pub const G2_SIZE: usize = 64;

// -----------------------------------------------------------------------------
// Wrapper types (compressed on the wire)
// -----------------------------------------------------------------------------

/// Compressed G1 point (32 bytes, ark canonical compressed).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct G1(pub [u8; G1_SIZE]);

impl Default for G1 {
    fn default() -> Self {
        Self::from_affine(&G1Projective::generator().into())
    }
}

impl core::fmt::Debug for G1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "G1({})", self.to_hex())
    }
}

impl G1 {
    pub fn from_affine(p: &G1Affine) -> Self {
        let mut v = Vec::new();
        p.serialize_compressed(&mut v).expect("serialize");
        let mut bytes = [0u8; G1_SIZE];
        bytes.copy_from_slice(&v);
        Self(bytes)
    }

    pub fn to_affine(&self) -> Option<G1Affine> {
        use ark_serialize::CanonicalDeserialize;
        let mut cursor = Cursor::new(&self.0[..]);
        G1Affine::deserialize_compressed(&mut cursor).ok()
    }

    /// Parse a G1 point from a 64-character hex string.
    ///
    /// # Arguments
    /// * `hex_str` - A 64-character hex string representing 32 bytes
    ///
    /// # Returns
    /// * `Ok(G1)` if the hex is valid and 32 bytes
    /// * `Err(&'static str)` with a descriptive error message otherwise
    pub fn from_hex(hex_str: &str) -> Result<Self, &'static str> {
        let bytes = hex::decode(hex_str).map_err(|_| "invalid hex")?;
        let arr: [u8; G1_SIZE] = bytes.try_into().map_err(|_| "expected 32 bytes")?;
        Ok(Self(arr))
    }

    /// Encode this G1 point as a 64-character hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl serde::Serialize for G1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            // JSON: serialize as hex string
            serializer.serialize_str(&self.to_hex())
        } else {
            // Binary (bincode): serialize as byte tuple
            let mut tup = serializer.serialize_tuple(G1_SIZE)?;
            for b in &self.0 {
                tup.serialize_element(b)?;
            }
            tup.end()
        }
    }
}

impl<'de> serde::Deserialize<'de> for G1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            // JSON: deserialize from hex string
            struct G1HexVisitor;
            impl<'de> Visitor<'de> for G1HexVisitor {
                type Value = G1;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "64-character hex string for G1")
                }
                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: DeError,
                {
                    G1::from_hex(v).map_err(|e| E::custom(e))
                }
            }
            deserializer.deserialize_str(G1HexVisitor)
        } else {
            // Binary (bincode): deserialize from byte tuple
            struct G1BinaryVisitor;
            impl<'de> Visitor<'de> for G1BinaryVisitor {
                type Value = G1;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "{}-byte compressed G1", G1_SIZE)
                }
                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let mut arr = [0u8; G1_SIZE];
                    for (i, byte) in arr.iter_mut().enumerate() {
                        *byte = seq.next_element::<u8>()?.ok_or_else(|| A::Error::invalid_length(i, &self))?;
                    }
                    Ok(G1(arr))
                }
            }
            deserializer.deserialize_tuple(G1_SIZE, G1BinaryVisitor)
        }
    }
}

/// Compressed G2 point (64-byte canonical serialization from ark-serialize).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct G2(pub [u8; G2_SIZE]);

impl Default for G2 {
    fn default() -> Self {
        Self([0u8; G2_SIZE])
    }
}

impl core::fmt::Debug for G2 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "G2({}..)", hex::encode(&self.0[..4]))
    }
}

impl G2 {
    pub fn from_affine(p: &G2Affine) -> Self {
        let mut v = Vec::with_capacity(G2_SIZE);
        p.serialize_compressed(&mut v).expect("serialize");
        let mut bytes = [0u8; G2_SIZE];
        bytes.copy_from_slice(&v[..G2_SIZE]);
        Self(bytes)
    }

    pub fn to_affine(&self) -> Option<G2Affine> {
        use ark_serialize::CanonicalDeserialize;
        let mut cursor = Cursor::new(&self.0[..]);
        G2Affine::deserialize_compressed(&mut cursor).ok()
    }

    /// Parse a G2 point from a 128-character hex string.
    pub fn from_hex(hex_str: &str) -> Result<Self, &'static str> {
        let bytes = hex::decode(hex_str).map_err(|_| "invalid hex")?;
        let arr: [u8; G2_SIZE] = bytes.try_into().map_err(|_| "expected 64 bytes")?;
        Ok(Self(arr))
    }

    /// Encode this G2 point as a 128-character hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl serde::Serialize for G2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            // JSON: serialize as hex string
            serializer.serialize_str(&self.to_hex())
        } else {
            // Binary (bincode): serialize as byte tuple
            let mut tup = serializer.serialize_tuple(G2_SIZE)?;
            for b in &self.0 {
                tup.serialize_element(b)?;
            }
            tup.end()
        }
    }
}

impl<'de> serde::Deserialize<'de> for G2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            // JSON: deserialize from hex string
            struct G2HexVisitor;
            impl<'de> Visitor<'de> for G2HexVisitor {
                type Value = G2;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "128-character hex string for G2")
                }
                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: DeError,
                {
                    G2::from_hex(v).map_err(|e| E::custom(e))
                }
            }
            deserializer.deserialize_str(G2HexVisitor)
        } else {
            // Binary (bincode): deserialize from byte tuple
            struct G2BinaryVisitor;
            impl<'de> Visitor<'de> for G2BinaryVisitor {
                type Value = G2;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "{}-byte compressed G2", G2_SIZE)
                }
                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let mut arr = [0u8; G2_SIZE];
                    for (i, byte) in arr.iter_mut().enumerate() {
                        *byte = seq.next_element::<u8>()?.ok_or_else(|| A::Error::invalid_length(i, &self))?;
                    }
                    Ok(G2(arr))
                }
            }
            deserializer.deserialize_tuple(G2_SIZE, G2BinaryVisitor)
        }
    }
}

// -----------------------------------------------------------------------------
// Keys & signatures
// -----------------------------------------------------------------------------

/// Secret key (scalar in Fr).
///
/// BLS secret keys are field elements in Fr (the scalar field of BN-254).
/// They should be generated securely using a cryptographic RNG.
///
/// Serialization format: 32-byte little-endian representation via `into_bigint().to_bytes_le()`.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct SecretKey(pub Fr);

/// Generate a new random secret key using the provided RNG.
pub fn rand_sk(rng: &mut OsRng) -> SecretKey {
    SecretKey(Fr::rand(rng))
}

impl SecretKey {
    /// Generate a new random secret key using `OsRng`.
    pub fn random() -> Self {
        rand_sk(&mut OsRng)
    }

    /// Derive the public key (G₁ point) from this secret key.
    ///
    /// Returns the compressed G1_SIZE-byte representation of `sk * G₁_generator`.
    pub fn public_key(&self ) -> [u8; G1_SIZE] {
        G1::from_affine(&(G1Projective::generator() * self.0).into_affine()).0
    }
}

// -----------------------------------------------------------------------------
// Hash-to-curve (RFC 9380 SVDW compliant)
// -----------------------------------------------------------------------------

/// Hash a message to a G₂ curve point using RFC 9380 SVDW method.
///
/// Uses the Shallue-van de Woestijne (SVDW) algorithm with SHA-256 and the
/// standard RFC 9380 domain separation tag for BN254 G2.
///
/// This implementation is compatible with gnark-crypto and provides uniform
/// distribution over the G₂ curve with proper cofactor clearing.
fn hash_to_g2(msg: &[u8]) -> G2Projective {
    // Use RFC 9380 compliant hash-to-curve implementation
    // Returns G2Affine from bn254_hash2curve, convert to G2Projective
    let g2_affine = HashToG2(msg, DST);
    g2_affine.into()
}

// -----------------------------------------------------------------------------
// BLS primitives
// -----------------------------------------------------------------------------

/// Sign a message using BLS signatures.
///
/// Computes σ = H(msg)^{sk} where H is the RFC 9380 hash-to-curve function.
///
/// # Arguments
/// * `sk` - The secret key
/// * `msg` - The message bytes to sign
///
/// # Returns
/// A 64-byte compressed G₂ signature
pub fn sign(sk: &SecretKey, msg: &[u8]) -> G2 {
    let sig = hash_to_g2(msg) * sk.0;
    G2::from_affine(&sig.into_affine())
}

/// Verify a single BLS signature.
///
/// Checks that e(G₁_gen, σ) == e(P, H(m)) using the BN-254 pairing.
///
/// # Arguments
/// * `pk` - The public key (compressed G₁ point)
/// * `msg` - The message that was signed
/// * `sig` - The signature to verify
///
/// # Returns
/// `true` if the signature is valid, `false` otherwise
pub fn verify(pk: &G1, msg: &[u8], sig: &G2) -> bool {
    let pk_affine = match pk.to_affine() {
        Some(p) => p,
        None => return false,
    };
    let sig_affine = match sig.to_affine() {
        Some(s) => s,
        None => return false,
    };
    let h_affine = hash_to_g2(msg).into_affine();

    Bn254::pairing(G1Projective::generator().into_affine(), sig_affine)
        == Bn254::pairing(pk_affine, h_affine)
}

/// Aggregate multiple BLS signatures into a single signature.
///
/// Computes Σ = σ₁ + σ₂ + ... + σₙ (point addition in G₂).
///
/// # Arguments
/// * `sigs` - Iterator of signatures to aggregate
///
/// # Returns
/// A single aggregated signature that can be verified against multiple (pk, msg) pairs
pub fn aggregate<'a, I>(sigs: I) -> G2
where
    I: IntoIterator<Item = &'a G2>,
{
    let mut acc = G2Projective::zero();
    for s in sigs {
        if let Some(a) = s.to_affine() {
            acc += a;
        }
    }
    G2::from_affine(&acc.into_affine())
}

/// Verify an aggregate BLS signature.
///
/// Checks that e(G₁_gen, Σ) == ∏ e(Pᵢ, H(mᵢ)) for all (pk, msg) pairs.
///
/// # Arguments
/// * `pairs` - Iterator of (public_key, message) pairs
/// * `sigma` - The aggregated signature
///
/// # Returns
/// `true` if the aggregate signature is valid for all pairs, `false` otherwise
///
/// # Security Note
/// This function does NOT check for duplicate public keys or messages.
/// Callers must ensure no key reuse across different messages (rogue key attack).
pub fn verify_aggregate<'a, I>(pairs: I, sigma: &G2) -> bool
where
    I: IntoIterator<Item = (&'a G1, &'a [u8])>,
{
    let sigma_affine = match sigma.to_affine() {
        Some(s) => s,
        None => return false,
    };

    let mut prod = <Bn254 as Pairing>::TargetField::one();
    for (pk, msg) in pairs {
        let pk_affine = match pk.to_affine() {
            Some(p) => p,
            None => return false,
        };
        let h_affine = hash_to_g2(msg).into_affine();
        let e = Bn254::pairing(pk_affine, h_affine);
        prod *= &e.0;
    }

    let lhs = Bn254::pairing(G1Projective::generator().into_affine(), sigma_affine);
    lhs.0 == prod
} 