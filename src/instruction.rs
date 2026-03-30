use solana_program::program_error::ProgramError;

/// Discriminator bytes for each instruction
pub const CREATE_CAMPAIGN: u8 = 0;
pub const CONTRIBUTE: u8 = 1;
pub const WITHDRAW: u8 = 2;
pub const REFUND: u8 = 3;

/// All instructions supported by this program
#[derive(Debug)]
pub enum CrowdfundingInstruction {
    /// Create a new fundraising campaign.
    ///
    /// Accounts:
    ///   0. [writable, signer] campaign_account — new account (creator must pre-allocate)
    ///   1. [signer]           creator
    ///   2. []                 system_program
    CreateCampaign {
        goal: u64,      // Target amount in lamports
        deadline: i64,  // Unix timestamp (seconds)
    },

    /// Contribute SOL to a campaign's vault.
    ///
    /// Accounts:
    ///   0. [writable]         campaign_account
    ///   1. [writable, signer] donor
    ///   2. [writable]         vault_pda       — PDA: [b"vault", campaign_key]
    ///   3. [writable]         contribution_pda — PDA: [b"contribution", campaign_key, donor_key]
    ///   4. []                 system_program
    Contribute {
        amount: u64, // Lamports to donate
    },

    /// Withdraw funds after a successful campaign (goal met + deadline passed).
    ///
    /// Accounts:
    ///   0. [writable]         campaign_account
    ///   1. [signer]           creator
    ///   2. [writable]         vault_pda — PDA: [b"vault", campaign_key]
    ///   3. []                 system_program
    Withdraw,

    /// Refund a donor's contribution if the campaign failed (goal not met after deadline).
    ///
    /// Accounts:
    ///   0. []                 campaign_account
    ///   1. [writable, signer] donor
    ///   2. [writable]         vault_pda        — PDA: [b"vault", campaign_key]
    ///   3. [writable]         contribution_pda — PDA: [b"contribution", campaign_key, donor_key]
    ///   4. []                 system_program
    Refund,
}

impl CrowdfundingInstruction {
    /// Decode an instruction from raw bytes.
    /// Layout:
    ///   [0]     = discriminator (1 byte)
    ///   [1..9]  = goal (u64 LE) — for CreateCampaign
    ///   [9..17] = deadline (i64 LE) — for CreateCampaign
    ///   [1..9]  = amount (u64 LE) — for Contribute
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (tag, rest) = input
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;

        match *tag {
            CREATE_CAMPAIGN => {
                if rest.len() < 16 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let goal = u64::from_le_bytes(
                    rest[0..8].try_into().map_err(|_| ProgramError::InvalidInstructionData)?
                );
                let deadline = i64::from_le_bytes(
                    rest[8..16].try_into().map_err(|_| ProgramError::InvalidInstructionData)?
                );
                Ok(CrowdfundingInstruction::CreateCampaign { goal, deadline })
            }

            CONTRIBUTE => {
                if rest.len() < 8 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let amount = u64::from_le_bytes(
                    rest[0..8].try_into().map_err(|_| ProgramError::InvalidInstructionData)?
                );
                Ok(CrowdfundingInstruction::Contribute { amount })
            }

            WITHDRAW => Ok(CrowdfundingInstruction::Withdraw),

            REFUND => Ok(CrowdfundingInstruction::Refund),

            _ => Err(ProgramError::InvalidInstructionData),
        }
    }

    /// Encode CreateCampaign instruction for clients
    pub fn create_campaign(goal: u64, deadline: i64) -> Vec<u8> {
        let mut data = vec![CREATE_CAMPAIGN];
        data.extend_from_slice(&goal.to_le_bytes());
        data.extend_from_slice(&deadline.to_le_bytes());
        data
    }

    /// Encode Contribute instruction for clients
    pub fn contribute(amount: u64) -> Vec<u8> {
        let mut data = vec![CONTRIBUTE];
        data.extend_from_slice(&amount.to_le_bytes());
        data
    }

    /// Encode Withdraw instruction for clients
    pub fn withdraw() -> Vec<u8> {
        vec![WITHDRAW]
    }

    /// Encode Refund instruction for clients
    pub fn refund() -> Vec<u8> {
        vec![REFUND]
    }
}
