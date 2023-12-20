use alloc::vec::Vec;

use mp_messages::conversions::eth_address_to_felt;
use mp_messages::{MessageL1ToL2, MessageL2ToL1};
use starknet_api::api_core::{ContractAddress, Nonce, PatriciaKey};
use starknet_api::hash::StarkFelt;

use crate::felt_reader::{FeltReader, FeltReaderError};
use crate::StarknetOsOutput;

/// This codec allows to convert structured OS program output into array of felts
///
/// In order to prepare parameters for the Starknet contract `updateState` method:
///     1. Cast the output to dynamic uint256[] array
///     2. Get onchain data hash & size
///     3. ABI encode parameters
///  
/// NOTE: Field element (252 bit) is encoded as an EVM word (256 bit) and vice versa
/// EVM developer should be aware of that and prevent data loss by not using the higest 4 bits
pub trait SnosCodec: Sized {
    /// Return an estimation of the number of field elements required to encode `self`
    ///
    /// This is to be used for allocating the correct amount of memory before encoding.
    /// It's for optimization purpose (avoiding reallocation) so it's implementation should be
    /// efficient (no iteration, no IO, no other allocation, no expensive computation).
    fn size_in_felts(&self) -> usize;
    /// Encodes current snos output field as felt array and appends to the result
    fn encode_to(self, output: &mut Vec<StarkFelt>);
    /// Tries to decode snos output field given a felt reader instance
    fn decode(input: &mut FeltReader) -> Result<Self, FeltReaderError>;
    /// Converts structured snos program output into array of field elements
    fn into_encoded_vec(self) -> Vec<StarkFelt> {
        let mut output: Vec<StarkFelt> = Vec::with_capacity(self.size_in_felts());
        self.encode_to(&mut output);
        output
    }
}

impl SnosCodec for StarkFelt {
    fn size_in_felts(&self) -> usize {
        1
    }

    fn encode_to(self, output: &mut Vec<StarkFelt>) {
        output.push(self);
    }

    fn decode(input: &mut FeltReader) -> Result<Self, FeltReaderError> {
        input.read()
    }
}

impl<T: SnosCodec> SnosCodec for Vec<T> {
    fn size_in_felts(&self) -> usize {
        // Works well for Vec<StarkFelt>
        // Works less well for Vec<Message>, but it just mean there will be a realocation
        // Nothing terrible, and still better than iterating
        1 + self.len()
    }

    fn encode_to(self, output: &mut Vec<StarkFelt>) {
        let segment_size = self.size_in_felts() as u64;
        output.push(segment_size.into());
        for elt in self.into_iter() {
            elt.encode_to(output);
        }
    }

    fn decode(input: &mut FeltReader) -> Result<Self, FeltReaderError> {
        let mut segment_reader = FeltReader::new(input.read_segment()?);
        let mut elements: Vec<T> = Vec::new();
        while segment_reader.remaining_len() > 0 {
            elements.push(T::decode(&mut segment_reader)?);
        }
        Ok(elements)
    }
}

impl SnosCodec for MessageL2ToL1 {
    fn size_in_felts(&self) -> usize {
        3
    }

    fn encode_to(self, output: &mut Vec<StarkFelt>) {
        output.push(self.from_address.0.0);
        output.push(eth_address_to_felt(&self.to_address));
        self.payload.encode_to(output);
    }

    fn decode(input: &mut FeltReader) -> Result<Self, FeltReaderError> {
        Ok(Self {
            from_address: ContractAddress(PatriciaKey(StarkFelt::decode(input)?)),
            to_address: StarkFelt::decode(input)?.try_into().map_err(|_| FeltReaderError::InvalidCast)?,
            payload: Vec::<StarkFelt>::decode(input)?,
        })
    }
}

impl SnosCodec for MessageL1ToL2 {
    fn size_in_felts(&self) -> usize {
        4 + self.payload.size_in_felts()
    }

    fn encode_to(self, output: &mut Vec<StarkFelt>) {
        output.push(self.from_address.0.0);
        output.push(self.to_address.0.0);
        output.push(self.nonce.0);
        output.push(self.selector);
        self.payload.encode_to(output);
    }

    fn decode(input: &mut FeltReader) -> Result<Self, FeltReaderError> {
        Ok(Self {
            from_address: ContractAddress(PatriciaKey(StarkFelt::decode(input)?)),
            to_address: ContractAddress(PatriciaKey(StarkFelt::decode(input)?)),
            nonce: Nonce(StarkFelt::decode(input)?),
            selector: StarkFelt::decode(input)?,
            payload: Vec::<StarkFelt>::decode(input)?,
        })
    }
}

impl SnosCodec for StarknetOsOutput {
    fn size_in_felts(&self) -> usize {
        5 + self.messages_to_l1.size_in_felts()
            + self.messages_to_l2.size_in_felts()
            + self.state_updates.len()
            + self.contract_class_diff.len()
    }

    fn encode_to(self, output: &mut Vec<StarkFelt>) {
        output.push(self.prev_state_root);
        output.push(self.new_state_root);
        output.push(self.block_number);
        output.push(self.block_hash);
        output.push(self.config_hash);
        self.messages_to_l1.encode_to(output);
        self.messages_to_l2.encode_to(output);
        self.state_updates.encode_to(output);
        self.contract_class_diff.encode_to(output);
    }

    fn decode(input: &mut FeltReader) -> Result<Self, FeltReaderError> {
        Ok(Self {
            prev_state_root: StarkFelt::decode(input)?,
            new_state_root: StarkFelt::decode(input)?,
            block_number: StarkFelt::decode(input)?,
            block_hash: StarkFelt::decode(input)?,
            config_hash: StarkFelt::decode(input)?,
            messages_to_l1: Vec::<MessageL2ToL1>::decode(input)?,
            messages_to_l2: Vec::<MessageL1ToL2>::decode(input)?,
            state_updates: Vec::<StarkFelt>::decode(input)?,
            contract_class_diff: Vec::<StarkFelt>::decode(input)?,
        })
    }
}
