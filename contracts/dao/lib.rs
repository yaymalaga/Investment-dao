#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
pub mod dao {
    use ink::{
        env::{
            call::{
                build_call,
                ExecutionInput,
                Selector,
            },
            DefaultEnvironment,
        },
        storage::Mapping,
    };
    use scale::{
        Decode,
        Encode,
    };

    const ONE_MINUTE: u64 = 60;

    #[derive(Encode, Decode, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum VoteType {
        Against,
        For,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Encode, Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum GovernorError {
        AmountShouldNotBeZero,
        DurationError,
        ProposalNotFound,
        ProposalAlreadyExecuted,
        VotePeriodEnded,
        AlreadyVoted,
        QuorumNotReached,
        ProposalNotAccepted,
        InsufficientBalance,
    }

    #[derive(Encode, Decode)]
    #[cfg_attr(
        feature = "std",
        derive(
            Debug,
            PartialEq,
            Eq,
            scale_info::TypeInfo,
            ink::storage::traits::StorageLayout
        )
    )]
    pub struct Proposal {
        to: AccountId,
        vote_start: u64,
        vote_end: u64,
        executed: bool,
        amount: Balance,
    }

    #[derive(Encode, Decode, Default)]
    #[cfg_attr(
        feature = "std",
        derive(
            Debug,
            PartialEq,
            Eq,
            scale_info::TypeInfo,
            ink::storage::traits::StorageLayout
        )
    )]
    pub struct ProposalVote {
        for_votes: u8,
        against_vote: u8,
    }

    pub type ProposalId = u32;

    #[ink(storage)]
    pub struct Governor {
        proposals: Mapping<ProposalId, Proposal>,
        proposal_votes: Mapping<ProposalId, ProposalVote>,
        votes: Mapping<(ProposalId, AccountId), ()>,
        next_proposal_id: ProposalId,
        quorum: u8,
        governance_token: AccountId,
    }

    impl Governor {
        #[ink(constructor, payable)]
        pub fn new(governance_token: AccountId, quorum: u8) -> Self {
            Self {
                proposals: Mapping::default(),
                proposal_votes: Mapping::default(),
                votes: Mapping::default(),
                next_proposal_id: 0,
                quorum,
                governance_token,
            }
        }

        #[ink(message)]
        pub fn propose(
            &mut self,
            to: AccountId,
            amount: Balance,
            duration: u64,
        ) -> Result<(), GovernorError> {
            if amount == 0 {
                return Err(GovernorError::AmountShouldNotBeZero)
            } else if amount >= self.env().balance() {
                return Err(GovernorError::InsufficientBalance)
            }

            if duration == 0 {
                return Err(GovernorError::DurationError)
            }

            let current_time = self.now();

            let proposal = Proposal {
                amount,
                to,
                vote_start: current_time,
                vote_end: current_time + duration * ONE_MINUTE,
                executed: false,
            };

            self.proposals.insert(self.next_proposal_id, &proposal);

            self.next_proposal_id += 1;

            Ok(())
        }

        #[ink(message)]
        pub fn vote(
            &mut self,
            proposal_id: ProposalId,
            vote: VoteType,
        ) -> Result<(), GovernorError> {
            let proposal = self.get_proposal(proposal_id)?;

            if proposal.executed {
                return Err(GovernorError::ProposalAlreadyExecuted)
            }

            if self.now() > proposal.vote_end {
                return Err(GovernorError::VotePeriodEnded)
            }

            if self.votes.contains((proposal_id, self.env().caller())) {
                return Err(GovernorError::AlreadyVoted)
            }

            self.votes.insert((proposal_id, self.env().caller()), &());

            // Check the weight of the caller of the governance token (the proportion of
            // caller balance in relation to total supply)
            let total_supply = self.total_supply();
            let caller_balance = self.balance_of(self.env().caller());
            let weight: u8 = (caller_balance * 100 / total_supply) as u8;

            let mut proposal_votes =
                self.proposal_votes.get(proposal_id).unwrap_or_default();
            if vote == VoteType::For {
                proposal_votes.for_votes += weight;
            } else {
                proposal_votes.against_vote += weight;
            }
            self.proposal_votes.insert(proposal_id, &proposal_votes);

            Ok(())
        }

        #[ink(message)]
        pub fn execute(&mut self, proposal_id: ProposalId) -> Result<(), GovernorError> {
            let mut proposal = self.get_proposal(proposal_id)?;

            if proposal.executed {
                return Err(GovernorError::ProposalAlreadyExecuted)
            }

            let proposal_votes = self.proposal_votes.get(proposal_id).unwrap_or_default();
            if proposal_votes.for_votes + proposal_votes.against_vote < self.quorum {
                return Err(GovernorError::QuorumNotReached)
            }

            if proposal_votes.for_votes < proposal_votes.against_vote {
                return Err(GovernorError::ProposalNotAccepted)
            }

            if self.env().balance() > proposal.amount {
                self.env().transfer(proposal.to, proposal.amount).unwrap();
            } else {
                return Err(GovernorError::InsufficientBalance)
            }

            proposal.executed = true;
            self.proposals.insert(proposal_id, &proposal);

            Ok(())
        }

        // used for test
        #[ink(message)]
        pub fn now(&self) -> u64 {
            self.env().block_timestamp()
        }

        pub fn get_proposal(
            &self,
            proposal_id: ProposalId,
        ) -> Result<Proposal, GovernorError> {
            self.proposals
                .get(proposal_id)
                .ok_or(GovernorError::ProposalNotFound)
        }

        pub fn next_proposal_id(&self) -> ProposalId {
            return self.next_proposal_id
        }

        #[ink(message)]
        pub fn total_supply(&self) -> Balance {
            let call_result = build_call::<DefaultEnvironment>()
                .call(self.governance_token)
                .gas_limit(5000000000)
                .exec_input(ExecutionInput::new(Selector::new(ink::selector_bytes!(
                    "PSP22::total_supply"
                ))))
                .returns::<Balance>()
                .try_invoke();

            return call_result.unwrap().unwrap()
        }

        #[ink(message)]
        pub fn balance_of(&self, account: AccountId) -> u128 {
            let call_result = build_call::<DefaultEnvironment>()
                .call(self.governance_token)
                .gas_limit(5000000000)
                .exec_input(
                    ExecutionInput::new(Selector::new(ink::selector_bytes!(
                        "PSP22::balance_of"
                    )))
                    .push_arg(account),
                )
                .returns::<Balance>()
                .try_invoke();

            return call_result.unwrap().unwrap()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn create_contract(initial_balance: Balance, quorum: u8) -> Governor {
            let accounts = default_accounts();
            set_sender(accounts.alice);
            set_balance(contract_id(), initial_balance);
            Governor::new(AccountId::from([0x01; 32]), quorum)
        }

        fn contract_id() -> AccountId {
            ink::env::test::callee::<ink::env::DefaultEnvironment>()
        }

        fn default_accounts(
        ) -> ink::env::test::DefaultAccounts<ink::env::DefaultEnvironment> {
            ink::env::test::default_accounts::<ink::env::DefaultEnvironment>()
        }

        fn set_sender(sender: AccountId) {
            ink::env::test::set_caller::<ink::env::DefaultEnvironment>(sender);
        }

        fn set_balance(account_id: AccountId, balance: Balance) {
            ink::env::test::set_account_balance::<ink::env::DefaultEnvironment>(
                account_id, balance,
            )
        }

        #[ink::test]
        fn propose_works() {
            let accounts = default_accounts();
            let mut governor = create_contract(1000, 50);
            assert_eq!(governor.next_proposal_id(), 0);

            assert_eq!(
                governor.propose(accounts.django, 0, 1),
                Err(GovernorError::AmountShouldNotBeZero)
            );
            assert_eq!(
                governor.propose(accounts.django, 100, 0),
                Err(GovernorError::DurationError)
            );

            let result = governor.propose(accounts.django, 100, 1);
            assert_eq!(result, Ok(()));
            let proposal = governor.get_proposal(0).unwrap();
            let now = governor.now();
            assert_eq!(
                proposal,
                Proposal {
                    to: accounts.django,
                    amount: 100,
                    vote_start: 0,
                    vote_end: now + 1 * ONE_MINUTE,
                    executed: false,
                }
            );
            assert_eq!(governor.next_proposal_id(), 1);

            let result: Result<(), GovernorError> = governor.propose(accounts.django, 200, 2);
            assert_eq!(result, Ok(()));
            let proposal = governor.get_proposal(1).unwrap();
            let now = governor.now();
            assert_eq!(
                proposal,
                Proposal {
                    to: accounts.django,
                    amount: 200,
                    vote_start: 0,
                    vote_end: now + 2 * ONE_MINUTE,
                    executed: false,
                }
            );
            assert_eq!(governor.next_proposal_id(), 2);

            assert_eq!(
                governor.propose(accounts.django, 2000, 0),
                Err(GovernorError::InsufficientBalance)
            );
        }

        #[ink::test]
        fn quorum_not_reached() {
            let mut governor = create_contract(1000, 50);
            let result = governor.propose(AccountId::from([0x02; 32]), 100, 1);
            assert_eq!(result, Ok(()));
            let execute = governor.execute(0);
            assert_eq!(execute, Err(GovernorError::QuorumNotReached));
        }

        #[ink::test]
        fn quorum_reached() {
            let mut governor = create_contract(1000, 0);
            let result = governor.propose(AccountId::from([0x02; 32]), 100, 1);
            assert_eq!(result, Ok(()));
            let execute = governor.execute(0);
            assert_eq!(execute, Ok(()));
        }

        #[ink::test]
        fn execute_random() {
            let mut governor = create_contract(1000, 0);
            let execute = governor.execute(16);
            assert_eq!(execute, Err(GovernorError::ProposalNotFound));
        }
    }
}
