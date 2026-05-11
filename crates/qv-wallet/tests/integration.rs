//! Integration tests
#[cfg(test)]
mod tests {
    use qv_wallet::Mnemonic;

    #[test]
    fn test_mnemonic_generation() {
        let m = Mnemonic::generate().expect("gen");
        let phrase = m.phrase();
        assert!(!phrase.is_empty());
        assert_eq!(phrase.split_whitespace().count(), 24);
    }

    #[test]
    fn test_mnemonic_round_trip() {
        let m1 = Mnemonic::generate().expect("gen");
        let phrase = m1.phrase().to_string();
        let m2 = Mnemonic::from_phrase(&phrase).expect("parse");
        assert_eq!(m1.phrase(), m2.phrase());
    }

    #[test]
    fn test_seed_derivation() {
        let m = Mnemonic::generate().expect("gen");
        let seed1 = m.to_seed("").expect("seed1");
        let seed2 = m.to_seed("").expect("seed2");
        assert_eq!(seed1, seed2);
        assert_eq!(seed1.len(), 64);
    }

    #[test]
    fn test_passphrase_changes_seed() {
        let m = Mnemonic::generate().expect("gen");
        let seed_empty = m.to_seed("").expect("empty");
        let seed_pass = m.to_seed("test").expect("test");
        assert_ne!(seed_empty, seed_pass);
    }

    #[test]
    fn test_coin_select_basic() {
        use qv_wallet::coin_select::CoinSelector;
        use qv_core::{Amount, OutPoint, TxId};
        use std::collections::BTreeMap;

        let mut utxos = BTreeMap::new();
        utxos.insert(
            OutPoint::new(TxId::from_bytes([1u8; 32]), 0),
            Amount::from_smallest_units(2_000),
        );

        // CoinSelector reserves a flat 1000-unit fee/dust buffer; pick a target
        // that leaves room above the reserve.
        let selector = CoinSelector::new(utxos, 1);
        let result = selector.select(Amount::from_smallest_units(500)).expect("select");
        assert_eq!(result.selected.len(), 1);
    }

    #[test]
    fn test_tx_builder_basic() {
        use qv_wallet::tx_builder::TxBuilder;
        use qv_core::{Amount, OutPoint, Script, TxInput, TxOutput, ValidityInterval, TxId};

        let validity = ValidityInterval::UNBOUNDED;
        let mut builder = TxBuilder::new(validity);

        let input = TxInput::new(OutPoint::new(TxId::from_bytes([0u8; 32]), 0));
        builder.add_input(input);

        let output = TxOutput::new(
            Amount::from_smallest_units(100),
            Script::new(vec![]),
        );
        builder.add_output(output);

        let tx = builder.build_unsigned().expect("build");
        assert_eq!(tx.inputs.len(), 1);
        assert_eq!(tx.outputs.len(), 1);
    }

    #[test]
    fn test_rpc_client_creation() {
        use qv_wallet::rpc_client::RpcClient;
        let client = RpcClient::new("http://localhost:8080");
        let debug = format!("{:?}", client);
        assert!(debug.contains("RpcClient"));
    }

    #[test]
    fn test_memory_match_store() {
        use qv_wallet::scanner::{MatchStore, MemoryMatchStore};
        use qv_core::{Amount, OutPoint, TxId};

        let mut store = MemoryMatchStore::new();
        let op = OutPoint::new(TxId::from_bytes([0u8; 32]), 0);
        store.add_match(op.clone(), Amount::from_smallest_units(100)).expect("add");
        let matches = store.get_matches().expect("get");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_cli_parse_init() {
        use clap::Parser;
        use qv_wallet::cli::{Cli, Commands};

        let args = vec!["wallet", "init"];
        let cli = Cli::try_parse_from(args).expect("parse");
        assert!(matches!(cli.command, Commands::Init { .. }));
    }

    #[test]
    fn test_cli_parse_send() {
        // Updated 2026-05-07 (W-05): Send takes named flags now.
        use clap::Parser;
        use qv_wallet::cli::{Cli, Commands};

        let args = vec![
            "wallet",
            "send",
            "--to-pubkey",
            "deadbeef",
            "--amount",
            "100",
            "--input",
            "00:0",
            "--input-value",
            "1000",
        ];
        let cli = Cli::try_parse_from(args).expect("parse");
        match cli.command {
            Commands::Send {
                to_pubkey,
                amount,
                input,
                input_value,
                fee,
                broadcast,
                ..
            } => {
                assert_eq!(to_pubkey, "deadbeef");
                assert_eq!(amount, 100);
                assert_eq!(input, "00:0");
                assert_eq!(input_value, 1000);
                assert_eq!(fee, 1000); // default
                assert!(!broadcast);
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn test_cli_parse_address() {
        use clap::Parser;
        use qv_wallet::cli::{Cli, Commands};

        let args = vec!["wallet", "address", "1"];
        let cli = Cli::try_parse_from(args).expect("parse");
        match cli.command {
            Commands::Address { account } => {
                assert_eq!(account, 1);
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn test_invalid_phrase() {
        let result = Mnemonic::from_phrase("single");
        assert!(result.is_err());
    }

    #[test]
    fn test_coin_select_insufficient() {
        use qv_wallet::coin_select::CoinSelector;
        use qv_core::{Amount, OutPoint, TxId};
        use std::collections::BTreeMap;

        let mut utxos = BTreeMap::new();
        utxos.insert(
            OutPoint::new(TxId::from_bytes([1u8; 32]), 0),
            Amount::from_smallest_units(100),
        );

        let selector = CoinSelector::new(utxos, 100);
        let result = selector.select(Amount::from_smallest_units(1000));
        assert!(result.is_err());
    }
}
