use solana_program::program_error::ProgramError;

/// All custom errors for the crowdfunding program.
/// Each maps to a unique ProgramError::Custom(u32) code.
#[derive(Debug, thiserror::Error)]
pub enum CrowdfundingError {
    #[error("Deadline must be in the future")]
    DeadlineInPast,          // 0

    #[error("Campaign deadline has not been reached yet")]
    DeadlineNotReached,      // 1

    #[error("Campaign has not reached its goal")]
    GoalNotMet,              // 2

    #[error("Campaign has already reached its goal — refunds not allowed")]
    GoalAlreadyMet,          // 3

    #[error("Funds have already been withdrawn")]
    AlreadyClaimed,          // 4

    #[error("Only the campaign creator can withdraw funds")]
    NotCreator,              // 5

    #[error("No contribution found for this donor")]
    NothingToRefund,         // 6

    #[error("Campaign is still active — contributions not accepted after deadline")]
    CampaignExpired,         // 7

    #[error("Contribution amount must be greater than zero")]
    ZeroContribution,        // 8

    #[error("Invalid vault account — PDA mismatch")]
    InvalidVault,            // 9

    #[error("Invalid contribution account — PDA mismatch")]
    InvalidContributionAccount, // 10
}

impl From<CrowdfundingError> for ProgramError {
    fn from(e: CrowdfundingError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
