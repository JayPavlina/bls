//! # Aggregate BLS signature library with extensive tuning options. 
//! 
//! In short, anyone using BLS signatures should normally choose both
//! an orientation as well as some aggregation and batching strategies
//! These two decissions impact performance dramaticaly, but making
//! the optimal choises requires some attentiom.  This crate employs
//! convenient abstraction boundaries between curver arithmatic, 
//! verifier routines, and aggregated and/or batched BLS signatures.
//! 
//! ### Pairings
//! 
//! If we have two elliptic curve with a pairing `e`, then
//! a BLS signature `sigma = s*H(msg)` by a public key `S = s g1`
//! can be verified with the one equation `e(g1,sigma) = e(S,H(msg))`.
//! These simple BLS signatures are very slow to verify however
//! because the pairing map `e` is far slower than many cryptographic
//! primitives.
//! 
//! Our pairing `e` maps from a small curve over `F(q)` and a larger
//! curve over `F(q^2)` into some multipliccative group if a field,
//! normally over `F(q^12)`.  In principle, this map `e` into `F(q^12)`
//! makes pairing based cryptography like BLS less secure than
//! other elliptic curve based cryptography, which further slows down
//! BLS signatures by requiring larger `q`.
//!
//! ### Arithmatic
//!
//! An almost universally applicable otimization is to seperate the
//! "Miller loop" that computes in `F(q)` and `F(q^2)` from the slow
//! final exponentiation that happens in `F(q^12)`.  So our actual
//! verification equation more resembles `e(-g1,sigma) e(S,H(msg)) = 1`.
//!
//! As one curve is smaller and hence faster, the user should choose 
//! which orientation of curves they prefer, meaning to which curve
//! they hash, and which curves hold the signatues and public keys.
//! In other words, your desired aggregation techniques and usage 
//! characteristics should determine if youp refer the verification
//! equation `e(g1,sigma) = e(S,H(msg))` or the fliped form
//! `e(sigma,g2) = e(H(msg),S)`.  See `UsualBLS` and `TinyBLS`.
//!
//! ### Aggregation
//!
//! We consder BLS signatures interesting because they support
//! dramatic optimizations when handling multiple signatures together.
//! In fact, BLS signatures support aggregation by a third party
//! that makes signatures smaller, not merely batch verification.  
//! All this stems from the bilinearity of `e`, meaning we reduce
//! the number of pairings, or size of the miller loop, by appling
//! rules like `e(x,z)e(y,z) = e(x+y,z)`, `e(x,y)e(x,z) = e(x,y+z)`,
//! etc.
//!
//! In essence, our aggregation tricks fall into two categories,
//! linear aggregation, in which only addition is used, and
//! delinearized optimiztions, in which we multiply curve points
//! by values unforseeable to the signers.
//! In general, linear techniques provide much better performance,
//! but require stronger invariants be maintained by the caller,
//! like messages being distinct, or limited signer sets with 
//! proofs-of-possession.  Also, the delinearized techniques remain
//! secure without tricky assumptions, but require more computation.
//! 
//! ### Verification
//!
//! We can often further reduce the pairings required in the
//! verification equation, beyond the naieve information tracked
//! by the aggregated signature itself.  Aggregated signature must
//! state all the individual messages and/or public keys, but
//! verifiers may collapse anything permitted. 
//! We thus encounter aggregation-like decissions that impact
//! verifier performance.
//!
//! We therefore provide an abstract interface that permits
//! doing further aggregation and/or passing any aggregate signature
//! to any verification routine.
//!
//! As a rule, we also attempt to batch normalize different arithmatic
//! outputs, but concievably small signer set sizes might make this
//! a pessimization.
//!
//! 
//!


// #![feature(generic_associated_types)]
#![feature(associated_type_defaults)]

#[macro_use]
extern crate arrayref;

// #[macro_use]
extern crate ff;

extern crate paired as pairing;
extern crate rand;
extern crate sha3;

#[cfg(feature = "serde")]
extern crate serde;


use std::borrow::Borrow;


pub mod engine;
pub mod single;
pub mod distinct;
pub mod pop;
pub mod bit;
pub mod delinear;
pub mod verifiers;
// pub mod delinear;

pub use engine::*;

pub use single::{PublicKey,KeypairVT,Keypair,SecretKeyVT,SecretKey,Signature};
pub use bit::{BitSignedMessage,CountSignedMessage};


/// Internal message hash size.  
///
/// We choose 256 bits here so that birthday bound attacks cannot
/// find messages with the same hash.
const MESSAGE_SIZE: usize = 32;

/// Internal message hash type.  Short for frequent rehashing
/// by `HashMap`, etc.
#[derive(Debug,Copy,Clone,Hash,PartialEq,Eq,PartialOrd,Ord)]
pub struct Message(pub [u8; MESSAGE_SIZE]);

impl Message {
    pub fn new(context: &[u8], message: &[u8]) -> Message {
        use sha3::{Shake128, digest::{Input,ExtendableOutput,XofReader}};
        let mut h = Shake128::default();
        h.input(context);
        let l = message.len() as u64;
        h.input(l.to_le_bytes());
        h.input(message);
        // let mut t = ::merlin::Transcript::new(context);
        // t.append_message(b"", message);
        let mut msg = [0u8; MESSAGE_SIZE];
        h.xof_result().read(&mut msg[..]);
        // t.challenge_bytes(b"", &mut msg);
        Message(msg)
    }

    pub fn hash_to_signature_curve<E: EngineBLS>(&self) -> E::SignatureGroup {
        E::hash_to_signature_curve(&self.0[..])
    }
}

impl<'a> From<&'a [u8]> for Message {
    fn from(x: &[u8]) -> Message { Message::new(b"",x) }     
}



/// Representation of an aggregated BLS signature.
///
/// We implement this trait only for borrows of appropriate structs
/// because otherwise we'd need extensive lifetime plumbing here,
/// due to the absence of assocaited type constructers (ATCs).
/// We shall make `messages_and_publickeys` take `&sefl` and
/// remove these limitations in the future once ATCs stabalize,
/// thus removing `PKG`.  See [Rust RFC 1598](https://github.com/rust-lang/rfcs/blob/master/text/1598-generic_associated_types.md)
/// We shall eventually remove MnPK entirely whenever `-> impl Trait`
/// in traits gets stabalized.  See [Rust RFCs 1522, 1951, and 2071](https://github.com/rust-lang/rust/issues/34511
pub trait Signed: Sized {
    type E: EngineBLS;

    /// Return the aggregated signature 
    fn signature(&self) -> Signature<Self::E>;

    type M: Borrow<Message> = Message;
    type PKG: Borrow<PublicKey<Self::E>> = PublicKey<Self::E>;

    /// Iterator over messages and public key reference pairs.
    type PKnM: Iterator<Item = (Self::M,Self::PKG)> + ExactSizeIterator;
    // type PKnM<'a>: Iterator<Item = (
    //    &'a <<Self as Signed<'a>>::E as EngineBLS>::PublicKeyGroup,
    //    &'a Self::M,
    // )> + DoubleEndedIterator + ExactSizeIterator + 'a;

    /// Returns an iterator over messages and public key reference for
    /// pairings, often only partially aggregated. 
    fn messages_and_publickeys(self) -> Self::PKnM;
    // fn messages_and_publickeys<'a>(&'s self) -> PKnM<'a>
    // -> impl Iterator<Item = (&'a Self::M, &'a Self::E::PublicKeyGroup)> + 'a;

    /// Appropriate BLS signature verification for the `Self` type.
    ///
    /// We use `verify_simple` as a default implementation because
    /// it supports unstable `self.messages_and_publickeys()` securely
    /// by calling it only once, and does not expect pulic key points
    /// to be normalized, but this should usually be replaced by more
    /// optimized variants. 
    fn verify(self) -> bool {
        verifiers::verify_simple(self)
    }
}



#[cfg(test)]
mod tests {
    // use super::*;

    // use rand::{SeedableRng, XorShiftRng};

    // #[test]
    // fn foo() { }
}

