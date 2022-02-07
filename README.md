# Mobix Staking

Compile:

```
rustup default stable
cargo wasm
```

Test:

```
cargo test
```

Optimize:

quick and good:
```
RUSTFLAGS='-C link-arg=-s' cargo wasm
```

slow and better (*requires Docker):
```
docker run --rm -v "$(pwd)":/code   --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target   --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry   cosmwasm/rust-optimizer:0.12.4
```

## Contract Verification Guide

After you used the command above for the optimized compilation of the contract with docker, you will find the wasm file in the `artifacts` dir. Ultimately you would want to verify this is the same as the contract on-chain. 

To do that you can use some of the following techniques.

### Without fetchd

- Get the hash of the compiled contract:
```
sha256sum artifacts/mobix_staking.wasm
```

- Get the info of the contract on-chain:
```
curl <rest_endpoint>/wasm/contract/<contract_address>
```
*You will need the `code_id` from the result.

- Get the hash of that contract on-chain:
```
curl <rest_endpoint>/wasm/code/<code_id>
```
*You will find it under `data_hash` from the result.

- Compare the two

### With fetchd

- Get the hash of the compiled contract:
```
sha256sum artifacts/mobix_staking.wasm
```

- Get the info of the contract on-chain:
```
fetchd query wasm contract <contract_address> $NODE
```
*You will need the `code_id` from the output.

- Download the wasm file:
```
fetchd query wasm code <code_id> <code_id>_code.wasm $NODE
```

- Compare the two files or their hashes:
```
diff artifacts/mobix_staking.wasm <code_id>_code.wasm
```