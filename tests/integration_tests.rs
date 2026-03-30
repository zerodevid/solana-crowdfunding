use borsh::BorshSerialize;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    rent::Rent,
    system_program,
};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use solana_crowdfunding::{
    instruction::CrowdfundingInstruction,
    state::{Campaign, Contribution},
};

// ─── Helpers ────────────────────────────────────────────────────────────────

fn program_id() -> Pubkey {
    Pubkey::new_unique() // overridden by ProgramTest
}

fn vault_pda(campaign_key: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault", campaign_key.as_ref()], program_id)
}

fn contribution_pda(campaign_key: &Pubkey, donor_key: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"contribution", campaign_key.as_ref(), donor_key.as_ref()],
        program_id,
    )
}

/// Build a ProgramTest context with the crowdfunding program loaded
fn setup() -> (ProgramTest, Pubkey) {
    let program_id = Pubkey::new_unique();
    let pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    (pt, program_id)
}

// ─── Test 1: Create Campaign ─────────────────────────────────────────────────

#[tokio::test]
async fn test_create_campaign() {
    let (pt, program_id) = setup();
    let mut ctx = pt.start_with_context().await;

    let creator = Keypair::new();
    let campaign_account = Keypair::new();

    // Airdrop to creator
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[solana_sdk::system_instruction::transfer(
                &ctx.payer.pubkey(),
                &creator.pubkey(),
                10_000_000_000,
            )],
            Some(&ctx.payer.pubkey()),
            &[&ctx.payer],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    // deadline = now + 1 day
    let deadline = {
        let clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
        clock.unix_timestamp + 86_400
    };
    let goal: u64 = 1_000_000_000; // 1 SOL

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), true),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::create_campaign(goal, deadline),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&creator.pubkey()),
        &[&creator, &campaign_account],
        ctx.last_blockhash,
    );

    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Verify campaign data written correctly
    let acct = ctx
        .banks_client
        .get_account(campaign_account.pubkey())
        .await
        .unwrap()
        .expect("Campaign account should exist");

    let campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert_eq!(campaign.creator, creator.pubkey());
    assert_eq!(campaign.goal, goal);
    assert_eq!(campaign.deadline, deadline);
    assert_eq!(campaign.raised, 0);
    assert!(!campaign.claimed);
}

// ─── Test 2: Contribute ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_contribute() {
    let (pt, program_id) = setup();
    let mut ctx = pt.start_with_context().await;

    let creator = Keypair::new();
    let donor = Keypair::new();
    let campaign_account = Keypair::new();

    // Fund creator and donor
    for pk in [creator.pubkey(), donor.pubkey()] {
        ctx.banks_client
            .process_transaction(Transaction::new_signed_with_payer(
                &[solana_sdk::system_instruction::transfer(
                    &ctx.payer.pubkey(),
                    &pk,
                    10_000_000_000,
                )],
                Some(&ctx.payer.pubkey()),
                &[&ctx.payer],
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
    }

    let clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    let deadline = clock.unix_timestamp + 86_400;
    let goal: u64 = 2_000_000_000; // 2 SOL

    // Create campaign
    let create_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), true),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::create_campaign(goal, deadline),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[create_ix],
            Some(&creator.pubkey()),
            &[&creator, &campaign_account],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    // Contribute 600_000_000 lamports (0.6 SOL)
    let amount: u64 = 600_000_000;
    let (vault, _) = vault_pda(&campaign_account.pubkey(), &program_id);
    let (contrib_pda, _) = contribution_pda(&campaign_account.pubkey(), &donor.pubkey(), &program_id);

    let contribute_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), false),
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::contribute(amount),
    };

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[contribute_ix],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    // Verify campaign.raised updated
    let acct = ctx.banks_client.get_account(campaign_account.pubkey()).await.unwrap().unwrap();
    let campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert_eq!(campaign.raised, amount);

    // Verify contribution record
    let contrib_acct = ctx.banks_client.get_account(contrib_pda).await.unwrap().unwrap();
    let contribution = Contribution::try_from_slice_unchecked(&contrib_acct.data).unwrap();
    assert_eq!(contribution.amount, amount);
    assert_eq!(contribution.donor, donor.pubkey());
}

// ─── Test 3: Withdraw before deadline → should fail ──────────────────────────

#[tokio::test]
async fn test_withdraw_before_deadline_fails() {
    let (pt, program_id) = setup();
    let mut ctx = pt.start_with_context().await;

    let creator = Keypair::new();
    let campaign_account = Keypair::new();

    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[solana_sdk::system_instruction::transfer(
                &ctx.payer.pubkey(), &creator.pubkey(), 10_000_000_000,
            )],
            Some(&ctx.payer.pubkey()), &[&ctx.payer], ctx.last_blockhash,
        ))
        .await.unwrap();

    let clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    let deadline = clock.unix_timestamp + 86_400;
    let goal: u64 = 100_000_000;

    let create_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), true),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::create_campaign(goal, deadline),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[create_ix], Some(&creator.pubkey()), &[&creator, &campaign_account], ctx.last_blockhash,
        ))
        .await.unwrap();

    let (vault, _) = vault_pda(&campaign_account.pubkey(), &program_id);

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let withdraw_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), false),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::withdraw(),
    };

    let result = ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[withdraw_ix], Some(&creator.pubkey()), &[&creator], ctx.last_blockhash,
        ))
        .await;

    // Must fail — deadline not reached
    assert!(result.is_err(), "Withdraw before deadline should fail");
}

// ─── Test 4: Refund after failed campaign ────────────────────────────────────

#[tokio::test]
async fn test_refund_after_failed_campaign() {
    let (mut pt, program_id) = setup();

    // Set clock so deadline is 1 second in the future — we'll advance past it
    pt.set_creation_time(solana_sdk::timing::duration_as_s(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap(),
    ) as i64);

    let mut ctx = pt.start_with_context().await;
    let creator = Keypair::new();
    let donor = Keypair::new();
    let campaign_account = Keypair::new();

    for pk in [creator.pubkey(), donor.pubkey()] {
        ctx.banks_client
            .process_transaction(Transaction::new_signed_with_payer(
                &[solana_sdk::system_instruction::transfer(
                    &ctx.payer.pubkey(), &pk, 10_000_000_000,
                )],
                Some(&ctx.payer.pubkey()), &[&ctx.payer], ctx.last_blockhash,
            ))
            .await.unwrap();
    }

    let clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    // deadline 2 seconds from now — we'll warp past it
    let deadline = clock.unix_timestamp + 2;
    let goal: u64 = 5_000_000_000; // 5 SOL — will NOT be met

    // Create campaign
    let create_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), true),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::create_campaign(goal, deadline),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[create_ix], Some(&creator.pubkey()), &[&creator, &campaign_account], ctx.last_blockhash,
        ))
        .await.unwrap();

    // Contribute 100_000_000 lamports (far below goal)
    let amount: u64 = 100_000_000;
    let (vault, _) = vault_pda(&campaign_account.pubkey(), &program_id);
    let (contrib_pda, _) = contribution_pda(&campaign_account.pubkey(), &donor.pubkey(), &program_id);

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let contribute_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), false),
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::contribute(amount),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[contribute_ix], Some(&donor.pubkey()), &[&donor], ctx.last_blockhash,
        ))
        .await.unwrap();

    // Warp past the deadline
    ctx.warp_to_slot(500).unwrap();
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();

    let donor_balance_before = ctx.banks_client.get_balance(donor.pubkey()).await.unwrap();

    // Refund
    let refund_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(campaign_account.pubkey(), false),
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::refund(),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[refund_ix], Some(&donor.pubkey()), &[&donor], ctx.last_blockhash,
        ))
        .await.unwrap();

    let donor_balance_after = ctx.banks_client.get_balance(donor.pubkey()).await.unwrap();
    // Donor should have gotten their lamports back (minus tx fee)
    assert!(
        donor_balance_after > donor_balance_before,
        "Donor should receive refund"
    );

    // Contribution record should be zeroed
    let contrib_acct = ctx.banks_client.get_account(contrib_pda).await.unwrap().unwrap();
    let contribution = Contribution::try_from_slice_unchecked(&contrib_acct.data).unwrap();
    assert_eq!(contribution.amount, 0, "Contribution should be zeroed after refund");
}

// ─── Test 5: Double refund fails ─────────────────────────────────────────────

#[tokio::test]
async fn test_double_refund_fails() {
    let (mut pt, program_id) = setup();
    let mut ctx = pt.start_with_context().await;

    let creator = Keypair::new();
    let donor = Keypair::new();
    let campaign_account = Keypair::new();

    for pk in [creator.pubkey(), donor.pubkey()] {
        ctx.banks_client
            .process_transaction(Transaction::new_signed_with_payer(
                &[solana_sdk::system_instruction::transfer(
                    &ctx.payer.pubkey(), &pk, 10_000_000_000,
                )],
                Some(&ctx.payer.pubkey()), &[&ctx.payer], ctx.last_blockhash,
            ))
            .await.unwrap();
    }

    let clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    let deadline = clock.unix_timestamp + 2;
    let goal: u64 = 5_000_000_000;

    let create_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), true),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::create_campaign(goal, deadline),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[create_ix], Some(&creator.pubkey()), &[&creator, &campaign_account], ctx.last_blockhash,
        ))
        .await.unwrap();

    let amount: u64 = 100_000_000;
    let (vault, _) = vault_pda(&campaign_account.pubkey(), &program_id);
    let (contrib_pda, _) = contribution_pda(&campaign_account.pubkey(), &donor.pubkey(), &program_id);

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let contribute_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_account.pubkey(), false),
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::contribute(amount),
    };
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[contribute_ix], Some(&donor.pubkey()), &[&donor], ctx.last_blockhash,
        ))
        .await.unwrap();

    // Warp past deadline
    ctx.warp_to_slot(500).unwrap();
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();

    let make_refund_ix = || Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(campaign_account.pubkey(), false),
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::refund(),
    };

    // First refund should succeed
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[make_refund_ix()], Some(&donor.pubkey()), &[&donor], ctx.last_blockhash,
        ))
        .await.unwrap();

    // Second refund should FAIL
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let result = ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[make_refund_ix()], Some(&donor.pubkey()), &[&donor], ctx.last_blockhash,
        ))
        .await;

    assert!(result.is_err(), "Second refund should fail");
}
