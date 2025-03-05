### MultiCompiler approach 

- Seems like the most straightforward way of going forward. 
  - a lot of the things in foundry and foundry-compilers rely on direct use of MultiCompiler instead of using a `Compiler` Trait.
  - Some of the errors when using a `Compiler` directly appear to be because of trait resolution and in some places the error is invisible because the `MultiCompiler` is used instead of a generic `Compiler`. this is especially true for `foundry`
    - e.g [C::ParsedSource: MaybeSolData,](https://github.com/paritytech/foundry-revive-compiler/blob/f15f2dc8c7be4e05522787eaba43825c8093aa62/crates/compilers/src/flatten.rs#L212) `MaybeSolData` is private despite being used in an exposed interface.
    - [pub fn collect_ordered_deps<D: ParsedSource + MaybeSolData>(](https://github.com/paritytech/foundry-revive-compiler/blob/f15f2dc8c7be4e05522787eaba43825c8093aa62/crates/compilers/src/flatten.rs#L838-L839) - same
    - [fn collect_deps<D: ParsedSource + MaybeSolData>(](https://github.com/paritytech/foundry-revive-compiler/blob/f15f2dc8c7be4e05522787eaba43825c8093aa62/crates/compilers/src/flatten.rs#L802-L803) - same as above.
  - `ProjectPaths`, `ProjectPathBuilder`, `Project` and `ProjectBuilder` assume to use `MultiCompiler` by default. 
  - `project_utils` and testing files assume direct use of `MultiCompiler` so reusing tests is quite hard because a lot of the setup depends on direct field manipulation inside `MultiCompiler` 
  - Requires refactoring of core traits inside foundry. e.g. `CompilerInput` as they were designed with closed world assumption where "1 input language = 1 compiler"
    - doesn't seem so bad, although will be a bit hacky-ish.
  - A bit nonsensical as `Vyper` is totally useless for `resolc` for now, although shouldn't it be possible to support it?
  - Requires smaller amount of changes than zksync approach as we can largely map the output types to the Multicompiler's default types. 
    - So instead of adding `if` statements throughout `foundry` we can just extends existing enums inside `foundry-compilers` and pattern match them away where needed to extend the functionality. e.g `Bytecode`, `DeployedBytecode`. 
  - On the other hand is in direct competition with the Matter-labs PR to introduce strategies to support different compilers inside `foundry`.

### `Compiler` approach 
- Follows the same approach as Matter-labs take wrt using Compiler directly
- Has issues with the fact that `foundry` itself relies on the fact that `MultiCompiler` is used directly.
- Will involve either lot's of `if` statements inside `foundry` crates for `resolc` specific cases and issues with reconciling `input` and `output` types.  
or lots of piping of traits and trait resolutions to use a generic `Compiler` everywhere in `foundry` where we expect to see `MultiCompiler` by default instead. 
  - example tests for the `foundry-compilers`
- Scope of the changes will be bigger to both `foundry` and the `foundry-compilers`. 
   - Yes, it involves refactoring code inside `foundry` and `foundry-compilers` to rely on generic `Compiler` impl, but this has a higher chance to be included. 
   - Possibility of Output reconciling issues wrt types. [see zksync example, happens because `foundry` relies on `MultiCompiler` instead](https://github.com/matter-labs/foundry-zksync/blob/4b59d03591fccb6ecd3a21a8605e83c52122d969/crates/forge/bin/cmd/build.rs#L69)
- When asked about compiler version we return `solc` version now. `zk-sync` compiler does the same. Should we return `resolc` version instead?
  - We need to implement version resolution for `solc` and `resolc` if the future changes to `resolc` can drop new `solc` versions. 
  - Alternatively support always the latest version of `resolc`.

### Common things 
- Bytecode returned by `resolc` is just a plain string so we can't really reconcile the types inside `foundry-compilers` and `revive`. (I assume the `bytecode` is linked by default.)
- `MetadataHash` in `resolc` is in fact `BytecodeHash` and only `keccak256` is supported right now. 
  - `Zksync` added support for `ipfs` hash in later versions. 
    - [PR era-compiler-solidity](https://github.com/matter-labs/era-compiler-solidity/pull/128)
    - [PR era-compiler-llvm-context](https://github.com/matter-labs/era-compiler-llvm-context/pull/38)
    - [PR era-compiler-llvm-context](https://github.com/matter-labs/era-compiler-llvm-context/pull/39)
  - So it seems like this needs to be done by `revive` too to fully support `foundry` 
- `eth` centric fields are unpopulated. (e.g `ewasm`) - couldn't care less(?). 
- 
- custom `Bytecode` and no difference between `DeployedBytecode` and `Bytecode`. 
- Foundry still hard depedency on `MultiCompiler`, `Zk-sync` guys trying to make it work [PR](https://github.com/foundry-rs/foundry/pull/9651).
  - Seems like we will be duplicating this work if we try to go with `Compiler` approach instead of `MultiCompiler` approach. 