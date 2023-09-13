//! Pedersen hash module.
use alloc::vec::Vec;

use starknet_core::crypto::compute_hash_on_elements;
use starknet_crypto::{pedersen_hash, FieldElement};

use crate::execution::felt252_wrapper::Felt252Wrapper;
use crate::traits::hash::HasherT;
use crate::traits::SendSyncStatic;

/// The Pedersen hasher.
#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "parity-scale-codec", derive(parity_scale_codec::Encode, parity_scale_codec::Decode))]
#[cfg_attr(feature = "scale-info", derive(scale_info::TypeInfo))]
pub struct PedersenHasher;

impl SendSyncStatic for PedersenHasher {}

/// The Pedersen hasher implementation.
impl HasherT for PedersenHasher {
    /// The Pedersen hash function.
    /// # Arguments
    /// * `data` - The data to hash.
    /// # Returns
    /// The hash of the data.
    fn hash_bytes(data: &[u8]) -> Felt252Wrapper {
        // For now we use the first 31 bytes of the data as the field element, to avoid any panics.
        // TODO: have proper error handling and think about how to hash efficiently big chunks of data.
        let field_element = FieldElement::from_byte_slice_be(&data[..31]).unwrap();
        Felt252Wrapper(pedersen_hash(&FieldElement::ZERO, &field_element))
    }

    /// Hashes a slice of field elements using the Pedersen hash function.
    ///
    /// # Arguments
    ///
    /// * `data` - The data to hash.
    ///
    /// # Returns
    ///
    /// The hash of the data.
    fn compute_hash_on_wrappers(data: &[Felt252Wrapper]) -> Felt252Wrapper {
        let hash = compute_hash_on_elements(&data.iter().map(|x| x.0).collect::<Vec<FieldElement>>());
        Felt252Wrapper(hash)
    }

    #[inline(always)]
    fn hash_elements(a: FieldElement, b: FieldElement) -> FieldElement {
        pedersen_hash(&a, &b)
    }

    /// Compute hash on elements, taken from [starknet-rs](https://github.com/xJonathanLEI/starknet-rs/blob/master/starknet-core/src/crypto.rs#L25) pending a no_std support.
    ///
    /// # Arguments
    ///
    /// * `elements` - The elements to hash.
    ///
    /// # Returns
    ///
    /// h(h(h(h(0, data\[0\]), data\[1\]), ...), data\[n-1\]), n).
    #[inline]
    fn compute_hash_on_elements(elements: &[FieldElement]) -> FieldElement {
        compute_hash_on_elements(elements)
    }
}
