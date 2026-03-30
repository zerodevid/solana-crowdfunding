# Solana Crowdfunding Program

A native Solana smart contract implementing a Kickstarter-style crowdfunding platform with PDA-controlled fund vaults.

## Architecture

```
src/
├── lib.rs          # Entrypoint — routes instruction_data to Processor
├── instruction.rs  # Instruction enum + LE byte serialization/deserialization
├── processor.rs    # Business logic for all 4 instructions
├── state.rs        # Campaign + Contribution structs (borsh serialized)
└── error.rs        # Custom ProgramError variants
tests/
└── integration_tests.rs  # solana-program-test suite
```

## Instructions

| # | Instruction | Discriminator |
|---|---|---|
| 0 | `CreateCampaign { goal: u64, deadline: i64 }` | `0x00` |
| 1 | `Contribute { amount: u64 }` | `0x01` |
| 2 | `Withdraw` | `0x02` |
| 3 | `Refund` | `0x03` |

## PDA Seeds

| PDA | Seeds |
|---|---|
| Vault | `[b"vault", campaign_account_key]` |
| Contribution | `[b"contribution", campaign_account_key, donor_key]` |

## Prerequisites

```bash
# Install Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/v1.18.26/install)"

# Install Rust BPF target
cargo install --locked --git https://github.com/solana-labs/cargo-build-bpf cargo-build-bpf
# Or via platform tools:
solana-install update
```

## Build

```bash
cargo build-bpf
# Output: target/deploy/solana_crowdfunding.so
```

## Test (local)

```bash
cargo test-bpf
```

## Deploy to Devnet

```bash
# 1. Switch to devnet
solana config set --url devnet

# 2. Fund your wallet (repeat if rate limited)
solana airdrop 2
solana airdrop 2

# 3. Deploy
solana program deploy target/deploy/solana_crowdfunding.so

# 4. Save the Program ID printed above
```

## Instruction Data Encoding

All fields are **little-endian**.

| Instruction | Bytes |
|---|---|
| CreateCampaign | `[0x00] ++ goal(8 bytes LE) ++ deadline(8 bytes LE)` |
| Contribute | `[0x01] ++ amount(8 bytes LE)` |
| Withdraw | `[0x02]` |
| Refund | `[0x03]` |

## Error Codes

| Code | Name | Meaning |
|---|---|---|
| 0 | `DeadlineInPast` | Deadline must be in the future |
| 1 | `DeadlineNotReached` | Campaign still active |
| 2 | `GoalNotMet` | Cannot withdraw — goal not reached |
| 3 | `GoalAlreadyMet` | Cannot refund — campaign succeeded |
| 4 | `AlreadyClaimed` | Funds already withdrawn |
| 5 | `NotCreator` | Only creator can withdraw |
| 6 | `NothingToRefund` | No contribution found |
| 7 | `CampaignExpired` | Contributions not accepted after deadline |
| 8 | `ZeroContribution` | Amount must be > 0 |
| 9 | `InvalidVault` | Vault PDA mismatch |
| 10 | `InvalidContributionAccount` | Contribution PDA mismatch |
