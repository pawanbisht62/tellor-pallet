// Copyright 2023 Tellor Inc.
// This file is part of Tellor.

// Tellor is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Tellor is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Tellor. If not, see <http://www.gnu.org/licenses/>.

//! Benchmarking setup for tellor

use super::*;
use codec::Compact;
use ethabi::{Token, Uint};

#[allow(unused)]
use crate::Pallet as Tellor;
use crate::{constants::DECIMALS, traits::BenchmarkHelper, types::QueryDataOf};
use codec::alloc::vec;
use frame_benchmarking::{account, benchmarks, BenchmarkError};
use frame_support::traits::OnInitialize;
use frame_system::RawOrigin;
use sp_core::bounded::BoundedVec;
use sp_runtime::traits::{Hash, Keccak256};
use types::{Address, Timestamp};

type RuntimeOrigin<T> = <T as frame_system::Config>::RuntimeOrigin;
const TRB: u128 = 10u128.pow(DECIMALS);
const SEED: u32 = 0;
const MAX_SUBMISSIONS: u32 = 5_000;

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

fn trb(amount: impl Into<f64>) -> Tributes {
	// TRB amount has 18 decimals
	Tributes::from((amount.into() * TRB as f64) as u128)
}

fn token<T: Config>(amount: impl Into<u64>) -> BalanceOf<T> {
	// consumer parachain token
	(amount.into() * unit::<T>() as u64).into()
}

fn unit<T: Config>() -> u128 {
	let decimals: u8 = T::Decimals::get();
	10u128.pow(decimals.into())
}

fn uint_value<T: Config>(value: impl Into<Uint>) -> ValueOf<T> {
	ethabi::encode(&[Token::Uint(value.into())]).try_into().unwrap()
}

fn deposit_stake<T: Config>(
	reporter: AccountIdOf<T>,
	amount: Tributes,
	address: Address,
) -> Result<RuntimeOrigin<T>, BenchmarkError> {
	match T::StakingOrigin::try_successful_origin() {
		Ok(origin) => {
			Tellor::<T>::report_stake_deposited(origin.clone(), reporter, amount, address)
				.map_err(|_| BenchmarkError::Weightless)?;
			Ok(origin)
		},
		Err(_) => Err(BenchmarkError::Weightless),
	}
}

// Helper function for creating feeds
fn create_feed<T: Config>(
	feed_creator: AccountIdOf<T>,
	query_id: QueryId,
	reward: BalanceOf<T>,
	start_time: Timestamp,
	interval: Timestamp,
	window: Timestamp,
	price_threshold: u16,
	reward_increase_per_second: BalanceOf<T>,
	query_data: QueryDataOf<T>,
	amount: BalanceOf<T>,
) -> FeedId {
	Tellor::<T>::setup_data_feed(
		RawOrigin::Signed(feed_creator).into(),
		query_id,
		reward,
		start_time,
		interval,
		window,
		price_threshold,
		reward_increase_per_second,
		query_data,
		amount,
	)
	.unwrap();
	let feed_id = Keccak256::hash(&ethabi::encode(&vec![
		Token::FixedBytes(query_id.0.into()),
		Token::Uint(reward.into()),
		Token::Uint(start_time.into()),
		Token::Uint(interval.into()),
		Token::Uint(window.into()),
		Token::Uint(price_threshold.into()),
		Token::Uint(reward_increase_per_second.into()),
	]))
	.into();
	feed_id
}

fn dispute_id(para_id: u32, query_id: QueryId, timestamp: Timestamp) -> DisputeId {
	Keccak256::hash(&ethabi::encode(&[
		Token::Uint(para_id.into()),
		Token::FixedBytes(query_id.0.to_vec()),
		Token::Uint(timestamp.into()),
	]))
	.into()
}

benchmarks! {
	register {
	}: _(RawOrigin::Root)

	claim_onetime_tip {
		// Maximum submissions in order to measure maximum weight as this extrinsic iterates over all the report submissions
		let s in 1..MAX_SUBMISSIONS;
		// Maximum timestamps for claiming tip for measuring maximum weight
		let t in 1..T::MaxClaimTimestamps::get();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let another_reporter = account::<AccountIdOf<T>>("account", 2, SEED);
		T::BenchmarkHelper::set_balance(reporter.clone(), 1_00_000);
		T::BenchmarkHelper::set_balance(another_reporter.clone(), 10_000);
		T::BenchmarkHelper::set_time(MINUTES);

		let address = Address::zero();
		// report deposit stake
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		deposit_stake::<T>(another_reporter.clone(), trb(1_200), address)?;
		for i in 1..=s {
			Tellor::<T>::tip(RawOrigin::Signed(reporter.clone()).into(), query_id, token::<T>(1u64), query_data.clone()).unwrap();
			T::BenchmarkHelper::set_time(HOURS);
			Tellor::<T>::submit_value(
				RawOrigin::Signed(reporter.clone()).into(),
				query_id,
				uint_value::<T>(i * 1_000),
				0,
				query_data.clone()
			)?;

			if i > T::MaxClaimTimestamps::get() {
				let timestamp = <ReportedTimestampsByIndex<T>>::get(query_id, i-1).unwrap();
				Tellor::<T>::begin_dispute(RawOrigin::Signed(
					another_reporter.clone()).into(),
					query_id,
					timestamp,
					None)?;
			}
		}
		let mut timestamps: BoundedVec<Compact<Timestamp>, T::MaxClaimTimestamps> = Default::default();
		let mut reported_timestamps: Vec<Timestamp> = <Reports<T>>::iter_key_prefix(query_id).collect();
		reported_timestamps.sort();
		for timestamp in reported_timestamps.iter().take(t as usize) {
			timestamps.try_push(timestamp.into()).unwrap();
		}
		T::BenchmarkHelper::set_time(12 * HOURS);
	}: _(RawOrigin::Signed(reporter), query_id, timestamps)

	claim_tip {
		// Maximum submissions in order to measure maximum weight as this extrinsic iterates over all the report submissions
		let s in 1..MAX_SUBMISSIONS;
		// Maximum timestamps for claiming tip for measuring maximum weight
		let t in 1..T::MaxClaimTimestamps::get();

		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let feed_creator = account::<AccountIdOf<T>>("account", 2, SEED);
		let address = Address::zero();

		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
		T::BenchmarkHelper::set_time(MINUTES);
		T::BenchmarkHelper::set_balance(feed_creator.clone(), 1_000);
		let feed_id = create_feed::<T>(feed_creator.clone(),
				query_id,
				token::<T>(10u64),
				T::Time::now().as_secs(),
				3600,
				600,
				1,
				token::<T>(0u64),
				query_data.clone(),
				token::<T>(1000u64)
		);

		// report deposit stake
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;

		for i in 1..=s {
			Tellor::<T>::tip(RawOrigin::Signed(reporter.clone()).into(), query_id, token::<T>(10u64), query_data.clone()).unwrap();
			T::BenchmarkHelper::set_time(1 * HOURS);
			Tellor::<T>::submit_value(
				RawOrigin::Signed(reporter.clone()).into(),
				query_id,
				uint_value::<T>(i * 1_000),
				0,
				query_data.clone())?;
		}

		let mut timestamps: BoundedVec<Compact<Timestamp>, T::MaxClaimTimestamps> = Default::default();
		let mut reported_timestamps: Vec<Timestamp> = <Reports<T>>::iter_key_prefix(query_id).collect();
		reported_timestamps.sort();
		reported_timestamps.reverse();

		for timestamp in reported_timestamps.iter().take(T::MaxClaimTimestamps::get() as usize)  {
			timestamps.try_push(timestamp.into()).unwrap();
		}

		T::BenchmarkHelper::set_time(12 * HOURS);
	}: _(RawOrigin::Signed(reporter), feed_id, query_id, timestamps)

	fund_feed{
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let feed_creator = account::<AccountIdOf<T>>("account", 1, SEED);

		T::BenchmarkHelper::set_balance(feed_creator.clone(), 1_000);
		let feed_id = create_feed::<T>(feed_creator.clone(),
				query_id,
				token::<T>(10u64),
				T::Time::now().as_secs(),
				700,
				60,
				0,
				token::<T>(0u64),
				query_data,
				token::<T>(1000u64)
		);

	}: _(RawOrigin::Signed(feed_creator), feed_id, query_id, token::<T>(10u64))
	verify {
		assert!(<DataFeeds<T>>::get(query_id, feed_id).is_some());
	}

	setup_data_feed {
		// Maximum value for query data in order to measure the maximum weight
		let q in 1..T::MaxQueryDataLength::get();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![1u8; q as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let feed_creator = account::<AccountIdOf<T>>("account", 1, SEED);

		T::BenchmarkHelper::set_balance(feed_creator.clone(), 1000);
		// create feed
		create_feed::<T>(
			feed_creator.clone(),
			query_id,
			token::<T>(10u64),
			T::Time::now().as_secs(),
			700,
			60,
			0,
			token::<T>(0u64),
			query_data.clone(),
			token::<T>(1_000u64)
		);

	}: _(RawOrigin::Signed(feed_creator), query_id, token::<T>(10u64), T::Time::now().as_secs(), 600, 60, 0, token::<T>(0u64), query_data, token::<T>(1_000u64))

	tip {
		// Maximum submissions in order to measure maximum weight as this extrinsic iterates over all the report submissions
		let s in 1..MAX_SUBMISSIONS;
		// Maximum value for query data in order to measure the maximum weight
		let q in 1..T::MaxQueryDataLength::get();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![1u8; q as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let another_reporter = account::<AccountIdOf<T>>("account", 2, SEED);
		let address = Address::zero();
		T::BenchmarkHelper::set_time(MINUTES);
		// report deposit stake
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		deposit_stake::<T>(another_reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_balance(reporter.clone(), 10_000);
		T::BenchmarkHelper::set_balance(another_reporter.clone(), 10_000);
		// submitting multiple reports
		for i in 1..=s {
			T::BenchmarkHelper::set_time(HOURS);
			Tellor::<T>::submit_value(
				RawOrigin::Signed(reporter.clone()).into(), query_id, uint_value::<T>(i * 1_000), 0, query_data.clone()
			)?;
			if i > 1 {
				let timestamp = <ReportedTimestampsByIndex<T>>::get(query_id, i-1).unwrap();
				Tellor::<T>::begin_dispute(RawOrigin::Signed(
					another_reporter.clone()).into(),
					query_id,
					timestamp,
					None)?;
			}
		}
		let amount = token::<T>(100u64);
	}: _(RawOrigin::Signed(reporter), query_id, amount, query_data)

	add_staking_rewards {
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
	}: _(RawOrigin::Signed(reporter), token::<T>(100u64))

	submit_value {
		// Maximum value for query data in order to measure the maximum weight
		let q in 1..T::MaxQueryDataLength::get();
		// Maximum length for value in order to measure the maximum weight
		let v in 1..T::MaxValueLength::get();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![1u8; q as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let value  = BoundedVec::try_from(vec![1u8; v as usize]).unwrap();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let address = Address::zero();
		// report deposit stake
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_time(HOURS);
	}: _(RawOrigin::Signed(reporter.clone()), query_id, value, 0, query_data)
	verify {
		assert!(<StakerDetails<T>>::get(reporter).is_some());
	}

	update_stake_amount {
		let staking_token_price_query_data: QueryDataOf<T> = T::BenchmarkHelper::get_staking_token_price_query_data();
		let staking_token_price_query_id = Keccak256::hash(staking_token_price_query_data.as_ref()).into();
		let staking_to_local_token_query_data: QueryDataOf<T> = T::BenchmarkHelper::get_staking_to_local_token_price_query_data();
		let staking_to_local_token_query_id: QueryId =
			Keccak256::hash(staking_to_local_token_query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let address = Address::zero();
		// report deposit stake
		deposit_stake::<T>(reporter.clone(), trb(10_000), address)?;
		T::BenchmarkHelper::set_time(HOURS);
		// submit value
		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			staking_token_price_query_id,
			uint_value::<T>(50 * 10u128.pow(18)),
			0,
			staking_token_price_query_data)?;
		T::BenchmarkHelper::set_time(12 * HOURS);

		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			staking_to_local_token_query_id,
			uint_value::<T>(6 * 10u128.pow(18)),
			0,
			staking_to_local_token_query_data)?;

		T::BenchmarkHelper::set_time(12 * HOURS);
	}: _(RawOrigin::Signed(reporter))

	begin_dispute {
		// Maximum number of sequential disputed timestamps
		let d in 1..T::MaxDisputedTimeSeries::get();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let stake_amount = <StakeAmount<T>>::get();
		let dispute_fees = 10;

		T::BenchmarkHelper::set_time(REPORTING_LOCK);

		// Create initial timestamp for later dispute in extrinsic call
		let reporter = account::<AccountIdOf<T>>("account", 0, SEED);
		deposit_stake::<T>(reporter.clone(), stake_amount, Address::zero())?;
		T::BenchmarkHelper::set_balance(reporter.clone(), dispute_fees);
		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(), query_id, uint_value::<T>(10), 0, query_data.clone()
		)?;

		// Create series of disputed timestamps, using new accounts to avoid reporting lock
		for i in 1..d {
			T::BenchmarkHelper::set_time(1);
			let reporter = account::<AccountIdOf<T>>("account", i, SEED);
			deposit_stake::<T>(reporter.clone(), stake_amount, Address::zero())?;
			T::BenchmarkHelper::set_balance(reporter.clone(), dispute_fees);
			Tellor::<T>::submit_value(
				RawOrigin::Signed(reporter.clone()).into(), query_id, uint_value::<T>(i * 10), i, query_data.clone()
			)?;
			Tellor::<T>::begin_dispute(RawOrigin::Signed(reporter).into(),
				query_id,
				<ReportedTimestampsByIndex<T>>::get(query_id, i).unwrap(),
				None)?;
		}

		let timestamp = <ReportedTimestampsByIndex<T>>::get(query_id, 0).unwrap();
	}: _(RawOrigin::Signed(reporter), query_id, timestamp, None)
	verify {
		let governance_contract = T::Governance::get();
		assert!(<LastReportedTimestamp<T>>::get(query_id).is_none());
		assert_last_event::<T>(
				Event::NewDisputeSent { para_id: governance_contract.para_id, contract_address: governance_contract.address.into()}.into(),
			);
	}

	vote {
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let address = Address::zero();
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_time(HOURS);
		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			query_id,
			uint_value::<T>(4_000),
			0,
			query_data)?;

		let disputed_timestamp = <TimeOfLastNewValue<T>>::get().unwrap();

		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
		Tellor::<T>::begin_dispute(RawOrigin::Signed(reporter.clone()).into(), query_id, disputed_timestamp, None)?;

		let dispute_id = dispute_id(T::ParachainId::get(), query_id, disputed_timestamp);
	}: _(RawOrigin::Signed(reporter.clone()), dispute_id, Some(true))
	verify {
		assert_last_event::<T>(
				Event::Voted { dispute_id, supports: Some(true), voter: reporter}.into(),
			);
	}

	vote_on_multiple_disputes {
		// Maximum votes for disputes are used in order to measure the maximum weight
		let v in 1..T::MaxVotes::get();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let another_reporter = account::<AccountIdOf<T>>("account", 2, SEED);
		let address = Address::zero();
		let mut votes: BoundedVec<(DisputeId, Option<bool>), T::MaxVotes> = BoundedVec::default();
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		deposit_stake::<T>(another_reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
		T::BenchmarkHelper::set_balance(another_reporter.clone(), 1_000);
		for i in 1..=v {
			T::BenchmarkHelper::set_time(HOURS);
			Tellor::<T>::submit_value(RawOrigin::Signed(reporter.clone()).into(), query_id, uint_value::<T>(i * 1_000), 0, query_data.clone())?;

			let timestamp = <TimeOfLastNewValue<T>>::get().unwrap();
			let dispute_id = dispute_id(T::ParachainId::get(), query_id, timestamp);
			if i % 2 == 0 {
				Tellor::<T>::begin_dispute(RawOrigin::Signed(reporter.clone()).into(), query_id, timestamp, None)?;
				votes.try_push((dispute_id, Some(true))).unwrap();
			} else {
				Tellor::<T>::begin_dispute(RawOrigin::Signed(another_reporter.clone()).into(), query_id, timestamp, None)?;
				votes.try_push((dispute_id, Some(false))).unwrap();
			}
		}
	}: _(RawOrigin::Signed(reporter), votes)

	send_votes {
		// The maximum number of votes be sent
		let v in 1..u8::MAX.into();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 256, SEED);
		let user = account::<AccountIdOf<T>>("account", 257, SEED);
		let address = Address::zero();
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
		for i in 1..=v {
			let another_reporter = account::<AccountIdOf<T>>("account", i, SEED);
			deposit_stake::<T>(another_reporter.clone(), trb(1_200), address)?;
			T::BenchmarkHelper::set_balance(another_reporter.clone(), 100);
			T::BenchmarkHelper::set_time(HOURS);
			Tellor::<T>::submit_value(
				RawOrigin::Signed(another_reporter.clone()).into(),
				query_id,
				uint_value::<T>(i * 1_000),
				0,
				query_data.clone())?;

			let timestamp = <TimeOfLastNewValue<T>>::get().unwrap();
			let dispute_id = dispute_id(T::ParachainId::get(), query_id, timestamp);
			if i % 2 == 0 {
				Tellor::<T>::begin_dispute(RawOrigin::Signed(reporter.clone()).into(), query_id, timestamp, None)?;
				Tellor::<T>::vote(RawOrigin::Signed(reporter.clone()).into(), dispute_id, Some(true))?;
				Tellor::<T>::vote(RawOrigin::Signed(another_reporter.clone()).into(), dispute_id, Some(false))?;
			} else {
				Tellor::<T>::begin_dispute(RawOrigin::Signed(another_reporter.clone()).into(), query_id, timestamp, None)?;
				Tellor::<T>::vote(RawOrigin::Signed(reporter.clone()).into(), dispute_id, Some(false))?;
				Tellor::<T>::vote(RawOrigin::Signed(another_reporter.clone()).into(), dispute_id, Some(true))?;
				Tellor::<T>::vote(RawOrigin::Signed(user.clone()).into(), dispute_id, None)?;
			}
		}

		T::BenchmarkHelper::set_time(HOURS);
	}: _(RawOrigin::Signed(reporter), v as u8)

	report_stake_deposited {
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let address = Address::zero();
		let amount = trb(100);
		let caller = deposit_stake::<T>(reporter.clone(), amount, address)?;
	}: _<RuntimeOrigin<T>>(caller, reporter.clone(), amount, address)
	verify {
		assert_last_event::<T>(
				Event::NewStakerReported { staker: reporter, amount, address }.into(),
			);
	}

	report_staking_withdraw_request {
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let address = Address::zero();
		let amount = trb(100);
		let caller = deposit_stake::<T>(reporter.clone(), amount, address)?;
	}: _<RuntimeOrigin<T>>(caller, reporter, amount, address)
	verify {
		let staking_contract = T::Staking::get();
		assert_last_event::<T>(
				Event::StakeWithdrawRequestConfirmationSent { para_id: staking_contract.para_id,
				contract_address: staking_contract.address.into() }.into(),
			);
	}

	report_stake_withdrawn {
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let address = Address::zero();
		let amount = trb(100);
		let caller = deposit_stake::<T>(reporter.clone(), amount, address)?;
		// request stake withdraw
		Tellor::<T>::report_staking_withdraw_request(caller.clone(), reporter.clone(), amount, address)?;
		T::BenchmarkHelper::set_time(WEEKS);
	}: _<RuntimeOrigin<T>>(caller, reporter.clone(), amount)
	verify {
		assert_last_event::<T>(
				Event::StakeWithdrawnReported { staker: reporter }.into(),
			);
	}

	report_slash {
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let caller = T::GovernanceOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
		let address = Address::zero();
		deposit_stake::<T>(reporter.clone(), trb(1_200), address).unwrap();
		T::BenchmarkHelper::set_time(HOURS);

		// submit value
		Tellor::<T>::submit_value(RawOrigin::Signed(reporter.clone()).into(), query_id, uint_value::<T>(4_000), 0, query_data.clone())?;
		let disputed_timestamp = <TimeOfLastNewValue<T>>::get().unwrap();
		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
		// begin dispute
		Tellor::<T>::begin_dispute(RawOrigin::Signed(reporter.clone()).into(), query_id, disputed_timestamp, None)?;
		let dispute_id = dispute_id(T::ParachainId::get(), query_id, disputed_timestamp);

		// vote in dispute
		Tellor::<T>::vote(RawOrigin::Signed(reporter.clone()).into(), dispute_id, Some(true))?;

		T::BenchmarkHelper::set_time(DAYS);
		// tally vote
		Tellor::<T>::report_vote_tallied(caller.clone(), dispute_id, VoteResult::Passed)?;

	}: _<RuntimeOrigin<T>>(caller, reporter.clone(), trb(100))
	verify {
		assert_last_event::<T>(
				Event::SlashReported { reporter, amount: trb(100)}.into(),
			);
	}

	report_vote_tallied {
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let caller = T::GovernanceOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
		let address = Address::zero();
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_time(HOURS);
		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			query_id,
			uint_value::<T>(4_000),
			0,
			query_data)?;
		let disputed_timestamp = <TimeOfLastNewValue<T>>::get().unwrap();
		T::BenchmarkHelper::set_balance(reporter.clone(), 1_000);
		Tellor::<T>::begin_dispute(RawOrigin::Signed(reporter.clone()).into(), query_id, disputed_timestamp, None)?;
		let dispute_id = dispute_id(T::ParachainId::get(), query_id, disputed_timestamp);
		Tellor::<T>::vote(RawOrigin::Signed(reporter).into(), dispute_id, Some(true))?;
		T::BenchmarkHelper::set_time(DAYS);
	}: _<RuntimeOrigin<T>>(caller, dispute_id, VoteResult::Passed)

	report_vote_executed {
		// Maximum number of vote rounds are used as this extrinsic iterates over all vote rounds
		let r in 1..u8::MAX.into();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 256, SEED);
		let caller = T::GovernanceOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
		let address = Address::zero();
		deposit_stake::<T>(reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_time(HOURS);
		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			query_id,
			uint_value::<T>(4_000),
			0,
			query_data)?;

		let mut dispute_initiators = Vec::default();

		let disputed_timestamp = <TimeOfLastNewValue<T>>::get().unwrap();
		for i in 1..=r {
			let another_reporter = account::<AccountIdOf<T>>("account", i, SEED);
			deposit_stake::<T>(another_reporter.clone(), trb(1_200), address)?;
			T::BenchmarkHelper::set_balance(another_reporter.clone(), 1_000);
			dispute_initiators.push(another_reporter);
		}
		for dispute_initiator in dispute_initiators{
			Tellor::<T>::begin_dispute(RawOrigin::Signed(dispute_initiator.clone()).into(), query_id, disputed_timestamp, None)?;
		}
		let dispute_id = dispute_id(T::ParachainId::get(), query_id, disputed_timestamp);
		Tellor::<T>::vote(RawOrigin::Signed(reporter).into(), dispute_id, Some(true))?;
		T::BenchmarkHelper::set_time(WEEKS);
		Tellor::<T>::report_vote_tallied(caller.clone(), dispute_id, VoteResult::Passed)?;
		T::BenchmarkHelper::set_time(DAYS);
	}: _<RuntimeOrigin<T>>(caller, dispute_id)

	on_initialize {
		let staking_token_price_query_data: QueryDataOf<T> = T::BenchmarkHelper::get_staking_token_price_query_data();
		let staking_token_price_query_id = Keccak256::hash(staking_token_price_query_data.as_ref()).into();
		let staking_to_local_token_query_data: QueryDataOf<T> = T::BenchmarkHelper::get_staking_to_local_token_price_query_data();
		let staking_to_local_token_query_id: QueryId =
			Keccak256::hash(staking_to_local_token_query_data.as_ref()).into();
		let query_data: QueryDataOf<T> = BoundedVec::try_from(vec![0u8; T::MaxQueryDataLength::get() as usize]).unwrap();
		let query_id = Keccak256::hash(query_data.as_ref()).into();
		let reporter = account::<AccountIdOf<T>>("account", 1, SEED);
		let another_reporter = account::<AccountIdOf<T>>("account", 2, SEED);
		let user = account::<AccountIdOf<T>>("account", 3, SEED);
		let address = Address::zero();
		// report deposit stake
		deposit_stake::<T>(reporter.clone(), trb(10_000), address)?;
		deposit_stake::<T>(another_reporter.clone(), trb(1_200), address)?;
		T::BenchmarkHelper::set_balance(another_reporter.clone(), 1_000);
		T::BenchmarkHelper::set_time(HOURS);
		// submit value
		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			staking_token_price_query_id,
			uint_value::<T>(50 * 10u128.pow(18)),
			0,
			staking_token_price_query_data.clone())?;
		T::BenchmarkHelper::set_time(HOURS);

		Tellor::<T>::submit_value(
			RawOrigin::Signed(reporter.clone()).into(),
			staking_to_local_token_query_id,
			uint_value::<T>(6 * 10u128.pow(18)),
			0,
			staking_to_local_token_query_data)?;

		T::BenchmarkHelper::set_time(12 * HOURS);

		for i in 1..4 {
			Tellor::<T>::submit_value(RawOrigin::Signed(reporter.clone()).into(),
				query_id,
				uint_value::<T>(i * 1_000),
				0,
				query_data.clone())?;
			let timestamp = <TimeOfLastNewValue<T>>::get().unwrap();
			let dispute_id = dispute_id(T::ParachainId::get(), query_id, timestamp);
			Tellor::<T>::begin_dispute(RawOrigin::Signed(another_reporter.clone()).into(), query_id, timestamp, None)?;
			Tellor::<T>::vote(RawOrigin::Signed(user.clone()).into(), dispute_id, Some(true))?;
			T::BenchmarkHelper::set_time(HOURS);
		}
	}: {
		Tellor::<T>::on_initialize(T::BlockNumber::zero())
	}

	impl_benchmark_test_suite!(Tellor, crate::mock::new_test_ext(), crate::mock::Test);
}
