use itertools::Itertools;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use curp::members::ClusterInfo;
use curp::ConfChangeType::{Add, AddLearner, Promote, Remove, Update};
use curp::{client::Client, cmd::generate_propose_id, ConfChange, ProposeConfChangeRequest};
use xlineapi::{
    Cluster, Member, MemberAddRequest, MemberAddResponse, MemberListRequest, MemberListResponse,
    MemberPromoteRequest, MemberPromoteResponse, MemberRemoveRequest, MemberRemoveResponse,
    MemberUpdateRequest, MemberUpdateResponse,
};

use super::command::{propose_err_to_status, Command};
use crate::header_gen::HeaderGenerator;

/// Cluster Server
pub(crate) struct ClusterServer {
    /// Consensus client
    client: Arc<Client<Command>>,
    /// Server name
    name: String,
    /// Header generator
    header_gen: Arc<HeaderGenerator>,
}

/// Get current timestamp in seconds
fn timestamp() -> u64 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| unreachable!("Time went backwards"))
        .as_secs()
}

impl ClusterServer {
    /// New `ClusterServer`
    pub(crate) fn new(
        client: Arc<Client<Command>>,
        header_gen: Arc<HeaderGenerator>,
        name: String,
    ) -> Self {
        Self {
            client,
            name,
            header_gen,
        }
    }

    /// Send propose conf change request
    async fn propose_conf_change(&self, change: ConfChange) -> Result<Vec<Member>, Status> {
        Ok(self
            .client
            .propose_conf_change(ProposeConfChangeRequest {
                id: generate_propose_id(&self.name),
                changes: vec![change],
            })
            .await
            .map_err(propose_err_to_status)??
            .into_iter()
            .map(|member| Member {
                id: member.id(),
                name: member.name().to_owned(),
                peer_ur_ls: member.addrs().to_vec(),
                client_ur_ls: member.addrs().to_vec(),
                is_learner: member.is_learner(),
            })
            .collect())
    }
}

#[tonic::async_trait]
impl Cluster for ClusterServer {
    async fn member_add(
        &self,
        request: Request<MemberAddRequest>,
    ) -> Result<Response<MemberAddResponse>, Status> {
        let req = request.into_inner();
        let change_type = if req.is_learner {
            i32::from(AddLearner)
        } else {
            i32::from(Add)
        };
        let peer_url_ls = req.peer_ur_ls.into_iter().sorted().collect_vec();
        // calculate node id based on addresses and current timestamp
        let node_id = ClusterInfo::calculate_member_id(peer_url_ls.clone(), "", Some(timestamp()));
        let members = self
            .propose_conf_change(ConfChange {
                change_type,
                node_id,
                address: peer_url_ls,
            })
            .await?;
        let resp = MemberAddResponse {
            header: Some(self.header_gen.gen_header()),
            member: members.iter().find(|m| m.id == node_id).cloned(),
            members,
        };
        Ok(Response::new(resp))
    }

    async fn member_remove(
        &self,
        request: Request<MemberRemoveRequest>,
    ) -> Result<Response<MemberRemoveResponse>, Status> {
        let req = request.into_inner();
        let members = self
            .propose_conf_change(ConfChange {
                change_type: i32::from(Remove),
                node_id: req.id,
                address: vec![],
            })
            .await?;
        let resp = MemberRemoveResponse {
            header: Some(self.header_gen.gen_header()),
            members,
        };
        Ok(Response::new(resp))
    }

    async fn member_update(
        &self,
        request: Request<MemberUpdateRequest>,
    ) -> Result<Response<MemberUpdateResponse>, Status> {
        let req = request.into_inner();
        let members = self
            .propose_conf_change(ConfChange {
                change_type: i32::from(Update),
                node_id: req.id,
                address: req.peer_ur_ls,
            })
            .await?;
        let resp = MemberUpdateResponse {
            header: Some(self.header_gen.gen_header()),
            members,
        };
        Ok(Response::new(resp))
    }

    async fn member_list(
        &self,
        request: Request<MemberListRequest>,
    ) -> Result<Response<MemberListResponse>, Status> {
        let req = request.into_inner();
        let members = self
            .client
            .get_cluster_from_curp(req.linearizable)
            .await
            .map_err(propose_err_to_status)?
            .members;
        let resp = MemberListResponse {
            header: Some(self.header_gen.gen_header()),
            members: members
                .into_iter()
                .map(|member| Member {
                    id: member.id,
                    name: member.name,
                    peer_ur_ls: member.addrs.clone(),
                    client_ur_ls: member.addrs,
                    is_learner: member.is_learner,
                })
                .collect(),
        };
        Ok(Response::new(resp))
    }

    async fn member_promote(
        &self,
        request: Request<MemberPromoteRequest>,
    ) -> Result<Response<MemberPromoteResponse>, Status> {
        let req = request.into_inner();
        let members = self
            .propose_conf_change(ConfChange {
                change_type: i32::from(Promote),
                node_id: req.id,
                address: vec![],
            })
            .await?;
        let resp = MemberPromoteResponse {
            header: Some(self.header_gen.gen_header()),
            members,
        };
        Ok(Response::new(resp))
    }
}