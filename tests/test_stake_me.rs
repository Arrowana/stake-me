use {
    borsh::to_vec,
    restake_me::StakeMeInstruction,
    solana_program::{
        pubkey::Pubkey,
        stake::state::{Authorized, Lockup, StakeStateV2},
        system_instruction,
    },
    solana_program_test::*,
    solana_sdk::{
        clock::Clock,
        instruction::{AccountMeta, Instruction},
        native_token::LAMPORTS_PER_SOL,
        signature::{Keypair, Signer},
        stake::{self, instruction as stake_instruction, state::StakeAuthorize},
        stake_history::StakeHistory,
        sysvar,
        transaction::Transaction,
        vote::{
            self,
            instruction::CreateVoteAccountConfig,
            state::{VoteInit, VoteStateVersions},
        },
    },
};

async fn setup_vote_account(context: &mut ProgramTestContext, vote_account: &Keypair) {
    // Create vote_account_a
    let identity = Keypair::new();
    let vote_init = VoteInit {
        node_pubkey: identity.pubkey(),
        authorized_voter: identity.pubkey(),
        authorized_withdrawer: identity.pubkey(),
        commission: 0,
    };
    let create_vote_account_config = CreateVoteAccountConfig {
        space: VoteStateVersions::vote_state_size_of(true) as u64,
        ..CreateVoteAccountConfig::default()
    };

    let transaction = Transaction::new_signed_with_payer(
        &vote::instruction::create_account_with_config(
            &context.payer.pubkey(),
            &vote_account.pubkey(),
            &vote_init,
            LAMPORTS_PER_SOL,
            create_vote_account_config,
        ),
        Some(&context.payer.pubkey()),
        &[&context.payer, vote_account, &identity],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_restake_flow() {
    let program_id = Pubkey::new_unique();
    let mut program_test = ProgramTest::new("restake_me", program_id, None);

    // Create test accounts
    let user = Keypair::new();
    let stake_account = Keypair::new();
    let vote_account_a = Keypair::new();
    let vote_account_b = Keypair::new();

    // Add some SOL to user's account
    program_test.add_account(
        user.pubkey(),
        solana_sdk::account::Account {
            lamports: 10_000_000_000,
            owner: solana_sdk::system_program::id(),
            ..solana_sdk::account::Account::default()
        },
    );

    // Start the test context
    let mut context = program_test.start_with_context().await;

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
    println!("Clock on start: {:?}", clock);

    setup_vote_account(&mut context, &vote_account_a).await;
    // context.increment_vote_account_credits(&vote_account_a.pubkey(), 10);
    setup_vote_account(&mut context, &vote_account_b).await;

    // Create and initialize stake account
    let rent = context.banks_client.get_rent().await.unwrap();
    let stake_rent = rent.minimum_balance(std::mem::size_of::<StakeStateV2>());
    let stake_amount = 5_000_000_000;
    let transaction = Transaction::new_signed_with_payer(
        &[
            system_instruction::create_account(
                &user.pubkey(),
                &stake_account.pubkey(),
                stake_rent + stake_amount,
                std::mem::size_of::<StakeStateV2>() as u64,
                &solana_sdk::stake::program::id(),
            ),
            stake_instruction::initialize(
                &stake_account.pubkey(),
                &Authorized {
                    staker: user.pubkey(),
                    withdrawer: user.pubkey(),
                },
                &Lockup::default(),
            ),
        ],
        Some(&user.pubkey()),
        &[&user, &stake_account],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Delegate stake to vote_account_a
    let transaction = Transaction::new_signed_with_payer(
        &[stake_instruction::delegate_stake(
            &stake_account.pubkey(),
            &user.pubkey(),
            &vote_account_a.pubkey(),
        )],
        Some(&user.pubkey()),
        &[&user],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    let slots = context
        .genesis_config()
        .epoch_schedule
        .get_slots_in_epoch(clock.epoch);
    context.warp_to_slot(slots + 1).unwrap();

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
    println!("{:?}", clock);

    let stake_account_data = context
        .banks_client
        .get_account(stake_account.pubkey())
        .await
        .unwrap()
        .unwrap();
    // Verify we are staked
    if let StakeStateV2::Stake(_meta, stake, _stake_flags) =
        bincode::deserialize(&stake_account_data.data).unwrap()
    {
        assert_eq!(stake.delegation.voter_pubkey, vote_account_a.pubkey());
        let stake_history_entry = stake.delegation.stake_activating_and_deactivating(
            clock.epoch,
            &context
                .banks_client
                .get_sysvar::<StakeHistory>()
                .await
                .unwrap(),
            None,
        );
        println!("{stake_history_entry:?}");
        assert!(stake_history_entry.effective > 0);
    } else {
        panic!("Stake account not properly staked");
    }

    // Calculate PDA for restake program
    let (pda, _bump_seed) = Pubkey::find_program_address(
        &[
            b"stake",
            vote_account_b.pubkey().as_ref(),
            user.pubkey().as_ref(),
        ],
        &program_id,
    );

    // Deactivate and set authorized staker to PDA
    let transaction = Transaction::new_signed_with_payer(
        &[
            stake_instruction::deactivate_stake(&stake_account.pubkey(), &user.pubkey()),
            stake_instruction::authorize(
                &stake_account.pubkey(),
                &user.pubkey(),
                &pda,
                StakeAuthorize::Staker,
                None,
            ),
        ],
        Some(&user.pubkey()),
        &[&user],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Advance clock to next epoch
    context
        .warp_to_slot(context.genesis_config().epoch_schedule.slots_per_epoch)
        .unwrap();

    // Call restake instruction
    let transaction = Transaction::new_signed_with_payer(
        &[Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(stake_account.pubkey(), false),
                AccountMeta::new_readonly(vote_account_b.pubkey(), false),
                AccountMeta::new_readonly(sysvar::clock::ID, false),
                AccountMeta::new_readonly(sysvar::stake_history::ID, false),
                AccountMeta::new_readonly(stake::config::ID, false),
                AccountMeta::new_readonly(pda, false),
                AccountMeta::new_readonly(stake::program::ID, false),
            ],
            data: to_vec(&StakeMeInstruction::Stake {
                target_stake_authority: user.pubkey(),
            })
            .unwrap(),
        }], // TODO
        Some(&user.pubkey()),
        &[&user],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Verify stake account is now delegated again
    let stake_account_data = context
        .banks_client
        .get_account(stake_account.pubkey())
        .await
        .unwrap()
        .unwrap();

    if let StakeStateV2::Stake(meta, stake, _stake_flags) =
        bincode::deserialize(&stake_account_data.data).unwrap()
    {
        assert_eq!(stake.delegation.voter_pubkey, vote_account_b.pubkey());
        assert_eq!(meta.authorized.staker, user.pubkey()); // Staker is back to target authorized stakers
    } else {
        panic!("Stake account not properly staked");
    }
}
