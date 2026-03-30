use borsh::BorshSerialize;
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
            CrowdfundingInstruction::Withdraw => {
                Self::process_withdraw(program_id, accounts)
            }
            CrowdfundingInstruction::Refund => {
                Self::process_refund(program_id, accounts)
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. CREATE CAMPAIGN
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Accounts:
    //   0. [writable, signer] campaign_account  — pre-allocated by client
    //   1. [signer]           creator
    //   2. []                 system_program
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
        let system_program_account = next_account_info(accounts_iter)?;

        // --- Validate signers ---
        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // --- Validate system program ---
        if system_program_account.key != &system_program::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        // --- Validate deadline is in the future ---
        let clock = Clock::get()?;
        if deadline <= clock.unix_timestamp {
            return Err(CrowdfundingError::DeadlineInPast.into());
        }

        // --- Validate goal > 0 ---
        if goal == 0 {
            return Err(CrowdfundingError::ZeroContribution.into());
        }

        // --- Allocate campaign account if not already allocated ---
        if campaign_account.data_is_empty() {
            let rent = Rent::get()?;
            let space = Campaign::LEN;
            let lamports = rent.minimum_balance(space);

            invoke(
                &system_instruction::create_account(
                    creator.key,
                    campaign_account.key,
                    lamports,
                    space as u64,
                    program_id,
                ),
                &[
                    creator.clone(),
                    campaign_account.clone(),
                    system_program_account.clone(),
                ],
            )?;
        }

        // --- Verify ownership ---
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }

        // --- Write campaign data ---
        let campaign = Campaign::new(*creator.key, goal, deadline);
        campaign
            .serialize(&mut *campaign_account.data.borrow_mut())
            .map_err(|_| ProgramError::AccountDataTooSmall)?;

        msg!(
            "Campaign created: goal={}, deadline={}",
            goal,
            deadline
        );

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. CONTRIBUTE
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Accounts:
    //   0. [writable]         campaign_account
    //   1. [writable, signer] donor
    //   2. [writable]         vault_pda        — [b"vault", campaign_key]
    //   3. [writable]         contribution_pda — [b"contribution", campaign_key, donor_key]
    //   4. []                 system_program
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

        // --- Validate signers ---
        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // --- Validate amount ---
        if amount == 0 {
            return Err(CrowdfundingError::ZeroContribution.into());
        }

        // --- Load + validate campaign ---
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }
        let mut campaign =
            Campaign::try_from_slice_unchecked(&campaign_account.data.borrow())?;

        let clock = Clock::get()?;
        if clock.unix_timestamp >= campaign.deadline {
            return Err(CrowdfundingError::CampaignExpired.into());
        }

        // --- Validate vault PDA ---
        let (expected_vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &expected_vault {
            return Err(CrowdfundingError::InvalidVault.into());
        }

        // --- Validate contribution PDA ---
        let (expected_contribution_pda, contribution_bump) = Pubkey::find_program_address(
            &[b"contribution", campaign_account.key.as_ref(), donor.key.as_ref()],
            program_id,
        );
        if contribution_pda.key != &expected_contribution_pda {
            return Err(CrowdfundingError::InvalidContributionAccount.into());
        }

        // --- Create vault if it doesn't exist yet (first contribution ever) ---
        if vault_pda.data_is_empty() || vault_pda.lamports() == 0 {
            let rent = Rent::get()?;
            let vault_lamports = rent.minimum_balance(0);
            invoke_signed(
                &system_instruction::create_account(
                    donor.key,
                    vault_pda.key,
                    vault_lamports,
                    0,
                    program_id,
                ),
                &[donor.clone(), vault_pda.clone(), system_program_account.clone()],
                &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
            )?;
        }

        // --- Create contribution account if it doesn't exist yet ---
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
                &[donor.clone(), contribution_pda.clone(), system_program_account.clone()],
                &[&[
                    b"contribution",
                    campaign_account.key.as_ref(),
                    donor.key.as_ref(),
                    &[contribution_bump],
                ]],
            )?;

            // Initialize the contribution record
            let new_contribution = Contribution::new(*donor.key);
            new_contribution
                .serialize(&mut *contribution_pda.data.borrow_mut())
                .map_err(|_| ProgramError::AccountDataTooSmall)?;
        }

        // --- Transfer SOL from donor → vault ---
        invoke(
            &system_instruction::transfer(donor.key, vault_pda.key, amount),
            &[donor.clone(), vault_pda.clone(), system_program_account.clone()],
        )?;

        // --- Update campaign.raised ---
        campaign.raised = campaign
            .raised
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        campaign
            .serialize(&mut *campaign_account.data.borrow_mut())
            .map_err(|_| ProgramError::AccountDataTooSmall)?;

        // --- Update contribution record ---
        let mut contribution =
            Contribution::try_from_slice_unchecked(&contribution_pda.data.borrow())?;
        contribution.amount = contribution
            .amount
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        contribution
            .serialize(&mut *contribution_pda.data.borrow_mut())
            .map_err(|_| ProgramError::AccountDataTooSmall)?;

        msg!(
            "Contributed: {} lamports, total={}",
            amount,
            campaign.raised
        );

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

        // --- Validate signer ---
        if !creator.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // --- Load campaign ---
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }
        let mut campaign =
            Campaign::try_from_slice_unchecked(&campaign_account.data.borrow())?;

        // --- Only the creator can withdraw ---
        if &campaign.creator != creator.key {
            return Err(CrowdfundingError::NotCreator.into());
        }

        // --- Deadline must have passed ---
        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundingError::DeadlineNotReached.into());
        }

        // --- Goal must be met ---
        if campaign.raised < campaign.goal {
            return Err(CrowdfundingError::GoalNotMet.into());
        }

        // --- Prevent double withdrawal ---
        if campaign.claimed {
            return Err(CrowdfundingError::AlreadyClaimed.into());
        }

        // --- Validate vault PDA ---
        let (expected_vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &expected_vault {
            return Err(CrowdfundingError::InvalidVault.into());
        }

        // --- Transfer all lamports from vault → creator ---
        let vault_balance = vault_pda.lamports();
        if vault_balance == 0 {
            return Err(ProgramError::InsufficientFunds);
        }

        invoke_signed(
            &system_instruction::transfer(vault_pda.key, creator.key, vault_balance),
            &[
                vault_pda.clone(),
                creator.clone(),
                system_program_account.clone(),
            ],
            &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
        )?;

        // --- Mark as claimed ---
        campaign.claimed = true;
        campaign
            .serialize(&mut *campaign_account.data.borrow_mut())
            .map_err(|_| ProgramError::AccountDataTooSmall)?;

        msg!("Withdrawn: {} lamports", vault_balance);

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. REFUND
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Accounts:
    //   0. []                 campaign_account
    //   1. [writable, signer] donor
    //   2. [writable]         vault_pda        — [b"vault", campaign_key]
    //   3. [writable]         contribution_pda — [b"contribution", campaign_key, donor_key]
    //   4. []                 system_program
    //
    fn process_refund(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();
        let campaign_account = next_account_info(accounts_iter)?;
        let donor = next_account_info(accounts_iter)?;
        let vault_pda = next_account_info(accounts_iter)?;
        let contribution_pda = next_account_info(accounts_iter)?;
        let system_program_account = next_account_info(accounts_iter)?;

        // --- Validate signer ---
        if !donor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // --- Load campaign ---
        if campaign_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }
        let campaign = Campaign::try_from_slice_unchecked(&campaign_account.data.borrow())?;

        // --- Deadline must have passed ---
        let clock = Clock::get()?;
        if clock.unix_timestamp < campaign.deadline {
            return Err(CrowdfundingError::DeadlineNotReached.into());
        }

        // --- Campaign must have FAILED (goal not met) ---
        if campaign.raised >= campaign.goal {
            return Err(CrowdfundingError::GoalAlreadyMet.into());
        }

        // --- Validate vault PDA ---
        let (expected_vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", campaign_account.key.as_ref()],
            program_id,
        );
        if vault_pda.key != &expected_vault {
            return Err(CrowdfundingError::InvalidVault.into());
        }

        // --- Validate contribution PDA ---
        let (expected_contribution_pda, _) = Pubkey::find_program_address(
            &[b"contribution", campaign_account.key.as_ref(), donor.key.as_ref()],
            program_id,
        );
        if contribution_pda.key != &expected_contribution_pda {
            return Err(CrowdfundingError::InvalidContributionAccount.into());
        }

        // --- Load donor's contribution ---
        if contribution_pda.data_is_empty() {
            return Err(CrowdfundingError::NothingToRefund.into());
        }
        let mut contribution =
            Contribution::try_from_slice_unchecked(&contribution_pda.data.borrow())?;

        if contribution.amount == 0 {
            return Err(CrowdfundingError::NothingToRefund.into());
        }

        let refund_amount = contribution.amount;

        // --- Zero out contribution FIRST (prevent re-entrancy / double refund) ---
        contribution.amount = 0;
        contribution
            .serialize(&mut *contribution_pda.data.borrow_mut())
            .map_err(|_| ProgramError::AccountDataTooSmall)?;

        // --- Transfer refund from vault → donor ---
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
