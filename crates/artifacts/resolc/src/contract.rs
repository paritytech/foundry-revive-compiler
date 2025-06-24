use std::collections::{BTreeMap, HashSet};

use alloy_json_abi::JsonAbi;
use foundry_compilers_artifacts_solc::{DevDoc, LosslessMetadata, StorageLayout, UserDoc};
use serde::{Deserialize, Serialize};

use crate::ResolcEVM;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResolcContract {
    /// The contract ABI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abi: Option<JsonAbi>,
    /// The contract metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// The contract developer documentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devdoc: Option<DevDoc>,
    /// The contract user documentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userdoc: Option<UserDoc>,
    /// The contract storage layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_layout: Option<StorageLayout>,
    /// Contract's bytecode and related objects
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm: Option<ResolcEVM>,
    /// The contract IR code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ir: Option<String>,
    /// The contract optimized IR code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ir_optimized: Option<String>,
    /// The contract PolkaVM bytecode hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// The contract factory dependencies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_dependencies: Option<BTreeMap<String, String>>,
    /// The contract missing libraries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_libraries: Option<HashSet<String>>,
}

impl From<ResolcContract> for foundry_compilers_artifacts_solc::Contract {
    fn from(contract: ResolcContract) -> Self {
        let meta = match contract.metadata.as_ref() {
            Some(serde_json::Value::Object(map)) => {
                map.get("solc_metadata")
                    .and_then(|solc_metadata| serde_json::from_value::<LosslessMetadata>(solc_metadata.clone()).ok())
                    .map(|mut solc_metadata| {
                        // Extract and inject revive compiler information if available.
                        if let (Some(revive_version), Some(solc_version)) = (
                            map.get("revive_version").and_then(|v| v.as_str()),
                            map.get("solc_version").and_then(|v| v.as_str()),
                        ) {
                            // Update version and regenerate raw metadata.
                            solc_metadata.metadata.compiler.version = format!(
                                "{{\"revive\":\"{revive_version}\", \"solc\":\"{solc_version}\"}}"
                            );

                            if let Ok(raw_metadata) = serde_json::to_string(&solc_metadata.metadata)
                            {
                                solc_metadata.raw_metadata = raw_metadata;
                            }
                        }
                        solc_metadata
                    })
            }
            _ => None,
        };

        Self {
            abi: contract.abi,
            evm: contract.evm.map(Into::into),
            metadata: meta,
            userdoc: contract.userdoc.unwrap_or_default(),
            devdoc: contract.devdoc.unwrap_or_default(),
            ir: contract.ir,
            storage_layout: contract.storage_layout.unwrap_or_default(),
            transient_storage_layout: Default::default(),
            ewasm: None,
            ir_optimized: contract.ir_optimized,
            ir_optimized_ast: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_compilers_artifacts_solc::Contract;
    use serde_json::json;

    fn create_test_solc_metadata() -> String {
        r#"{
            "compiler": {
                "version": "0.8.29+commit.ab55807c"
            },
            "language": "Solidity",
            "output": {
                "abi": [],
                "devdoc": {"kind": "dev", "methods": {}, "version": 1},
                "userdoc": {"kind": "user", "methods": {}, "version": 1}
            },
            "settings": {
                "compilationTarget": {"src/Counter.sol": "Counter"},
                "evmVersion": "cancun",
                "libraries": {},
                "metadata": {"bytecodeHash": "none"},
                "optimizer": {
                    "details": {
                        "constantOptimizer": false,
                        "cse": false,
                        "deduplicate": false,
                        "inliner": false,
                        "jumpdestRemover": false,
                        "orderLiterals": false,
                        "peephole": false,
                        "simpleCounterForLoopUncheckedIncrement": true,
                        "yul": false
                    },
                    "runs": 200
                },
                "remappings": [":forge-std/=lib/forge-std/src/"]
            },
            "sources": {
                "src/Counter.sol": {
                    "keccak256": "0x09277f949d59a9521708c870dc39c2c434ad8f86a5472efda6a732ef728c0053",
                    "license": "UNLICENSED",
                    "urls": [
                        "bzz-raw://94cd5258357da018bf911aeda60ed9f5b130dce27445669ee200313cd3389200",
                        "dweb:/ipfs/QmNbEfWAqXCtfQpk6u7TpGa8sTHXFLpUz7uebz2FVbchSC"
                    ]
                }
            },
            "version": 1
        }"#.to_string()
    }

    fn create_resolc_contract(metadata: Option<serde_json::Value>) -> ResolcContract {
        ResolcContract {
            abi: None,
            metadata,
            devdoc: None,
            userdoc: None,
            storage_layout: None,
            evm: None,
            ir: None,
            ir_optimized: None,
            hash: None,
            factory_dependencies: None,
            missing_libraries: None,
        }
    }

    #[test]
    fn test_from_resolc_contract_with_revive_metadata() {
        let metadata = json!({
            "solc_metadata": create_test_solc_metadata(),
            "revive_version": "0.2.0+commit.e94432e.llvm-18.1.8",
            "solc_version": "0.8.29+commit.ab55807c.Darwin.appleclang"
        });

        let contract: Contract = create_resolc_contract(Some(metadata)).into();

        let metadata = contract.metadata.expect("metadata should be present");
        assert_eq!(
            metadata.metadata.compiler.version,
            r#"{"revive":"0.2.0+commit.e94432e.llvm-18.1.8", "solc":"0.8.29+commit.ab55807c.Darwin.appleclang"}"#
        );
    }

    #[test]
    fn test_from_resolc_contract_without_revive_metadata() {
        let metadata = json!({
            "solc_metadata": create_test_solc_metadata()
        });

        let contract: Contract = create_resolc_contract(Some(metadata)).into();

        let metadata = contract.metadata.expect("metadata should be present");
        assert_eq!(metadata.metadata.compiler.version, "0.8.29+commit.ab55807c");
    }

    #[test]
    fn test_from_resolc_contract_with_partial_revive_metadata() {
        let metadata = json!({
            "solc_metadata": create_test_solc_metadata(),
            "revive_version": "0.2.0+commit.e94432e.llvm-18.1.8"
        });

        let contract: Contract = create_resolc_contract(Some(metadata)).into();

        let metadata = contract.metadata.expect("metadata should be present");
        assert_eq!(metadata.metadata.compiler.version, "0.8.29+commit.ab55807c");
    }
}
