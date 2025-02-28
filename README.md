# Stake me

https://x.com/trentdotsol/status/1894897559283355859

Allow permissionless delegation to the chosen vote account while returning the authorized staker

Once the epoch ends, the stake account can permissionlessly be delegated to the vote account and the authorized staker will be returned to the specified authorized staker.

The program is stateless and tracking eligible accounts is up to indexing, the pda could be flagged with a memo instruction when setting the authorized staker to the PDA

```rust

    let (pda, _) = Pubkey::find_program_address(
        &[
            b"stake",
            // Delegate to vote account
            vote_account.as_ref(),
            // Authorized staker to return to
            authorized_staker.as_ref(),
        ],
        &program_id,
    );
```
