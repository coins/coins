//! Cryptography layer – minimal-public-key BLS on the BN-254 curve.
//!
//! • Public key: G₁ point (32-byte compressed)
//! • Signature : σ = H(msg)^{sk} ∈ G₂ (64-byte compressed)
//!
//! Verify single:   e(G₁_gen, σ) == e(P, H(m))
//! Verify aggregate: e(G₁_gen, Σ) == ∏ e(Pᵢ, H(mᵢ))
//!
//! WARNING This is a **demo-quality** implementation!  Hash-to-curve is a very
//! simplified Blake2s-into-scalar approach and does NOT follow RFC 9380.  The
//! compression format is ad-hoc (x-only + sign bit).  Do NOT use in production.

use ark_bn254::{Bn254, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::{pairing::Pairing, CurveGroup, Group};
use ark_ff::{PrimeField, UniformRand, Zero, One};
use blake2::{Blake2s256, Digest};
use rand::rngs::OsRng;
use ark_serialize::CanonicalSerialize;
use std::io::Cursor;
use serde::{ser::{Serializer, SerializeTuple}, de::{Deserializer, Visitor, SeqAccess, Error as DeError}};

const DST: &[u8] = b"EC-TOKEN"; // domain-sep tag

// -----------------------------------------------------------------------------
// Wrapper types (compressed on the wire)
// -----------------------------------------------------------------------------

/// Compressed G1 point (32 bytes, ark canonical compressed).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct G1(pub [u8; 32]);

impl Default for G1 {
    fn default() -> Self {
        Self::from_affine(&G1Projective::generator().into())
    }
}

impl core::fmt::Debug for G1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "G1({})", hex::encode(self.0))
    }
}

impl G1 {
    pub fn from_affine(p: &G1Affine) -> Self {
        let mut v = Vec::new();
        p.serialize_compressed(&mut v).expect("serialize");
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&v);
        Self(bytes)
    }

    pub fn to_affine(&self) -> Option<G1Affine> {
        use ark_serialize::CanonicalDeserialize;
        let mut cursor = Cursor::new(&self.0[..]);
        G1Affine::deserialize_compressed(&mut cursor).ok()
    }
}

impl serde::Serialize for G1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tup = serializer.serialize_tuple(32)?;
        for b in &self.0 {
            tup.serialize_element(b)?;
        }
        tup.end()
    }
}

impl<'de> serde::Deserialize<'de> for G1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct G1Visitor;
        impl<'de> Visitor<'de> for G1Visitor {
            type Value = G1;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "32-byte compressed G1")
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut arr = [0u8; 32];
                for i in 0..32 {
                    arr[i] = seq.next_element::<u8>()?.ok_or_else(|| A::Error::invalid_length(i, &self))?;
                }
                Ok(G1(arr))
            }
        }
        deserializer.deserialize_tuple(32, G1Visitor)
    }
}

/// Compressed G2 point (64-byte canonical serialization from ark-serialize).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct G2(pub [u8; 64]);

impl Default for G2 {
    fn default() -> Self {
        Self([0u8; 64])
    }
}

impl core::fmt::Debug for G2 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "G2({}..)", hex::encode(&self.0[..4]))
    }
}

impl G2 {
    pub fn from_affine(p: &G2Affine) -> Self {
        let mut v = Vec::with_capacity(64);
        p.serialize_compressed(&mut v).expect("serialize");
        let mut bytes = [0u8; 64];
        bytes.copy_from_slice(&v[..64]);
        Self(bytes)
    }

    pub fn to_affine(&self) -> Option<G2Affine> {
        use ark_serialize::CanonicalDeserialize;
        let mut cursor = Cursor::new(&self.0[..]);
        G2Affine::deserialize_compressed(&mut cursor).ok()
    }
}

impl serde::Serialize for G2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tup = serializer.serialize_tuple(64)?;
        for b in &self.0 {
            tup.serialize_element(b)?;
        }
        tup.end()
    }
}

impl<'de> serde::Deserialize<'de> for G2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct G2Visitor;
        impl<'de> Visitor<'de> for G2Visitor {
            type Value = G2;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "64-byte compressed G2")
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut arr = [0u8; 64];
                for i in 0..64 {
                    arr[i] = seq.next_element::<u8>()?.ok_or_else(|| A::Error::invalid_length(i, &self))?;
                }
                Ok(G2(arr))
            }
        }
        deserializer.deserialize_tuple(64, G2Visitor)
    }
}

// -----------------------------------------------------------------------------
// Keys & signatures
// -----------------------------------------------------------------------------

/// Secret key (scalar in Fr).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct SecretKey(pub Fr);

/// Generate a new random secret key.
pub fn rand_sk(rng: &mut OsRng) -> SecretKey {
    SecretKey(Fr::rand(rng))
}

impl SecretKey {
    pub fn random() -> Self {
        rand_sk(&mut OsRng)
    }

    pub fn public_key(&self ) -> [] {
        G1::from_affine((G1Projective::generator() * self.0).into_affine()).0
    }
}

// -----------------------------------------------------------------------------
// Hash-to-curve (very simplified)
// -----------------------------------------------------------------------------

fn hash_to_g2(msg: &[u8]) -> G2Projective {
    // Blake2s → scalar mod r
    let mut hasher = Blake2s256::new();
    hasher.update(DST);
    hasher.update(msg);
    let digest = hasher.finalize();

    let mut tmp = [0u8; 32];
    tmp.copy_from_slice(&digest);
    let scalar = Fr::from_le_bytes_mod_order(&tmp);
    G2Projective::generator() * scalar
}

// -----------------------------------------------------------------------------
// BLS primitives
// -----------------------------------------------------------------------------

/// σ = H(m)^{sk}
pub fn sign(sk: &SecretKey, msg: &[u8]) -> G2 {
    let sig = hash_to_g2(msg) * sk.0;
    G2::from_affine(&sig.into_affine())
}

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

/// Σ = Σᵢ σᵢ
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