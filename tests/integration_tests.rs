use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
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

fn vault_pda(campaign_key: &Pubkey, program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"vault", campaign_key.as_ref()], program_id).0
}

fn contribution_pda(campaign_key: &Pubkey, donor_key: &Pubkey, program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"contribution", campaign_key.as_ref(), donor_key.as_ref()],
        program_id,
    )
    .0
}

async fn fund(ctx: &mut ProgramTestContext, targets: &[Pubkey]) {
    for pk in targets {
        ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
        ctx.banks_client
            .process_transaction(Transaction::new_signed_with_payer(
                &[solana_sdk::system_instruction::transfer(
                    &ctx.payer.pubkey(),
                    pk,
                    2_000_000_000_000, // 2000 SOL
                )],
                Some(&ctx.payer.pubkey()),
                &[&ctx.payer],
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
    }
}

/// Inject a pre-allocated, program-owned campaign account at genesis.
/// This avoids needing the program to CPI create_account for campaign.
fn inject_campaign(pt: &mut ProgramTest, campaign_key: Pubkey, program_id: Pubkey) {
    let rent = solana_program::rent::Rent::default();
    pt.add_account(
        campaign_key,
        Account {
            lamports: rent.minimum_balance(Campaign::LEN),
            data: vec![0u8; Campaign::LEN],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    );
}

// ─── Test 1: Create Campaign ─────────────────────────────────────────────────

#[tokio::test]
async fn test_create_campaign() {
    let program_id = Pubkey::new_unique();
    let campaign_keypair = Keypair::new();
    let creator = Keypair::new();

    let mut pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    inject_campaign(&mut pt, campaign_keypair.pubkey(), program_id);

    let mut ctx = pt.start_with_context().await;
    fund(&mut ctx, &[creator.pubkey()]).await;

    let clock = ctx
        .banks_client
        .get_sysvar::<solana_program::clock::Clock>()
        .await
        .unwrap();
    let goal: u64 = 1_000_000_000;
    let deadline = clock.unix_timestamp + 86_400;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new_readonly(creator.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::create_campaign(goal, deadline),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let acct = ctx
        .banks_client
        .get_account(campaign_keypair.pubkey())
        .await
        .unwrap()
        .expect("Campaign account should exist");

    let campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert_eq!(campaign.creator, creator.pubkey());
    assert_eq!(campaign.goal, goal);
    assert_eq!(campaign.deadline, deadline);
    assert_eq!(campaign.raised, 0);
    assert!(!campaign.claimed);

    println!("✅ test_create_campaign passed");
}

// ─── Test 2: Contribute ──────────────────────────────────────────────────────
// vault + contribution_pda are created by processor via invoke_signed CPI

#[tokio::test]
async fn test_contribute() {
    let program_id = Pubkey::new_unique();
    let campaign_keypair = Keypair::new();
    let creator = Keypair::new();
    let donor = Keypair::new();

    let mut pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    inject_campaign(&mut pt, campaign_keypair.pubkey(), program_id);

    let mut ctx = pt.start_with_context().await;
    fund(&mut ctx, &[creator.pubkey(), donor.pubkey()]).await;

    let clock = ctx
        .banks_client
        .get_sysvar::<solana_program::clock::Clock>()
        .await
        .unwrap();
    let goal: u64 = 2_000_000_000;
    let deadline = clock.unix_timestamp + 86_400;

    // Create campaign
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new_readonly(creator.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::create_campaign(goal, deadline),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let vault = vault_pda(&campaign_keypair.pubkey(), &program_id);
    let contrib_pda =
        contribution_pda(&campaign_keypair.pubkey(), &donor.pubkey(), &program_id);
    let amount: u64 = 600_000_000;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new(donor.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(contrib_pda, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::contribute(amount),
            }],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let acct = ctx
        .banks_client
        .get_account(campaign_keypair.pubkey())
        .await
        .unwrap()
        .unwrap();
    let campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert_eq!(campaign.raised, amount);

    let contrib_acct = ctx
        .banks_client
        .get_account(contrib_pda)
        .await
        .unwrap()
        .unwrap();
    let contribution = Contribution::try_from_slice_unchecked(&contrib_acct.data).unwrap();
    assert_eq!(contribution.amount, amount);

    println!("✅ test_contribute passed: raised={}", campaign.raised);
}

// ─── Test 3: Withdraw before deadline → should fail ──────────────────────────

#[tokio::test]
async fn test_withdraw_before_deadline_fails() {
    let program_id = Pubkey::new_unique();
    let campaign_keypair = Keypair::new();
    let creator = Keypair::new();

    let mut pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    inject_campaign(&mut pt, campaign_keypair.pubkey(), program_id);

    let mut ctx = pt.start_with_context().await;
    fund(&mut ctx, &[creator.pubkey()]).await;

    let clock = ctx
        .banks_client
        .get_sysvar::<solana_program::clock::Clock>()
        .await
        .unwrap();
    let deadline = clock.unix_timestamp + 86_400;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new_readonly(creator.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::create_campaign(100_000_000, deadline),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let vault = vault_pda(&campaign_keypair.pubkey(), &program_id);

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let result = ctx
        .banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new(creator.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::withdraw(),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await;

    assert!(result.is_err(), "Withdraw before deadline should fail");
    println!("✅ test_withdraw_before_deadline_fails passed");
}

// ─── Test 4: Refund after failed campaign ────────────────────────────────────

#[tokio::test]
async fn test_refund_after_failed_campaign() {
    let program_id = Pubkey::new_unique();
    let campaign_keypair = Keypair::new();
    let creator = Keypair::new();
    let donor = Keypair::new();

    let mut pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    inject_campaign(&mut pt, campaign_keypair.pubkey(), program_id);

    let mut ctx = pt.start_with_context().await;
    fund(&mut ctx, &[creator.pubkey(), donor.pubkey()]).await;

    let clock = ctx
        .banks_client
        .get_sysvar::<solana_program::clock::Clock>()
        .await
        .unwrap();
    let deadline = clock.unix_timestamp + 1; // expires soon
    let goal: u64 = 5_000_000_000;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new_readonly(creator.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::create_campaign(goal, deadline),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let vault = vault_pda(&campaign_keypair.pubkey(), &program_id);
    let contrib_pda =
        contribution_pda(&campaign_keypair.pubkey(), &donor.pubkey(), &program_id);
    let amount: u64 = 100_000_000;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new(donor.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(contrib_pda, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::contribute(amount),
            }],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    // Warp past deadline: advance slot AND override clock timestamp
    ctx.warp_to_slot(500).unwrap();
    // Override the on-chain clock so unix_timestamp > deadline
    let mut clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    clock.unix_timestamp = deadline + 100;
    ctx.set_sysvar(&clock);
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();

    let donor_balance_before = ctx.banks_client.get_balance(donor.pubkey()).await.unwrap();

    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new_readonly(campaign_keypair.pubkey(), false),
                    AccountMeta::new(donor.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(contrib_pda, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::refund(),
            }],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let donor_balance_after = ctx.banks_client.get_balance(donor.pubkey()).await.unwrap();
    assert!(
        donor_balance_after > donor_balance_before,
        "Donor should receive refund"
    );

    let contrib_acct = ctx
        .banks_client
        .get_account(contrib_pda)
        .await
        .unwrap()
        .unwrap();
    let contribution = Contribution::try_from_slice_unchecked(&contrib_acct.data).unwrap();
    assert_eq!(contribution.amount, 0);

    println!("✅ test_refund_after_failed_campaign passed");
}

// ─── Test 5: Double refund must fail ─────────────────────────────────────────

#[tokio::test]
async fn test_double_refund_fails() {
    let program_id = Pubkey::new_unique();
    let campaign_keypair = Keypair::new();
    let creator = Keypair::new();
    let donor = Keypair::new();

    let mut pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    inject_campaign(&mut pt, campaign_keypair.pubkey(), program_id);

    let mut ctx = pt.start_with_context().await;
    fund(&mut ctx, &[creator.pubkey(), donor.pubkey()]).await;

    let clock = ctx
        .banks_client
        .get_sysvar::<solana_program::clock::Clock>()
        .await
        .unwrap();
    let deadline = clock.unix_timestamp + 1;
    let goal: u64 = 5_000_000_000;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new_readonly(creator.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::create_campaign(goal, deadline),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let vault = vault_pda(&campaign_keypair.pubkey(), &program_id);
    let contrib_pda =
        contribution_pda(&campaign_keypair.pubkey(), &donor.pubkey(), &program_id);
    let amount: u64 = 100_000_000;

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new(donor.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(contrib_pda, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::contribute(amount),
            }],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    // Warp past deadline: advance slot AND override clock timestamp
    ctx.warp_to_slot(500).unwrap();
    let mut clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    clock.unix_timestamp = deadline + 100;
    ctx.set_sysvar(&clock);

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();

    let make_refund_ix = || Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(campaign_keypair.pubkey(), false),
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::refund(),
    };

    // First refund must succeed
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[make_refund_ix()],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    // Second refund must FAIL
    // We append a dummy transfer instruction so the transaction signature is different from the first one.
    // Otherwise, BanksClient just returns Ok(()) caching the success of the first transaction!
    let second_tx = Transaction::new_signed_with_payer(
        &[
            make_refund_ix(),
            solana_sdk::system_instruction::transfer(&donor.pubkey(), &donor.pubkey(), 1),
        ],
        Some(&donor.pubkey()),
        &[&donor],
        ctx.last_blockhash,
    );
    let result = ctx.banks_client.process_transaction(second_tx).await;

    assert!(result.is_err(), "Second refund should fail");
    println!("✅ test_double_refund_fails passed");
}

// ─── Test 6: Testing Checklist Flow ──────────────────────────────────────────

#[tokio::test]
async fn test_checklist_flow() {
    let program_id = Pubkey::new_unique();
    let campaign_keypair = Keypair::new();
    let creator = Keypair::new();
    let donor = Keypair::new();

    let mut pt = ProgramTest::new(
        "solana_crowdfunding",
        program_id,
        processor!(solana_crowdfunding::processor::Processor::process),
    );
    inject_campaign(&mut pt, campaign_keypair.pubkey(), program_id);

    let mut ctx = pt.start_with_context().await;
    fund(&mut ctx, &[creator.pubkey(), donor.pubkey()]).await;

    let clock = ctx
        .banks_client
        .get_sysvar::<solana_program::clock::Clock>()
        .await
        .unwrap();
    
    // 1. Create a campaign with goal=1000 SOL, deadline=tomorrow
    let goal: u64 = 1_000 * 1_000_000_000; // 1000 SOL in lamports
    let deadline = clock.unix_timestamp + 86_400; // tomorrow

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new_readonly(creator.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::create_campaign(goal, deadline),
            }],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let vault = vault_pda(&campaign_keypair.pubkey(), &program_id);
    let contrib_pda =
        contribution_pda(&campaign_keypair.pubkey(), &donor.pubkey(), &program_id);

    // 2. Contribute 600 SOL → should succeed, raised=600
    let amount_1: u64 = 600 * 1_000_000_000;
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new(donor.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(contrib_pda, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::contribute(amount_1),
            }],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let acct = ctx.banks_client.get_account(campaign_keypair.pubkey()).await.unwrap().unwrap();
    let mut campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert_eq!(campaign.raised, amount_1); // raised=600

    // 3. Contribute 500 SOL → should succeed, raised=1100
    let amount_2: u64 = 500 * 1_000_000_000;
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(campaign_keypair.pubkey(), false),
                    AccountMeta::new(donor.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(contrib_pda, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: CrowdfundingInstruction::contribute(amount_2),
            }],
            Some(&donor.pubkey()),
            &[&donor],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();

    let acct = ctx.banks_client.get_account(campaign_keypair.pubkey()).await.unwrap().unwrap();
    campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert_eq!(campaign.raised, amount_1 + amount_2); // raised=1100

    // 4. Try withdraw before deadline → should fail
    let make_withdraw_ix = || Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign_keypair.pubkey(), false),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: CrowdfundingInstruction::withdraw(),
    };

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let result = ctx
        .banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[make_withdraw_ix()],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await;
    assert!(result.is_err(), "Withdraw before deadline should fail");

    // 5. Wait until after deadline → withdraw should succeed
    ctx.warp_to_slot(1000).unwrap();
    let mut clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    clock.unix_timestamp = deadline + 100;
    ctx.set_sysvar(&clock);

    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    ctx.banks_client
        .process_transaction(Transaction::new_signed_with_payer(
            &[make_withdraw_ix()],
            Some(&creator.pubkey()),
            &[&creator],
            ctx.last_blockhash,
        ))
        .await
        .unwrap();
    
    let acct = ctx.banks_client.get_account(campaign_keypair.pubkey()).await.unwrap().unwrap();
    campaign = Campaign::try_from_slice_unchecked(&acct.data).unwrap();
    assert!(campaign.claimed, "Campaign should be marked as claimed");

    // 6. Try withdraw again → should fail (already claimed)
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let second_withdraw_tx = Transaction::new_signed_with_payer(
        &[
            make_withdraw_ix(),
            solana_sdk::system_instruction::transfer(&creator.pubkey(), &creator.pubkey(), 1), // dummy exact diff
        ],
        Some(&creator.pubkey()),
        &[&creator],
        ctx.last_blockhash,
    );
    let result2 = ctx.banks_client.process_transaction(second_withdraw_tx).await;
    assert!(result2.is_err(), "Withdraw again should fail");

    println!("✅ test_checklist_flow passed");
}

