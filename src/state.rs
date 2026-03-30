use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    program_error::ProgramError,
    pubkey::Pubkey,
};

/// Size constants
pub const CAMPAIGN_LEN: usize = 32 + 8 + 8 + 8 + 1; // 57 bytes
pub const CONTRIBUTION_LEN: usize = 32 + 8;           // 40 bytes

/// Campaign account data stored on-chain
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct Campaign {
    pub creator: Pubkey,  // 32 bytes — who created the campaign
    pub goal: u64,        // 8 bytes  — target amount in lamports
    pub raised: u64,      // 8 bytes  — total raised so far
    pub deadline: i64,    // 8 bytes  — unix timestamp when campaign ends
    pub claimed: bool,    // 1 byte   — whether creator has withdrawn
}

impl Campaign {
    pub const LEN: usize = CAMPAIGN_LEN;

    pub fn new(creator: Pubkey, goal: u64, deadline: i64) -> Self {
        Self {
            creator,
            goal,
            raised: 0,
            deadline,
            claimed: false,
        }
    }

    pub fn try_from_slice_unchecked(data: &[u8]) -> Result<Self, ProgramError> {
        Self::try_from_slice(data).map_err(|_| ProgramError::InvalidAccountData)
    }
}

/// Per-donor contribution record stored in a PDA
/// Seeds: [b"contribution", campaign_key, donor_key]
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct Contribution {
    pub donor: Pubkey,  // 32 bytes — who donated
    pub amount: u64,    // 8 bytes  — how much they donated (lamports)
}

impl Contribution {
    pub const LEN: usize = CONTRIBUTION_LEN;

    pub fn new(donor: Pubkey) -> Self {
        Self { donor, amount: 0 }
    }

    pub fn try_from_slice_unchecked(data: &[u8]) -> Result<Self, ProgramError> {
        Self::try_from_slice(data).map_err(|_| ProgramError::InvalidAccountData)
    }
}
