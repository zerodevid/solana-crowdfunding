use borsh::to_vec;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    system_program,
    sysvar::Sysvar,
};

use crate::{
    error::CrowdfundingError,
    instruction::CrowdfundingInstruction,
    state::{Campaign, Contribution},
};

pub struct Processor;

impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = CrowdfundingInstruction::unpack(instruction_data)?;

        match instruction {
            CrowdfundingInstruction::CreateCampaign { goal, deadline } => {
                Self::process_create_campaign(program_id, accounts, goal, deadline)
            }
            CrowdfundingInstruction::Contribute { amount } => {
                Self::process_contribute(program_id, accounts, amount)
            }
            CrowdfundingInstruction::Withdraw => Self::process_withdraw(program_id, accounts),
            CrowdfundingInstruction::Refund => Self::process_refund(program_id, accounts),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. CREATE CAMPAIGN
    // ─────────────────────────────────────────────────────────────────────────
    //
    // campaign_account: pre-allocated by client, owned by program_id, size=Campaign::LEN
    //
    // Accounts:
    //   0. [writable]  campaign_account  — owned by program, pre-allocated
    //   1. [signer]    creator
    //   2. []          system_program
    //
    fn process_create_campaign(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        goal: u64,
        deadline: i64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let campaign_account = next_account_info(accounts_iter)?;
        let creator = next_account_info(accounts_iter)?;
        let _system_program = next_account_info(accounts_iter)?;

        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }
        if campaign_account.data_len() < Campaign::LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }

        let clock = Clock::get()?;
        if deadline <= clock.unix_timestamp {
            return Err(CrowdfundingError::DeadlineInPast.into());
        }
        if goal == 0 {
            return Err(CrowdfundingError::ZeroContribution.into());
        }

        let campaign = Campaign::new(*creator.key, goal, deadline);
        let data = to_vec(&campaign).map_err(|_| ProgramError::InvalidAccountData)?;
        campaign_account.data.borrow_mut()[..data.len()].copy_from_slice(&data);

        msg!("Campaign created: goal={}, deadline={}", goal, deadline);
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. CONTRIBUTE
    // ─────────────────────────────────────────────────────────────────────────
    //
    // vault_pda:        System-program-owned PDA — funds held here
    // contribution_pda: Program-owned PDA — tracks per-donor amount
    //
    // Accounts:
    //   0. [writable]  campaign_account
    //   1. [writable, signer] donor
    //   2. [writable]  vault_pda        — System-owned PDA: [b"vault", campaign_key]
    //   3. [writable]  contribution_pda — Program-owned PDA: [b"contribution", campaign_key, donor_key]
    //   4. []          system_program
    //
    fn process_contribute(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        amount: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let campaign_account = next_account_info(accounts_iter)?;
        let donor = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let contribution_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if amount == 0 {
            return Err(CrowdfundingError::ZeroContribution.into());
        }
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }

        let mut campaign = Campaign::try_from_slice_unchecked(&campaign_account.data.borrow())?;

        let clock = Clock::get()?;
        if clock.unix_timestamp >= campaign.deadline {
            return Err(CrowdfundingError::CampaignExpired.into());
        }

        // Validate vault PDA
        let (expected_vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &expected_vault {
            return Err(CrowdfundingError::InvalidVault.into());
        }

        // Validate contribution PDA
        let (expected_contribution_pda, contribution_bump) = Pubkey::find_program_address(
            &[
                b"contribution",
                campaign_account.key.as_ref(),
                donor.key.as_ref(),
            ],
            program_id,
        );
        if contribution_pda.key != &expected_contribution_pda {
            return Err(CrowdfundingError::InvalidContributionAccount.into());
        }

        // Create vault PDA if it doesn't exist yet (System-owned, zero data)
        // vault is owned by System Program so normal system_instruction::transfer works
        if vault_pda.lamports() == 0 {
            let rent = Rent::get()?;
            let vault_lamports = rent.minimum_balance(0);
            invoke_signed(
                &system_instruction::create_account(
                    donor.key,
                    vault_pda.key,
                    vault_lamports,
                    0,
                    &system_program::ID, // System Program owns the vault
                ),
                &[
                    donor.clone(),
                    vault_pda.clone(),
                    system_program_account.clone(),
                ],
                &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
            )?;
        }

        // Create contribution PDA if it doesn't exist (Program-owned, size=Contribution::LEN)
        if contribution_pda.data_is_empty() {
            let rent = Rent::get()?;
            let contribution_lamports = rent.minimum_balance(Contribution::LEN);
            invoke_signed(
                &system_instruction::create_account(
                    donor.key,
                    contribution_pda.key,
                    contribution_lamports,
                    Contribution::LEN as u64,
                    program_id,
                ),
                &[
                    donor.clone(),
                    contribution_pda.clone(),
                    system_program_account.clone(),
                ],
                &[&[
                    b"contribution",
                    campaign_account.key.as_ref(),
                    donor.key.as_ref(),
                    &[contribution_bump],
                ]],
            )?;

            let new_contribution = Contribution::new(*donor.key);
            let init_data =
                to_vec(&new_contribution).map_err(|_| ProgramError::InvalidAccountData)?;
            contribution_pda.data.borrow_mut()[..init_data.len()].copy_from_slice(&init_data);
        }

        // Transfer SOL from donor → vault (both System-Program-owned, standard transfer)
        invoke(
            &system_instruction::transfer(donor.key, vault_pda.key, amount),
            &[
                donor.clone(),
                vault_pda.clone(),
                system_program_account.clone(),
            ],
        )?;

        // Update campaign.raised
        campaign.raised = campaign
            .raised
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let campaign_data = to_vec(&campaign).map_err(|_| ProgramError::InvalidAccountData)?;
        campaign_account.data.borrow_mut()[..campaign_data.len()].copy_from_slice(&campaign_data);

        // Update contribution record
        let mut contribution =
            Contribution::try_from_slice_unchecked(&contribution_pda.data.borrow())?;
        contribution.amount = contribution
            .amount
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let contrib_data = to_vec(&contribution).map_err(|_| ProgramError::InvalidAccountData)?;
        contribution_pda.data.borrow_mut()[..contrib_data.len()].copy_from_slice(&contrib_data);

        msg!("Contributed: {} lamports, total={}", amount, campaign.raised);
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. WITHDRAW
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Accounts:
    //   0. [writable]  campaign_account
    //   1. [signer]    creator
    //   2. [writable]  vault_pda — [b"vault", campaign_key]
    //   3. []          system_program
    //
    fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let campaign_account = next_account_info(accounts_iter)?;
        let creator = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }

        let mut campaign = Campaign::try_from_slice_unchecked(&campaign_account.data.borrow())?;

        if &campaign.creator != creator.key {
            return Err(CrowdfundingError::NotCreator.into());
        }

        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundingError::DeadlineNotReached.into());
        }
        if campaign.raised < campaign.goal {
            return Err(CrowdfundingError::GoalNotMet.into());
        }
        if campaign.claimed {
            return Err(CrowdfundingError::AlreadyClaimed.into());
        }

        let (expected_vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &expected_vault {
            return Err(CrowdfundingError::InvalidVault.into());
        }

        let vault_balance = vault_pda.lamports();
        if vault_balance == 0 {
            return Err(ProgramError::InsufficientFunds);
        }

        // vault is System-owned PDA → invoke_signed with PDA seeds to authorize
        invoke_signed(
            &system_instruction::transfer(vault_pda.key, creator.key, vault_balance),
            &[
                vault_pda.clone(),
                creator.clone(),
                system_program_account.clone(),
            ],
            &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
        )?;

        campaign.claimed = true;
        let campaign_data = to_vec(&campaign).map_err(|_| ProgramError::InvalidAccountData)?;
        campaign_account.data.borrow_mut()[..campaign_data.len()].copy_from_slice(&campaign_data);

        msg!("Withdrawn: {} lamports", vault_balance);
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. REFUND
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Accounts:
    //   0. []                  campaign_account
    //   1. [writable, signer]  donor
    //   2. [writable]          vault_pda        — [b"vault", campaign_key]
    //   3. [writable]          contribution_pda — [b"contribution", campaign_key, donor_key]
    //   4. []                  system_program
    //
    fn process_refund(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let campaign_account = next_account_info(accounts_iter)?;
        let donor = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let contribution_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }

        let campaign = Campaign::try_from_slice_unchecked(&campaign_account.data.borrow())?;

        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundingError::DeadlineNotReached.into());
        }
        if campaign.raised >= campaign.goal {
            return Err(CrowdfundingError::GoalAlreadyMet.into());
        }

        let (expected_vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &expected_vault {
            return Err(CrowdfundingError::InvalidVault.into());
        }

        let (expected_contribution_pda, _) = Pubkey::find_program_address(
            &[
                b"contribution",
                campaign_account.key.as_ref(),
                donor.key.as_ref(),
            ],
            program_id,
        );
        if contribution_pda.key != &expected_contribution_pda {
            return Err(CrowdfundingError::InvalidContributionAccount.into());
        }

        if contribution_pda.data_is_empty() {
            return Err(CrowdfundingError::NothingToRefund.into());
        }

        let mut contribution =
            Contribution::try_from_slice_unchecked(&contribution_pda.data.borrow())?;
        if contribution.amount == 0 {
            return Err(CrowdfundingError::NothingToRefund.into());
        }

        let refund_amount = contribution.amount;

        // Zero contribution FIRST (prevent double refund)
        contribution.amount = 0;
        let contrib_data = to_vec(&contribution).map_err(|_| ProgramError::InvalidAccountData)?;
        contribution_pda.data.borrow_mut()[..contrib_data.len()].copy_from_slice(&contrib_data);

        // vault is System-owned PDA → invoke_signed with PDA seeds
        invoke_signed(
            &system_instruction::transfer(vault_pda.key, donor.key, refund_amount),
            &[
                vault_pda.clone(),
                donor.clone(),
                system_program_account.clone(),
            ],
            &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
        )?;

        msg!("Refunded: {} lamports", refund_amount);
        Ok(())
    }
}
