use frame_support::assert_ok;
use mp_starknet::execution::types::Felt252Wrapper;
use mp_starknet::transaction::UserTransaction;

use super::mock::default_mock::*;
use super::mock::*;
use crate::tests::{get_invoke_dummy, get_storage_read_write_dummy};
#[test]
fn estimates_tx_fee_successfully() {
    new_test_ext::<MockRuntime>().execute_with(|| {
        basic_test_setup(2);

        let tx = get_invoke_dummy(Felt252Wrapper::ZERO);
        let tx = UserTransaction::Invoke(tx.into());

        let (actual, l1_gas_usage) = Starknet::estimate_fee(tx).unwrap();
        assert!(actual > 0, "actual fee is missing");
        assert!(l1_gas_usage == 0, "this should not be charged any l1_gas as it does not store nor send messages");

        let tx = get_storage_read_write_dummy();
        let tx = UserTransaction::Invoke(tx.into());

        let (actual, l1_gas_usage) = Starknet::estimate_fee(tx).unwrap();
        assert!(actual > 0, "actual fee is missing");
        assert!(l1_gas_usage > 0, "this should be charged l1_gas as it store a value to storage");
    });
}

#[test]
fn estimates_tx_fee_with_query_version() {
    new_test_ext::<MockRuntime>().execute_with(|| {
        basic_test_setup(2);

        let tx = get_invoke_dummy(Felt252Wrapper::ZERO);
        let pre_storage = Starknet::pending().len();
        let tx = UserTransaction::Invoke(tx.into());

        assert_ok!(Starknet::estimate_fee(tx));

        assert!(pre_storage == Starknet::pending().len(), "estimate should not add a tx to pending");
    });
}
