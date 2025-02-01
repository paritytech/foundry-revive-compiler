use crate::{
    error::Result, resolc::contracts::VersionedContracts, sources::VersionedSourceFiles,
    ArtifactFile, ArtifactOutput, Artifacts, ArtifactsMap, OutputContext, ProjectPathsConfig,
};
use alloy_json_abi::JsonAbi;
use alloy_primitives::{hex, Bytes};
use foundry_compilers_artifacts::{
    resolc::{contract::ResolcContract, ResolcEVM},
    BytecodeObject, CompactBytecode, CompactContract, CompactContractBytecode,
    CompactContractBytecodeCow, CompactDeployedBytecode, DevDoc, SolcLanguage, SourceFile,
    StorageLayout, UserDoc,
};
use foundry_compilers_core::error::SolcIoError;
use path_slash::PathBufExt;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ResolcArtifactOutput();

#[derive(Debug, Serialize, Clone)]
pub struct ContractArtifact {
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

impl<'de> Deserialize<'de> for ContractArtifact {
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

        Ok(ContractArtifact {
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

impl Default for ContractArtifact {
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

impl<'a> From<&'a ContractArtifact> for CompactContractBytecodeCow<'a> {
    fn from(value: &'a ContractArtifact) -> Self {
        if let Some((standard_abi, compact_bytecode, compact_deployed_bytecode)) =
            create_compact_bytecode(value)
        {
            Self {
                abi: Some(Cow::Owned(standard_abi)),
                bytecode: Some(Cow::Owned(compact_bytecode)),
                deployed_bytecode: Some(Cow::Owned(compact_deployed_bytecode)),
            }
        } else {
            Self { abi: value.abi.clone().map(Cow::Owned), bytecode: None, deployed_bytecode: None }
        }
    }
}

impl From<ContractArtifact> for CompactContractBytecode {
    fn from(value: ContractArtifact) -> Self {
        if let Some((standard_abi, compact_bytecode, compact_deployed_bytecode)) =
            create_compact_bytecode(&value)
        {
            Self {
                abi: Some(standard_abi),
                bytecode: Some(compact_bytecode),
                deployed_bytecode: Some(compact_deployed_bytecode),
            }
        } else {
            Self { abi: value.abi, bytecode: None, deployed_bytecode: None }
        }
    }
}

impl From<ContractArtifact> for CompactContract {
    fn from(value: ContractArtifact) -> Self {
        if let Some((standard_abi, compact_bytecode, _)) = create_compact_bytecode(&value) {
            Self {
                bin: Some(compact_bytecode.object.clone()),
                bin_runtime: Some(compact_bytecode.object),
                abi: Some(standard_abi),
            }
        } else {
            Self { bin: None, bin_runtime: None, abi: value.abi }
        }
    }
}

impl ArtifactOutput for ResolcArtifactOutput {
    type Artifact = ContractArtifact;

    fn contract_to_artifact(
        &self,
        _file: &std::path::Path,
        _name: &str,
        _contract: foundry_compilers_artifacts::Contract,
        _source_file: Option<&foundry_compilers_artifacts::SourceFile>,
    ) -> Self::Artifact {
        todo!("Implement this if needed")
    }

    fn standalone_source_file_to_artifact(
        &self,
        _path: &std::path::Path,
        _file: &crate::sources::VersionedSourceFile,
    ) -> Option<Self::Artifact> {
        None
    }
}

impl ResolcArtifactOutput {
    pub fn resolc_contract_to_artifact(
        &self,
        _file: &Path,
        _name: &str,
        contract: ResolcContract,
        _source_file: Option<&SourceFile>,
    ) -> ContractArtifact {
        ContractArtifact {
            abi: contract.abi,
            metadata: contract.metadata,
            devdoc: contract.devdoc,
            userdoc: contract.userdoc,
            storage_layout: contract.storage_layout,
            evm: contract.evm,
            ir_optimized: contract.ir_optimized,
            hash: None,
            factory_dependencies: contract.factory_dependencies,
            missing_libraries: contract.missing_libraries,
        }
    }

    /// Handle the aggregated set of compiled contracts from the solc [`crate::CompilerOutput`].
    ///
    /// This will be invoked with all aggregated contracts from (multiple) solc `CompilerOutput`.
    /// See [`crate::AggregatedCompilerOutput`]
    pub fn resolc_on_output(
        &self,
        contracts: &VersionedContracts,
        sources: &VersionedSourceFiles,
        layout: &ProjectPathsConfig<SolcLanguage>,
        ctx: OutputContext<'_>,
    ) -> Result<Artifacts<ContractArtifact>> {
        let mut artifacts = self.resolc_output_to_artifacts(contracts, sources, ctx, layout);
        fs::create_dir_all(&layout.artifacts).map_err(|err| {
            error!(dir=?layout.artifacts, "Failed to create artifacts folder");
            SolcIoError::new(err, &layout.artifacts)
        })?;

        artifacts.join_all(&layout.artifacts);
        artifacts.write_all()?;

        Ok(artifacts)
    }
    /// Convert the compiler output into a set of artifacts
    ///
    /// **Note:** This does only convert, but _NOT_ write the artifacts to disk, See
    /// [`Self::on_output()`]
    pub fn resolc_output_to_artifacts(
        &self,
        contracts: &VersionedContracts,
        sources: &VersionedSourceFiles,
        ctx: OutputContext<'_>,
        layout: &ProjectPathsConfig<SolcLanguage>,
    ) -> Artifacts<ContractArtifact> {
        let mut artifacts = ArtifactsMap::new();

        // this tracks all the `SourceFile`s that we successfully mapped to a contract
        let mut non_standalone_sources = HashSet::new();

        // prepopulate taken paths set with cached artifacts
        let mut taken_paths_lowercase = ctx
            .existing_artifacts
            .values()
            .flat_map(|artifacts| artifacts.values())
            .flat_map(|artifacts| artifacts.values())
            .flat_map(|artifacts| artifacts.values())
            .map(|a| a.path.to_slash_lossy().to_lowercase())
            .collect::<HashSet<_>>();

        let mut files = contracts.keys().collect::<Vec<_>>();
        // Iterate starting with top-most files to ensure that they get the shortest paths.
        files.sort_by(|file1, file2| {
            (file1.components().count(), file1).cmp(&(file2.components().count(), file2))
        });
        for file in files {
            for (name, versioned_contracts) in &contracts[file] {
                let unique_versions =
                    versioned_contracts.iter().map(|c| &c.version).collect::<HashSet<_>>();
                let unique_profiles =
                    versioned_contracts.iter().map(|c| &c.profile).collect::<HashSet<_>>();
                for contract in versioned_contracts {
                    non_standalone_sources.insert(file);

                    // track `SourceFile`s that can be mapped to contracts
                    let source_file = sources.find_file_and_version(file, &contract.version);

                    let artifact_path = Self::get_artifact_path(
                        &ctx,
                        &taken_paths_lowercase,
                        file,
                        name,
                        layout.artifacts.as_path(),
                        &contract.version,
                        &contract.profile,
                        unique_versions.len() > 1,
                        unique_profiles.len() > 1,
                    );

                    taken_paths_lowercase.insert(artifact_path.to_slash_lossy().to_lowercase());

                    trace!(
                        "use artifact file {:?} for contract file {} {}",
                        artifact_path,
                        file.display(),
                        contract.version
                    );

                    let artifact = self.resolc_contract_to_artifact(
                        file,
                        name,
                        contract.contract.clone(),
                        source_file,
                    );

                    let artifact = ArtifactFile {
                        artifact,
                        file: artifact_path,
                        version: contract.version.clone(),
                        build_id: contract.build_id.clone(),
                        profile: contract.profile.clone(),
                    };

                    artifacts
                        .entry(file.to_path_buf())
                        .or_default()
                        .entry(name.to_string())
                        .or_default()
                        .push(artifact);
                }
            }
        }

        // extend with standalone source files and convert them to artifacts
        // this is unfortunately necessary, so we can "mock" `Artifacts` for solidity files without
        // any contract definition, which are not included in the `CompilerOutput` but we want to
        // create Artifacts for them regardless
        for (file, sources) in sources.as_ref().iter() {
            let unique_versions = sources.iter().map(|s| &s.version).collect::<HashSet<_>>();
            let unique_profiles = sources.iter().map(|s| &s.profile).collect::<HashSet<_>>();
            for source in sources {
                if !non_standalone_sources.contains(file) {
                    // scan the ast as a safe measure to ensure this file does not include any
                    // source units
                    // there's also no need to create a standalone artifact for source files that
                    // don't contain an ast
                    if source.source_file.ast.is_none()
                        || source.source_file.contains_contract_definition()
                    {
                        continue;
                    }

                    // we use file and file stem
                    if let Some(name) = Path::new(file).file_stem().and_then(|stem| stem.to_str()) {
                        if let Some(artifact) =
                            self.standalone_source_file_to_artifact(file, source)
                        {
                            let artifact_path = Self::get_artifact_path(
                                &ctx,
                                &taken_paths_lowercase,
                                file,
                                name,
                                &layout.artifacts,
                                &source.version,
                                &source.profile,
                                unique_versions.len() > 1,
                                unique_profiles.len() > 1,
                            );

                            taken_paths_lowercase
                                .insert(artifact_path.to_slash_lossy().to_lowercase());

                            artifacts
                                .entry(file.clone())
                                .or_default()
                                .entry(name.to_string())
                                .or_default()
                                .push(ArtifactFile {
                                    artifact,
                                    file: artifact_path,
                                    version: source.version.clone(),
                                    build_id: source.build_id.clone(),
                                    profile: source.profile.clone(),
                                });
                        }
                    }
                }
            }
        }

        Artifacts(artifacts)
    }
}

fn create_compact_bytecode(
    parent_contract: &ContractArtifact,
) -> Option<(JsonAbi, CompactBytecode, CompactDeployedBytecode)> {
    let standard_abi = parent_contract.abi.clone().unwrap_or_default();
    let evm = parent_contract.evm.as_ref()?;
    let deserialized_contract_bytecode = evm.bytecode.as_ref()?.object.as_bytes()?;
    let deserialized_contract_deployed_bytecode = evm.deployed_bytecode.as_ref()?.bytes()?;

    let bytecode = match hex::decode(deserialized_contract_bytecode) {
        Ok(bytes) => BytecodeObject::Bytecode(Bytes::from(bytes)),
        Err(_) => return None,
    };

    let deployed_bytecode = match hex::decode(deserialized_contract_deployed_bytecode) {
        Ok(bytes) => BytecodeObject::Bytecode(Bytes::from(bytes)),
        Err(_) => return None,
    };

    let compact_bytecode = CompactBytecode {
        object: bytecode,
        source_map: None,
        link_references: BTreeMap::default(),
    };

    let compact_bytecode_deployed = CompactBytecode {
        object: deployed_bytecode,
        source_map: None,
        link_references: BTreeMap::default(),
    };

    let compact_deployed_bytecode = CompactDeployedBytecode {
        bytecode: Some(compact_bytecode_deployed),
        immutable_references: BTreeMap::default(),
    };

    Some((standard_abi, compact_bytecode, compact_deployed_bytecode))
}
