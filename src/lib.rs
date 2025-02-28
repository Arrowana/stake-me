use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::entrypoint;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    stake::{
        instruction::{authorize, delegate_stake},
        state::StakeAuthorize,
    },
};

entrypoint!(process_instruction);

#[derive(BorshSerialize, BorshDeserialize)]
pub enum StakeMeInstruction {
    Stake { target_stake_authority: Pubkey },
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if let Ok(stake_me_instruction) = StakeMeInstruction::try_from_slice(instruction_data) {
        match stake_me_instruction {
            StakeMeInstruction::Stake {
                target_stake_authority,
            } => {
                let [stake_ai, vote_account_ai, clock_ai, stake_history_ai, stake_config_ai, stake_authority_ai, _stake_program_ai] =
                    accounts
                else {
                    return Err(ProgramError::NotEnoughAccountKeys);
                };

                let delegate_stake_instruction =
                    delegate_stake(stake_ai.key, stake_authority_ai.key, &vote_account_ai.key);

                let seeds: &[&[u8]] = &[
                    b"stake",
                    vote_account_ai.key.as_ref(),
                    target_stake_authority.as_ref(),
                ];
                let (_, bump) = Pubkey::find_program_address(seeds, program_id);

                let signers_seeds: &[&[&[u8]]] = &[&[
                    b"stake",
                    vote_account_ai.key.as_ref(),
                    target_stake_authority.as_ref(),
                    &[bump],
                ]];

                invoke_signed(
                    &delegate_stake_instruction,
                    &[
                        stake_ai.clone(),
                        vote_account_ai.clone(),
                        clock_ai.clone(),
                        stake_history_ai.clone(),
                        stake_config_ai.clone(),
                        stake_authority_ai.clone(),
                    ],
                    signers_seeds,
                )?;

                msg!("Authorize the target stake authority");

                // Return the stake authority to the previous stake authority
                let authorize_instruction = authorize(
                    stake_ai.key,
                    stake_authority_ai.key,
                    &target_stake_authority,
                    StakeAuthorize::Staker,
                    None,
                );

                invoke_signed(
                    &authorize_instruction,
                    &[
                        stake_ai.clone(),
                        clock_ai.clone(),
                        stake_authority_ai.clone(),
                    ],
                    signers_seeds,
                )?;
                return Ok(());
            }
        }
    };

    Err(ProgramError::InvalidInstructionData)
}
