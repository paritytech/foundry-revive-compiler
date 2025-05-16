use foundry_compilers_artifacts::{
    output_selection::OutputSelection, Remapping, Settings, SolcLanguage, Source, Sources,
};
use foundry_compilers_core::utils::strip_prefix_owned;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{to_value, Map, Value};
use std::{
    collections::{BTreeSet, HashSet},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use crate::{
    solc::{CliSettings, SolcRestrictions},
    CompilerInput, CompilerSettings,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolcVersionedInput {
    #[serde(flatten)]
    pub input: ResolcInput,
    pub solc_version: Version,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolcInput {
    pub language: SolcLanguage,
    pub sources: Sources,
    pub settings: ResolcSettings,
}

impl ResolcInput {
    fn new(language: SolcLanguage, sources: Sources, settings: ResolcSettings) -> Self {
        Self { language, sources, settings }
    }

    pub fn strip_prefix(&mut self, base: &Path) {
        self.sources = std::mem::take(&mut self.sources)
            .into_iter()
            .map(|(path, s)| (strip_prefix_owned(path, base), s))
            .collect();

        self.settings.settings.strip_prefix(base);
    }
}

impl Default for ResolcInput {
    fn default() -> Self {
        Self {
            language: SolcLanguage::Solidity,
            sources: Sources::default(),
            settings: ResolcSettings::default(),
        }
    }
}

impl CompilerInput for ResolcVersionedInput {
    type Settings = ResolcSettings;
    type Language = SolcLanguage;

    fn build(
        sources: Sources,
        settings: Self::Settings,
        language: Self::Language,
        version: Version,
    ) -> Self {
        let hash_set = HashSet::from([
            "abi",
            "metadata",
            "devdoc",
            "userdoc",
            "evm.methodIdentifiers",
            "storageLayout",
            "ast",
            "irOptimized",
            "evm.legacyAssembly",
            "evm.bytecode",
            "evm.deployedBytecode",
            "evm.assembly",
            "ir",
        ]);
        let json_settings = settings.settings.sanitized(&version, language);

        let mut settings =
            Self::Settings { settings: json_settings, cli_settings: settings.cli_settings };
        settings.update_output_selection(|selection| {
            for (_, key) in selection.0.iter_mut() {
                for (_, value) in key.iter_mut() {
                    value.retain(|item| hash_set.contains(item.as_str()));
                }
            }
        });
        let input = ResolcInput::new(language, sources, settings);
        Self { input, solc_version: version }
    }

    fn language(&self) -> Self::Language {
        self.input.language
    }

    fn version(&self) -> &Version {
        &self.solc_version
    }

    fn sources(&self) -> impl Iterator<Item = (&Path, &Source)> {
        self.input.sources.iter().map(|(path, source)| (path.as_path(), source))
    }

    fn strip_prefix(&mut self, base: &Path) {
        self.input.strip_prefix(base);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ResolcSettings {
    /// JSON settings expected by Solc
    #[serde(flatten)]
    pub settings: ResolcJsonSettings,
    /// Additional CLI args configuration
    #[serde(flatten)]
    pub cli_settings: CliSettings,
}

impl Deref for ResolcSettings {
    type Target = ResolcJsonSettings;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

impl DerefMut for ResolcSettings {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.settings
    }
}

impl CompilerSettings for ResolcSettings {
    type Restrictions = SolcRestrictions;

    fn update_output_selection(&mut self, f: impl FnOnce(&mut OutputSelection) + Copy) {
        f(&mut self.settings.settings.output_selection)
    }

    fn can_use_cached(&self, other: &Self) -> bool {
        let Self {
            settings:
                ResolcJsonSettings {
                    settings:
                        Settings {
                            stop_after,
                            remappings,
                            optimizer,
                            model_checker,
                            metadata,
                            output_selection,
                            evm_version,
                            via_ir,
                            debug,
                            libraries,
                            eof_version,
                        },
                    stack_size,
                    heap_size,
                    optimizer_mode,
                },
            ..
        } = self;

        *stop_after == other.settings.settings.stop_after
            && *remappings == other.settings.settings.remappings
            && *optimizer == other.settings.settings.optimizer
            && *model_checker == other.settings.settings.model_checker
            && *metadata == other.settings.settings.metadata
            && *evm_version == other.settings.settings.evm_version
            && *via_ir == other.settings.settings.via_ir
            && *debug == other.settings.settings.debug
            && *libraries == other.settings.settings.libraries
            && *eof_version == other.settings.settings.eof_version
            && output_selection.is_subset_of(&other.settings.settings.output_selection)
            && *stack_size == other.stack_size
            && *heap_size == other.heap_size
            && *optimizer_mode == other.optimizer_mode
    }

    fn with_remappings(mut self, remappings: &[Remapping]) -> Self {
        self.settings.settings.remappings = remappings.to_vec();

        self
    }

    fn with_allow_paths(mut self, allowed_paths: &BTreeSet<PathBuf>) -> Self {
        self.cli_settings.allow_paths.clone_from(allowed_paths);
        self
    }

    fn with_base_path(mut self, base_path: &Path) -> Self {
        self.cli_settings.base_path = Some(base_path.to_path_buf());
        self
    }

    fn with_include_paths(mut self, include_paths: &BTreeSet<PathBuf>) -> Self {
        self.cli_settings.include_paths.clone_from(include_paths);
        self
    }

    fn satisfies_restrictions(&self, restrictions: &Self::Restrictions) -> bool {
        // TODO Add resolc restrictions
        let mut satisfies = true;

        let SolcRestrictions { evm_version, via_ir, optimizer_runs, bytecode_hash } = restrictions;

        satisfies &= evm_version.satisfies(self.settings.evm_version);
        satisfies &= via_ir.is_none_or(|via_ir| via_ir == self.settings.via_ir.unwrap_or_default());
        satisfies &= bytecode_hash.is_none_or(|bytecode_hash| {
            self.settings.metadata.as_ref().and_then(|m| m.bytecode_hash) == Some(bytecode_hash)
        });
        satisfies &= optimizer_runs.satisfies(self.settings.optimizer.runs);

        // Ensure that we either don't have min optimizer runs set or that the optimizer is enabled
        satisfies &= optimizer_runs
            .min
            .is_none_or(|min| min == 0 || self.settings.optimizer.enabled.unwrap_or_default());

        satisfies
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolcJsonSettings {
    pub settings: Settings,
    pub heap_size: Option<u64>,
    pub stack_size: Option<u64>,
    pub optimizer_mode: Option<char>,
}

impl Deref for ResolcJsonSettings {
    type Target = Settings;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

impl DerefMut for ResolcJsonSettings {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.settings
    }
}

impl Serialize for ResolcJsonSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize ResolcJsonSettings to JSON value
        let mut json = to_value(&self.settings).map_err(serde::ser::Error::custom)?;
        let settings_obj = json
            .as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("Expected settings to be a JSON object"))?;

        // Inject optimizer.mode
        if let Some(mode) = &self.optimizer_mode {
            let optimizer = settings_obj
                .entry("optimizer")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| serde::ser::Error::custom("Expected `optimizer` to be an object"))?;

            optimizer.insert("mode".to_string(), Value::String(mode.to_string()));
        }

        // Ensure settings.polkavm.memory_config exists
        let polkavm = settings_obj
            .entry("polkavm")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("Expected `polkavm` to be an object"))?;

        let memory_config = polkavm
            .entry("memory_config")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("Expected `memory_config` to be an object"))?;

        // Inject heap_size
        if let Some(heap) = self.heap_size {
            memory_config.insert("heap_size".to_string(), Value::Number(heap.into()));
        }

        // Inject stack_size
        if let Some(stack) = self.stack_size {
            memory_config.insert("stack_size".to_string(), Value::Number(stack.into()));
        }

        // Serialize final result
        json.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResolcJsonSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize JSON into a Value first
        let mut json = Value::deserialize(deserializer)?;

        // Extract 'settings' object, error if missing or wrong type
        let settings_val =
            json.get_mut("settings").ok_or_else(|| serde::de::Error::missing_field("settings"))?;

        // Deserialize settings into Settings struct
        let settings: Settings =
            serde_json::from_value(settings_val.take()).map_err(serde::de::Error::custom)?;

        // Use combinators to try extract optimizer.mode as char
        let optimizer_mode = json
            .get("settings")
            .and_then(|s| s.get("optimizer"))
            .and_then(|opt| opt.get("mode"))
            .and_then(|mode_val| mode_val.as_str())
            .and_then(|s| s.chars().next());

        // Extract heap_size and stack_size from settings.polkavm.memory_config
        let memory_config = json
            .get("settings")
            .and_then(|s| s.get("polkavm"))
            .and_then(|p| p.get("memory_config"));

        let heap_size = memory_config.and_then(|mem| mem.get("heap_size")).and_then(Value::as_u64);

        let stack_size =
            memory_config.and_then(|mem| mem.get("stack_size")).and_then(Value::as_u64);

        Ok(Self { settings, optimizer_mode, heap_size, stack_size })
    }
}

impl ResolcJsonSettings {
    /// Creates a new `ResolcJsonSettings` instance with the given `output_selection`
    pub fn new(output_selection: impl Into<OutputSelection>) -> Self {
        let mut s: Self = Default::default();
        s.settings.output_selection = output_selection.into();
        s
    }

    /// Consumes the type and returns a [ResolcJsonSettings::sanitize] version
    pub fn sanitized(mut self, version: &Version, language: SolcLanguage) -> Self {
        self.sanitize(version, language);
        self
    }

    /// This will remove/adjust values in the settings that are not compatible with this version.
    pub fn sanitize(&mut self, version: &Version, language: SolcLanguage) {
        // TODO remove/adjust values in the settings that are not compatible with resolc version
        self.settings.sanitize(version, language)
    }

    /// Enable `viaIR` and use the minimum optimization settings.
    ///
    /// This is useful in the following scenarios:
    /// - When compiling for test coverage, this can resolve the "stack too deep" error while still
    ///   giving a relatively accurate source mapping
    /// - When compiling for test, this can reduce the compilation time
    pub fn with_via_ir_minimum_optimization(mut self) -> Self {
        self.settings = self.settings.with_via_ir_minimum_optimization();
        self.optimizer_mode = Some('0');
        self
    }
}

impl Default for ResolcJsonSettings {
    fn default() -> Self {
        Self {
            optimizer_mode: Some('z'),
            // We do not override default resolc stack and heap size.
            stack_size: None,
            heap_size: None,
            settings: Default::default(),
        }
    }
}
