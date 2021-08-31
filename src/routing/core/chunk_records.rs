// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::Core;
use crate::messaging::{
    data::{
        ChunkDataExchange, ChunkRead, ChunkWrite, CmdError, Error as ErrorMessage, StorageLevel,
    },
    system::{NodeCmd, NodeQuery, SystemMsg},
    AuthorityProof, EndUser, MessageId, ServiceAuth,
};

use super::Command;
use super::{capacity::CHUNK_COPY_COUNT, Prefix, Result};
use crate::routing::error::convert_to_error_message;
use crate::routing::section::SectionUtils;
use crate::types::{Chunk, ChunkAddress, PublicKey};
use std::collections::BTreeSet;
use tracing::info;
use xor_name::XorName;

use crate::routing::Error;

impl Core {
    pub(crate) fn get_copy_count(&self) -> usize {
        CHUNK_COPY_COUNT
    }

    pub(crate) async fn get_data_of(&self, prefix: &Prefix) -> ChunkDataExchange {
        // Prepare full_adult details
        let adult_levels = self.capacity.levels_matching(*prefix).await;
        ChunkDataExchange { adult_levels }
    }

    pub(crate) async fn update_chunks(&self, chunk_data: ChunkDataExchange) {
        let ChunkDataExchange { adult_levels } = chunk_data;
        self.capacity.set_adult_levels(adult_levels).await
    }

    /// Registered holders not present in provided list of members
    /// will be removed from adult_storage_info and no longer tracked for liveness.
    pub(crate) async fn retain_members_only(&self, members: BTreeSet<XorName>) -> Result<()> {
        // full adults
        self.capacity.retain_members_only(&members).await;

        // stop tracking liveness of absent holders
        self.liveness.retain_members_only(members);

        Ok(())
    }

    pub(super) async fn write_chunk_to_adults(
        &self,
        write: ChunkWrite,
        msg_id: MessageId,
        auth: AuthorityProof<ServiceAuth>,
        origin: EndUser,
    ) -> Result<Vec<Command>> {
        trace!("Init of sending. ChunkWrite to adults {:?}", write);
        use ChunkWrite::*;
        match write {
            New(data) => self.store(data, msg_id, auth, origin).await,
            DeletePrivate(address) => self.delete_chunk(address, auth, origin, msg_id).await,
        }
    }

    /// Set storage level of a given node.
    /// Returns whether the level changed or not.
    pub(crate) async fn set_storage_level(&self, node_id: &PublicKey, level: StorageLevel) -> bool {
        info!("Setting new storage level..");
        let changed = self
            .capacity
            .set_adult_level(XorName::from(*node_id), level)
            .await;
        let avg_usage = self.capacity.avg_usage().await;
        info!(
            "Avg storage usage among Adults is between {}-{} %",
            avg_usage * 10,
            (avg_usage + 1) * 10
        );
        changed
    }

    pub(crate) async fn full_adults(&self) -> BTreeSet<XorName> {
        self.capacity.full_adults().await
    }

    async fn store(
        &self,
        chunk: Chunk,
        msg_id: MessageId,
        auth: AuthorityProof<ServiceAuth>,
        origin: EndUser,
    ) -> Result<Vec<Command>> {
        if let Err(error) = validate_chunk_owner(&chunk, &auth.public_key) {
            return self.send_error(error, msg_id, origin).await;
        }

        let target = *chunk.name();

        let msg = SystemMsg::NodeCmd(NodeCmd::Chunks {
            cmd: ChunkWrite::New(chunk),
            auth: auth.into_inner(),
            origin,
        });

        let targets = self.get_chunk_holder_adults(&target).await;

        let aggregation = false;

        if self.get_copy_count() > targets.len() {
            let error = CmdError::Data(ErrorMessage::InsufficientAdults(*self.section().prefix()));
            return self.send_cmd_error_response(error, origin, msg_id);
        }

        self.send_node_msg_to_targets(msg, targets, aggregation)
    }

    pub(crate) async fn send_error(
        &self,
        error: Error,
        msg_id: MessageId,
        origin: EndUser,
    ) -> Result<Vec<Command>> {
        let error = convert_to_error_message(error);
        let error = CmdError::Data(error);

        self.send_cmd_error_response(error, origin, msg_id)
    }

    async fn delete_chunk(
        &self,
        address: ChunkAddress,
        auth: AuthorityProof<ServiceAuth>,
        origin: EndUser,
        _msg_id: MessageId,
    ) -> Result<Vec<Command>> {
        trace!("Handling delete at elders, forwarding to adults");
        let targets = self.get_chunk_holder_adults(address.name()).await;

        let msg = SystemMsg::NodeCmd(NodeCmd::Chunks {
            cmd: ChunkWrite::DeletePrivate(address),
            auth: auth.into_inner(),
            origin,
        });

        let aggregation = false;

        self.send_node_msg_to_targets(msg, targets, aggregation)
    }

    pub(super) async fn read_chunk_from_adults(
        &self,
        read: &ChunkRead,
        msg_id: MessageId,
        auth: AuthorityProof<ServiceAuth>,
        origin: EndUser,
    ) -> Result<Vec<Command>> {
        trace!("setting up ChunkRead for adults, {:?}", read.dst_address());

        let ChunkRead::Get(address) = read;
        let targets = self.get_chunk_holder_adults(address.name()).await;

        if targets.is_empty() {
            return self
                .send_error(Error::NoAdults(*self.section().prefix()), msg_id, origin)
                .await;
        }

        let mut fresh_targets = BTreeSet::new();
        for target in targets {
            let _ = self
                .liveness
                .add_a_pending_request_operation(target, read.operation_id()?);
            let _ = fresh_targets.insert(target);
        }

        let msg = SystemMsg::NodeQuery(NodeQuery::Chunks {
            query: ChunkRead::Get(*address),
            auth: auth.into_inner(),
            origin,
        });

        let aggregation = false;

        self.send_node_msg_to_targets(msg, fresh_targets, aggregation)
    }
}

fn validate_chunk_owner(chunk: &Chunk, requester: &PublicKey) -> Result<()> {
    if chunk.is_private() {
        chunk
            .owner()
            .ok_or_else(|| Error::InvalidOwner(*requester))
            .and_then(|chunk_owner| {
                if chunk_owner != requester {
                    Err(Error::InvalidOwner(*requester))
                } else {
                    Ok(())
                }
            })
    } else {
        Ok(())
    }
}
