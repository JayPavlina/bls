//! ## Adaptation of `pairing::Engine` to BLS-like signatures.
//!
//! We provide an `EngineBLS` trait that adapts `pairing::Engine`
//! to BLS-like signatures by permitting the group roles to be
//! transposed, which involves removing the field of definition, 
//! while retaining the correct associations.  
//! 
//! We support same-message aggregation strategies using wrappers
//! that satisfy `EngineBLS` as well, primarily because these
//! strategies must ocntroll access to the public key type.
//!
//! In future, we should support [Pixel](https://github.com/w3f/bls/issues/4)
//! by adding wrapper that replace `SignatureGroup` with a product
//! of both groups.  I think this requires abstracting `CruveAffine`
//! and `CruveProjective` without their base fields and wNAF windows,
//! but still with their affine, projective, and compressed forms,
//! and batch normalization. 


use std::borrow::Borrow;

use ff::{Field, PrimeField, ScalarEngine, SqrtField}; // PrimeFieldDecodingError, PrimeFieldRepr
use pairing::{CurveAffine, CurveProjective, Engine};
use rand::{Rand, Rng};


/// A weakening of `pairing::Engine` to permit transposing the groups.
///
/// You cannot transpose the two groups in a `pairing::Engine` without
/// first providing panicing implementations of `pairing::PrimeField`
/// for `Engine::Fqe`, which is not a prime field, and second,
/// providing wrapper types for the projective and affine group 
/// representations, which makes interacting with the original
/// `pairing::Engine` annoying.  This trait merely replicates
/// transposable functionality from `pairing::Engine` by removing
/// the fields of definition, but leaves the actual BLS signature
/// scheme to wrapper types.
///
/// We also extract two functions users may with to override:
/// random scalar generation and hashing to the singature curve.
pub trait EngineBLS {
    type Engine: Engine + ScalarEngine<Fr = Self::Scalar>;
    type Scalar: PrimeField + SqrtField; // = <Self::Engine as ScalarEngine>::Fr;

    /// Group where BLS public keys live
    /// 
    /// You should take this to be the `Engine::G1` curve usually
    /// becuase all verifiers perform additions on this curve, or
    /// even scalar multiplicaitons with delinearization.
    type PublicKeyGroup: 
        CurveProjective<Engine = Self::Engine, Scalar = Self::Scalar>
        + Into<<Self::PublicKeyGroup as CurveProjective>::Affine>;

    /// Group where BLS signatures live
    ///
    /// You should take this to be the `Engine::G2` curve usually
    /// becuase only aggregators perform additions on this curve, or
    /// scalar multiplicaitons with delinearization.
    type SignatureGroup: 
        CurveProjective<Engine = Self::Engine, Scalar = Self::Scalar>
        + Into<<Self::SignatureGroup as CurveProjective>::Affine>;

    /// Generate a random scalar for use as a secret key.
    fn generate<R: Rng>(rng: &mut R) -> Self::Scalar {
        Self::Scalar::rand(rng)
    }

    /// Hash one message to the signature curve.
    fn hash_to_signature_curve<M: Borrow<[u8]>>(message: M) -> Self::SignatureGroup {
        <Self::SignatureGroup as CurveProjective>::hash(message.borrow())
    }

    /// Run the Miller loop from `Engine` but orients its arguments
    /// to be a `SignatureGroup` and `PublicKeyGroup`.
    fn miller_loop<'a,I>(i: I) -> <Self::Engine as Engine>::Fqk
    where
        I: IntoIterator<Item = (
            &'a <<Self::PublicKeyGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
            &'a <<Self::SignatureGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
        )>;

    /// Perform final exponentiation on the result of a Miller loop.
    fn final_exponentiation(e: &<Self::Engine as Engine>::Fqk) -> Option<<Self::Engine as Engine>::Fqk> {
        Self::Engine::final_exponentiation(e)
    }

    /// Performs a pairing operation `e(p, q)` by calling `Engine::pairing`
    /// but orients its arguments to be a `PublicKeyGroup` and `SignatureGroup`.
    fn pairing<G1,G2>(p: G1, q: G2) -> <Self::Engine as Engine>::Fqk
    where
        G1: Into<<Self::PublicKeyGroup as CurveProjective>::Affine>,
        G2: Into<<Self::SignatureGroup as CurveProjective>::Affine>;
    /*
    {
        Self::final_exponentiation(&Self::miller_loop(
            [(&(p.into().prepare()), &(q.into().prepare()))].into_iter(),
        )).unwrap()
    }
    */

    /// Implement verification equation for aggregate BLS signatures
    /// provided as prepared points
    /// 
    /// This low-level routine does no verification of critical security
    /// properties like message distinctness.  It exists purely to
    /// simplify replacing mid-level routines with optimized variants,
    /// like versions that cache public key preperation or use fewer pairings. 
    fn verify_prepared<'a,I>(
        signature: &'a <<Self::SignatureGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
        inputs: I
      ) -> bool
    where
        I: IntoIterator<Item = (
            &'a <<Self::PublicKeyGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
            &'a <<Self::SignatureGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
        )>
    {
        // Use a polymorphic static or const if we ever get either. 
        let mut g1_minus_generator = <Self::PublicKeyGroup as CurveProjective>::Affine::one();
        g1_minus_generator.negate();
        Self::final_exponentiation( & Self::miller_loop(
            inputs.into_iter().map(|t| t)  // reborrow hack
                .chain(::std::iter::once( (& g1_minus_generator.prepare(), signature) ))
        ) ).unwrap() == <Self::Engine as Engine>::Fqk::one()
    }
}


/// Usual aggregate BLS signature scheme on ZCash's BLS12-381 curve.
pub type ZBLS = UsualBLS<::pairing::bls12_381::Bls12>;

/// Usual aggregate BLS signature scheme on ZCash's BLS12-381 curve.
pub const Z_BLS : ZBLS = UsualBLS(::pairing::bls12_381::Bls12);


/// Usual BLS variant with tiny 48 byte public keys and 96 byte signatures.
///
/// We favor this variant because verifiers always perform
/// `O(signers)` additions on the `PublicKeyGroup`, or worse 128 bit
/// scalar multiplications with delinearization. 
/// We also orient this variant to match zcash's traits.
#[derive(Default)]
pub struct UsualBLS<E: Engine>(pub E);

impl<E: Engine> EngineBLS for UsualBLS<E> {
    type Engine = E;
    type Scalar = <Self::Engine as ScalarEngine>::Fr;
    type PublicKeyGroup = E::G1;
    type SignatureGroup = E::G2;

    fn miller_loop<'a,I>(i: I) -> E::Fqk
    where
        I: IntoIterator<Item = (
            &'a <E::G1Affine as CurveAffine>::Prepared,
            &'a <E::G2Affine as CurveAffine>::Prepared,
        )>,
    {
        // We require an ugly unecessary allocation here because
        // zcash's pairing library cnsumes an iterator of references
        // to tuples of references, which always requires 
        let i = i.into_iter().map(|t| t)
              .collect::<Vec<(&<E::G1Affine as CurveAffine>::Prepared,&<E::G2Affine as CurveAffine>::Prepared)>>();
        E::miller_loop(&i)
    }

    fn pairing<G1,G2>(p: G1, q: G2) -> E::Fqk
    where
        G1: Into<E::G1Affine>,
        G2: Into<E::G2Affine>,
    {
        E::pairing(p,q)
    }
}


/// Infrequently used BLS variant with tiny 48 byte signatures and 96 byte public keys,
///
/// We recommend gainst this variant by default because verifiers
/// always perform `O(signers)` additions on the `PublicKeyGroup`,
/// or worse 128 bit scalar multiplications with delinearization. 
/// Yet, there are specific use cases where this variant performs
/// better.  We swapy two group roles relative to zcash here.
#[derive(Default)]
pub struct TinyBLS<E: Engine>(pub E);

impl<E: Engine> EngineBLS for TinyBLS<E> {
    type Engine = E;
    type Scalar = <Self::Engine as ScalarEngine>::Fr;
    type PublicKeyGroup = E::G2;
    type SignatureGroup = E::G1;

    fn miller_loop<'a,I>(i: I) -> E::Fqk
    where
        I: IntoIterator<Item = (
            &'a <E::G2Affine as CurveAffine>::Prepared,
            &'a <E::G1Affine as CurveAffine>::Prepared,
        )>,
    {
        // We require an ugly unecessary allocation here because
        // zcash's pairing library cnsumes an iterator of references
        // to tuples of references, which always requires 
        let i = i.into_iter().map(|(x,y)| (y,x))
              .collect::<Vec<(&<E::G1Affine as CurveAffine>::Prepared,&<E::G2Affine as CurveAffine>::Prepared)>>();
        E::miller_loop(&i)
    }

    fn pairing<G2,G1>(p: G2, q: G1) -> E::Fqk
    where
        G1: Into<E::G1Affine>,
        G2: Into<E::G2Affine>,
    {
        E::pairing(q,p)
    }
}


/// Rogue key attack defence by proof-of-possession
#[derive(Default)]
pub struct PoP<E>(pub E);

impl<E: EngineBLS> EngineBLS for PoP<E> {
    type Engine = E::Engine;
    type Scalar = <Self::Engine as ScalarEngine>::Fr;
    type PublicKeyGroup = E::PublicKeyGroup;
    type SignatureGroup = E::SignatureGroup;

    fn miller_loop<'a,I>(i: I) -> <Self::Engine as Engine>::Fqk
    where
        I: IntoIterator<Item = (
            &'a <<Self::PublicKeyGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
            &'a <<Self::SignatureGroup as CurveProjective>::Affine as CurveAffine>::Prepared,
        )>,
    {
        E::miller_loop(i)
    }

    fn pairing<G1,G2>(p: G1, q: G2) -> <E::Engine as Engine>::Fqk
    where
        G1: Into<<Self::PublicKeyGroup as CurveProjective>::Affine>,
        G2: Into<<Self::SignatureGroup as CurveProjective>::Affine>,
    {
        E::pairing(p,q)
    }
}


/// Any `EngineBLS` whose keys remain unmutated.
///
/// We mutate delinearized public keys when loading them, so they
/// cannot be serialized or deserialized directly.  Instead, you
/// should interact with the keys using the base `EngineBLS` and call
/// `delinearize` before signing or verifying.
pub trait UnmutatedKeys : EngineBLS {}

impl<E: Engine> UnmutatedKeys for TinyBLS<E> {}
impl<E: Engine> UnmutatedKeys for UsualBLS<E> {}
impl<E: EngineBLS> UnmutatedKeys for PoP<E> {}

/// Any `EngineBLS` whose keys can be trivially deserlialized.
/// 
/// We disallow deserlialization for proof-of-possession, so that
/// developers must call `i_have_checked_this_proof_of_possession`.
pub trait DeserializePublicKey : EngineBLS+UnmutatedKeys {}

impl<E: Engine> DeserializePublicKey for TinyBLS<E> {}
impl<E: Engine> DeserializePublicKey for UsualBLS<E> {}


