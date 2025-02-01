use crate::ResolcEVM;
use alloy_json_abi::JsonAbi;
use foundry_compilers_artifacts_solc::{
    CompactBytecode, CompactContractBytecode, CompactContractBytecodeCow, CompactContractRef,
    CompactDeployedBytecode, DevDoc, StorageLayout, UserDoc,
};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ResolcContract {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abi: Option<JsonAbi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devdoc: Option<DevDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userdoc: Option<UserDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_layout: Option<StorageLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm: Option<ResolcEVM>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ir_optimized: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_dependencies: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_libraries: Option<HashSet<String>>,
}

impl<'de> Deserialize<'de> for ResolcContract {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ContractFields {
            #[serde(default)]
            abi: Option<JsonAbi>,
            #[serde(default)]
            metadata: Option<Value>,
            #[serde(default)]
            devdoc: Option<DevDoc>,
            #[serde(default)]
            userdoc: Option<UserDoc>,
            #[serde(default)]
            storage_layout: Option<StorageLayout>,
            #[serde(default)]
            evm: Option<ResolcEVM>,
            #[serde(default)]
            ir_optimized: Option<String>,
            #[serde(default)]
            hash: Option<String>,
            #[serde(default)]
            factory_dependencies: Option<BTreeMap<String, String>>,
            #[serde(default)]
            missing_libraries: Option<HashSet<String>>,
        }

        let fields = ContractFields::deserialize(deserializer)?;

        fn extract_from_metadata<T: DeserializeOwned>(
            metadata: &Option<Value>,
            field: &str,
        ) -> Option<T> {
            metadata.as_ref().and_then(|metadata| {
                if let Value::String(s) = metadata {
                    serde_json::from_str(s).ok().and_then(|md: Value| {
                        md.as_object()?
                            .get("solc_metadata")?
                            .get(field)
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                    })
                } else {
                    None
                }
            })
        }

        let abi = fields.abi.or_else(|| extract_from_metadata(&fields.metadata, "abi"));
        let userdoc = fields.userdoc.or_else(|| extract_from_metadata(&fields.metadata, "userdoc"));
        let devdoc = fields.devdoc.or_else(|| extract_from_metadata(&fields.metadata, "devdoc"));

        Ok(ResolcContract {
            abi,
            metadata: fields.metadata,
            devdoc,
            userdoc,
            storage_layout: fields.storage_layout,
            evm: fields.evm,
            ir_optimized: fields.ir_optimized,
            hash: fields.hash,
            factory_dependencies: fields.factory_dependencies,
            missing_libraries: fields.missing_libraries,
        })
    }
}

impl Default for ResolcContract {
    fn default() -> Self {
        Self {
            abi: None,
            metadata: None,
            devdoc: None,
            userdoc: None,
            storage_layout: None,
            evm: None,
            ir_optimized: None,
            hash: None,
            factory_dependencies: None,
            missing_libraries: None,
        }
    }
}

impl<'a> From<&'a ResolcContract> for CompactContractBytecodeCow<'a> {
    fn from(value: &'a ResolcContract) -> Self {
        if let Some((standard_abi, compact_bytecode, compact_deployed_bytecode)) =
            create_compact_bytecode(value)
        {
            Self {
                abi: Some(Cow::Owned(standard_abi)),
                bytecode: Some(Cow::Owned(compact_bytecode)),
                deployed_bytecode: Some(Cow::Owned(compact_deployed_bytecode)),
            }
        } else {
            Self { abi: None, bytecode: None, deployed_bytecode: None }
        }
    }
}

impl From<ResolcContract> for CompactContractBytecode {
    fn from(value: ResolcContract) -> Self {
        if let Some((standard_abi, compact_bytecode, compact_deployed_bytecode)) =
            create_compact_bytecode(&value)
        {
            Self {
                abi: Some(standard_abi),
                bytecode: Some(compact_bytecode),
                deployed_bytecode: Some(compact_deployed_bytecode),
            }
        } else {
            Self { abi: None, bytecode: None, deployed_bytecode: None }
        }
    }
}

impl<'a> From<&'a ResolcContract> for CompactContractRef<'a> {
    fn from(c: &'a ResolcContract) -> Self {
        let (bin, bin_runtime) = if let Some(ref evm) = c.evm {
            (
                evm.bytecode.as_ref().map(|code| &code.object),
                evm.deployed_bytecode
                    .as_ref()
                    .and_then(|deployed| deployed.bytecode.as_ref().map(|code| &code.object)),
            )
        } else {
            (None, None)
        };

        Self { abi: c.abi.as_ref(), bin, bin_runtime }
    }
}

fn create_compact_bytecode(
    parent_contract: &ResolcContract,
) -> Option<(JsonAbi, CompactBytecode, CompactDeployedBytecode)> {
    let Some(resolc_evm) = &parent_contract.evm else {
        return None;
    };

    let Some(bytecode) = &resolc_evm.bytecode else {
        return None;
    };

    let Some(deployed) = &resolc_evm.deployed_bytecode else {
        return None;
    };

    let Some(deployed_bytecode) = &deployed.bytecode else {
        return None;
    };

    let compact_bytecode = CompactBytecode {
        object: bytecode.object.clone(),
        source_map: None,
        link_references: BTreeMap::default(),
    };

    let compact_bytecode_deployed = CompactBytecode {
        object: deployed_bytecode.object.clone(),
        source_map: None,
        link_references: BTreeMap::default(),
    };

    let compact_deployed_bytecode = CompactDeployedBytecode {
        bytecode: Some(compact_bytecode_deployed),
        immutable_references: BTreeMap::default(),
    };

    Some((
        parent_contract.abi.clone().unwrap_or_default(),
        compact_bytecode,
        compact_deployed_bytecode,
    ))
}
