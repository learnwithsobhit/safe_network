// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::store::TransferStore;
use crate::Result;
use bls::{PublicKeySet, SecretKeyShare};
use log::info;
use safe_nd::{
    AccountId, DebitAgreementProof, Error as NdError, Money, PublicKey as NdPublicKey, PublicKey,
    ReplicaEvent, Result as NdResult, SignedTransfer, TransferPropagated, TransferRegistered,
    TransferValidated,
};
use safe_transfers::{get_genesis, TransferReplica as Replica};
use std::collections::BTreeSet;

use routing::SectionProofChain;
#[cfg(feature = "simulated-payouts")]
use {
    crate::node::node_ops::MessagingDuty,
    bls::{SecretKey, SecretKeySet},
    rand::thread_rng,
    safe_nd::{Signature, SignatureShare, Transfer},
};

/// Manages an instance of an AT2 Replica,
/// which is responsible for a number of AT2 Actors,
/// both those of clients but also the distributed
/// Actor run by this section.
pub struct ReplicaManager {
    replica: Replica,
    store: TransferStore,
    info: ReplicaInfo,
}

struct ReplicaInfo {
    initiating: bool,
    secret_key: SecretKeyShare,
    key_index: usize,
    peer_replicas: PublicKeySet,
    section_proof_chain: SectionProofChain,
}

impl ReplicaManager {
    pub(crate) fn new(
        store: TransferStore,
        secret_key: &SecretKeyShare,
        key_index: usize,
        peer_replicas: &PublicKeySet,
        section_proof_chain: SectionProofChain,
    ) -> Result<Self> {
        Ok(Self {
            store,
            replica: Replica::from_history(
                secret_key.clone(),
                key_index,
                peer_replicas.clone(),
                vec![],
            )?,
            info: ReplicaInfo {
                initiating: true,
                secret_key: secret_key.clone(),
                key_index,
                peer_replicas: peer_replicas.clone(),
                section_proof_chain,
            },
        })
    }

    pub(crate) fn all_keys(&self) -> Option<Vec<AccountId>> {
        self.store.all_stream_keys()
    }

    pub(crate) fn all_events(&self) -> Option<Vec<ReplicaEvent>> {
        self.store.try_load().ok()
    }

    pub(crate) fn history(&self, id: &AccountId) -> Option<Vec<ReplicaEvent>> {
        self.store.history(id)
    }

    pub(crate) fn balance(&self, id: &AccountId) -> Option<Money> {
        self.replica.balance(id)
    }

    /// When section splits, the Replicas in either resulting section
    /// also split the responsibility of the accounts.
    /// Thus, both Replica groups need to drop the accounts that
    /// the other group is now responsible for.
    pub(crate) fn drop_accounts(&mut self, accounts: &BTreeSet<AccountId>) -> NdResult<()> {
        self.check_init_status()?;

        // Drops the streams from db.
        self.store
            .drop(accounts)
            .map_err(|e| NdError::NetworkOther(e.to_string()))?;

        // Replays the kept streams
        // on a new instance of a Replica.
        self.update_replica_keys(
            self.info.secret_key.clone(),
            self.info.key_index,
            self.info.peer_replicas.clone(),
            self.info.section_proof_chain.clone(),
        )
    }

    /// Needs to be called before the replica manager
    /// can run properly. Any events from existing Replicas
    /// are supposed to be passed in. Without them, this Replica will
    /// not be able to function properly together with the others.
    pub(crate) fn initiate(&mut self, events: &[ReplicaEvent]) -> NdResult<()> {
        if !self.info.initiating {
            // can only synch while initiating
            return Err(NdError::InvalidOperation);
        }
        if events.is_empty() {
            // This means we are the first node in the network.
            let balance = u32::MAX as u64 * 1_000_000_000;
            let debit_proof = get_genesis(
                balance,
                PublicKey::Bls(self.info.peer_replicas.public_key()),
            )?;
            match self.replica.genesis(&debit_proof, || None) {
                Ok(Some(event)) => {
                    let event = ReplicaEvent::TransferPropagated(event);
                    self.persist(event)?;
                }
                Ok(None) | Err(_) => return Err(NdError::InvalidOperation), // todo: storage error
            };
        } else {
            let existing_events = self
                .store
                .try_load()
                .map_err(|e| NdError::NetworkOther(e.to_string()))?;
            let events: Vec<_> = events
                .iter()
                .cloned()
                .filter(|e| !existing_events.contains(e))
                .collect();
            // no more should be necessary for merging
            // these sets of events, but remains to be seen.
            // only order required is within specific streams,
            // and that order should have been presereved.
            // (otherwise we can simply call sort on the vec.)
            self.store
                .init(events.clone())
                .map_err(|e| NdError::NetworkOther(e.to_string()))?;
            self.replica = Replica::from_history(
                self.info.secret_key.clone(),
                self.info.key_index,
                self.info.peer_replicas.clone(),
                events,
            )?;
            // make sure to indicate that we are no longer initiating
            self.info.initiating = false;
        }
        Ok(())
    }

    pub(crate) fn update_replica_keys(
        &mut self,
        secret_key: SecretKeyShare,
        key_index: usize,
        peer_replicas: PublicKeySet,
        section_proof_chain: SectionProofChain,
    ) -> NdResult<()> {
        match self.store.try_load() {
            Ok(events) => {
                let events = if self.info.initiating { vec![] } else { events };
                self.replica = Replica::from_history(
                    secret_key.clone(),
                    key_index,
                    peer_replicas.clone(),
                    events,
                )?;
                self.info = ReplicaInfo {
                    initiating: self.info.initiating,
                    secret_key,
                    key_index,
                    peer_replicas,
                    section_proof_chain,
                };
                info!("Successfully updated Replica details on churn");
                Ok(())
            }
            Err(_e) => Err(NdError::InvalidOperation), // todo: storage error
        }
    }

    pub(crate) fn validate(
        &mut self,
        transfer: SignedTransfer,
    ) -> NdResult<Option<TransferValidated>> {
        self.check_init_status()?;

        let result = self.replica.validate(transfer);
        if let Ok(Some(event)) = result {
            match self.persist(ReplicaEvent::TransferValidated(event.clone())) {
                Ok(()) => Ok(Some(event)),
                Err(err) => Err(err),
            }
        } else {
            result
        }
    }

    pub(crate) fn register(
        &mut self,
        proof: &DebitAgreementProof,
    ) -> NdResult<Option<TransferRegistered>> {
        self.check_init_status()?;

        let serialized = bincode::serialize(&proof.signed_transfer)
            .map_err(|e| NdError::NetworkOther(e.to_string()))?;
        let sig = proof
            .debiting_replicas_sig
            .clone()
            .into_bls()
            .ok_or_else(|| {
                NdError::NetworkOther("Error retrieving threshold::Signature from DAP ".to_string())
            })?;
        let section_keys = self.info.section_proof_chain.clone();

        let result = self.replica.clone().register(proof, move || {
            let key = section_keys
                .keys()
                .find(|&key_in_chain| key_in_chain == &proof.replica_key.public_key());
            if let Some(key_in_chain) = key {
                key_in_chain.verify(&sig, serialized)
            } else {
                // PublicKey provided by the transfer was never a part of the Section retrospectively.
                false
            }
        });

        if let Ok(Some(event)) = result {
            match self.persist(ReplicaEvent::TransferRegistered(event.clone())) {
                Ok(()) => Ok(Some(event)),
                Err(err) => Err(err),
            }
        } else {
            result
        }
    }

    pub(crate) fn receive_propagated(
        &mut self,
        proof: &DebitAgreementProof,
    ) -> NdResult<Option<TransferPropagated>> {
        self.check_init_status()?;

        let serialized = bincode::serialize(&proof.signed_transfer)
            .map_err(|e| NdError::NetworkOther(e.to_string()))?;
        let section_keys = self.info.section_proof_chain.clone();
        let sig = proof
            .debiting_replicas_sig
            .clone()
            .into_bls()
            .ok_or_else(|| {
                NdError::NetworkOther("Error retrieving threshold::Signature from DAP ".to_string())
            })?;

        let result = self.replica.receive_propagated(proof, move || {
            let key = section_keys
                .keys()
                .find(|&key_in_chain| key_in_chain == &proof.replica_key.public_key());
            if let Some(key_in_chain) = key {
                if key_in_chain.verify(&sig, serialized) {
                    Some(NdPublicKey::from(*key_in_chain))
                } else {
                    None
                }
            } else {
                // PublicKey provided by the transfer was never a part of the Section retrospectively.
                None
            }
        });

        if let Ok(Some(event)) = result {
            match self.persist(ReplicaEvent::TransferPropagated(event.clone())) {
                Ok(()) => Ok(Some(event)),
                Err(err) => Err(err),
            }
        } else {
            result
        }
    }

    fn persist(&mut self, event: ReplicaEvent) -> NdResult<()> {
        self.store
            .try_append(event.clone())
            .map_err(|e| NdError::NetworkOther(e.to_string()))?;
        self.replica.apply(event)
    }

    /// Get the replica's PK set
    pub fn replicas_pk_set(&self) -> Option<PublicKeySet> {
        self.replica.replicas_pk_set()
    }

    /// While a Replica is initiating, i.e.
    /// retrieving events from the other Replicas,
    /// it will return an error on incoming cmds.
    fn check_init_status(&mut self) -> NdResult<()> {
        if self.info.initiating {
            return Err(NdError::InvalidOperation);
        }
        Ok(())
    }
}

#[cfg(feature = "simulated-payouts")]
impl ReplicaManager {
    pub fn credit_without_proof(&mut self, transfer: Transfer) -> Option<MessagingDuty> {
        self.replica.credit_without_proof(transfer.clone());
        let dummy_msg = "DUMMY MSG";
        let mut rng = thread_rng();
        let sec_key_set = SecretKeySet::random(7, &mut rng);
        let replica_key = sec_key_set.public_keys();
        let sec_key = SecretKey::random();
        let pub_key = sec_key.public_key();
        let dummy_shares = SecretKeyShare::default();
        let dummy_sig = dummy_shares.sign(dummy_msg);
        let sig = sec_key.sign(dummy_msg);
        let debit_proof = DebitAgreementProof {
            signed_transfer: SignedTransfer {
                transfer,
                actor_signature: Signature::from(sig.clone()),
            },
            debiting_replicas_sig: Signature::from(sig),
            replica_key,
        };
        self.store
            .try_append(ReplicaEvent::TransferPropagated(TransferPropagated {
                debit_proof,
                debiting_replicas: PublicKey::from(pub_key),
                crediting_replica_sig: SignatureShare {
                    index: 0,
                    share: dummy_sig,
                },
            }))
            .ok()?;
        None
    }

    pub fn debit_without_proof(&mut self, transfer: Transfer) {
        self.replica.debit_without_proof(transfer)
    }
}
