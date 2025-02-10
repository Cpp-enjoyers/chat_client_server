use crate::server::ChatServerInternal;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{
    ChatMessage, ConfirmRegistration, ErrorMessage, JoinChannel, MessageData, SendMessage,
};
use log::{debug, info, trace};
use rand::{rng, RngCore};
use std::collections::HashSet;
use wg_2024::network::NodeId;

impl ChatServerInternal {
    pub(crate) fn msg_clijoin(
        &mut self,
        replies: &mut Vec<(NodeId, ChatMessage)>,
        data: &JoinChannel,
        cli_node_id: NodeId,
    ) {
        info!(target: format!("Server {}", self.own_id).as_str(), "Received join request: {data:?}");
        let channelinfo;
        let channel_id;
        if let (Some(id), Some(data)) = (
            data.channel_id,
            data.channel_id
                .and_then(|id| self.channel_info.get_mut(&id)),
        ) {
            debug!(target: format!("Server {}", self.own_id).as_str(), "Joining channel by ID {id}");
            channelinfo = data;
            channel_id = id;
        } else if let (Some(id), Some(cdata)) = (
            self.channels.get_by_right(&data.channel_name),
            self.channels
                .get_by_right(&data.channel_name)
                .and_then(|id| self.channel_info.get_mut(id)),
        ) {
            channelinfo = cdata;
            channel_id = *id;
            debug!(target: format!("Server {}", self.own_id).as_str(), "Joining channel by name {}({id})",data.channel_name);
        } else if !data.channel_name.is_empty() {
            let mut id = rng().next_u64() & 0xFFFF_FFFF_FFFF_FFF0 | 0x2;
            while self.channels.contains_left(&id) || self.channel_info.contains_key(&id) {
                id = rng().next_u64() & 0xFFFF_FFFF_FFFF_FFF0 | 0x2;
            }
            debug!(target: format!("Server {}", self.own_id).as_str(), "Creating new channel with ID {id} and name {}", data.channel_name);
            self.channels.insert(id, data.channel_name.clone());
            self.channel_info.insert(id, (true, HashSet::new()));
            // This is safe, since we just inserted the channel
            channelinfo = self.channel_info.get_mut(&id).unwrap();
            channel_id = id;
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::SrvChannelCreationSuccessful(channel_id)),
                },
            ));
        } else {
            debug!(target: format!("Server {}", self.own_id).as_str(), "Invalid channel join request from client {cli_node_id}");
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::Err(ErrorMessage {
                        error_type: "CHANNEL_NOT_EXISTS".to_string(),
                        error_message: "Channel with that ID doesn't exist".to_string(),
                    })),
                },
            ));
            return;
        }
        if channelinfo.1.contains(&cli_node_id) {
            debug!(target: format!("Server {}", self.own_id).as_str(), "Client {cli_node_id} is already in channel {channel_id}");
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::Err(ErrorMessage {
                        error_type: "CHANNEL_ALREADY_JOINED".to_string(),
                        error_message: "Channel was already joined!".to_string(),
                    })),
                },
            ));
        } else {
            {
                channelinfo.1.insert(cli_node_id);
            }
            for val in self.channel_info.iter_mut().filter(|(id, _x)| {
                **id != 0x1 && **id != u64::from(cli_node_id) << 32 | 0x8 && **id != channel_id
            }) {
                trace!(target: format!("Server {}", self.own_id).as_str(), "Removing client {cli_node_id} from channel {}", val.0);
                val.1 .1.remove(&cli_node_id);
            }
            trace!(target: format!("Server {}", self.own_id).as_str(), "Client {cli_node_id} is joining channel {channel_id}");
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::SrvChannelCreationSuccessful(channel_id)),
                },
            ));
            replies.extend_from_slice(self.generate_channel_updates().as_slice());
        }
    }

    pub(crate) fn msg_sendmsg(
        &mut self,
        replies: &mut Vec<(NodeId, ChatMessage)>,
        cli_node_id: NodeId,
        msg: &SendMessage,
    ) {
        info!(target: format!("Server {}", self.own_id).as_str(), "Received message: {msg:?}");
        match (
            self.channel_info.get(&msg.channel_id),
            self.usernames.get_by_left(&cli_node_id),
        ) {
            (Some(channel_data), Some(username)) => {
                debug!(target: format!("Server {}", self.own_id).as_str(), "Forwarding message sent by {username}");
                for id in channel_data.1.iter().filter(|x| **x != cli_node_id) {
                    trace!(target: format!("Server {}", self.own_id).as_str(), "Forwarding message to client {id}");
                    replies.push((
                        *id,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::SrvDistributeMessage(MessageData {
                                username: username.clone(),
                                timestamp: chrono::Utc::now().timestamp_millis().unsigned_abs(),
                                message: msg.message.clone(),
                                channel_id: msg.channel_id,
                            })),
                        },
                    ));
                }
            }
            (_, None) => {
                debug!(target: format!("Server {}", self.own_id).as_str(), "Client {cli_node_id} is not registered");
                replies.push((
                    cli_node_id,
                    ChatMessage {
                        own_id: self.own_id.into(),
                        message_kind: Some(MessageKind::Err(ErrorMessage {
                            error_type: "NOT_REGISTERED".to_string(),
                            error_message: "Can't send message, you're not registered".to_string(),
                        })),
                    },
                ));
            }
            (None, Some(_)) => {
                debug!(target: format!("Server {}", self.own_id).as_str(), "Channel doesn't exist");
                replies.push((
                    cli_node_id,
                    ChatMessage {
                        own_id: self.own_id.into(),
                        message_kind: Some(MessageKind::Err(ErrorMessage {
                            error_type: "CHANNEL_NOT_EXISTS".to_string(),
                            error_message: "Can't send message, channel doesn't exist".to_string(),
                        })),
                    },
                ));
            }
        }
    }

    pub(crate) fn msg_cliregisterrequest(
        &mut self,
        replies: &mut Vec<(NodeId, ChatMessage)>,
        cli_node_id: NodeId,
        req: String,
    ) {
        info!(target: format!("Server {}", self.own_id).as_str(), "Received register request: {req:?}");
        if self.usernames.contains_left(&cli_node_id) {
            debug!(target: format!("Server {}", self.own_id).as_str(), "Client {cli_node_id} already registered");
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::SrvConfirmReg(ConfirmRegistration {
                        successful: false,
                        error: Some("Client already registered".to_string()),
                        username: req,
                    })),
                },
            ));
        } else if self.usernames.contains_right(&req) {
            debug!(target: format!("Server {}", self.own_id).as_str(), "Username {req} already exists");
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::SrvConfirmReg(ConfirmRegistration {
                        successful: false,
                        error: Some("Username already exists".to_string()),
                        username: req,
                    })),
                },
            ));
        } else {
            debug!(target: format!("Server {}", self.own_id).as_str(), "Registering client {cli_node_id} with username {req}");
            replies.push((
                cli_node_id,
                ChatMessage {
                    own_id: self.own_id.into(),
                    message_kind: Some(MessageKind::SrvConfirmReg(ConfirmRegistration {
                        successful: true,
                        error: None,
                        username: req.clone(),
                    })),
                },
            ));
            self.usernames.insert(cli_node_id, req.clone());
            self.channel_info
                .get_mut(&0x1)
                .map(|x| x.1.insert(cli_node_id));
            self.channels
                .insert(u64::from(cli_node_id) << 32 | 0x8, req);
            self.channel_info.insert(
                u64::from(cli_node_id) << 32 | 0x8,
                (false, map_macro::hash_set! {cli_node_id}),
            );
            replies.extend_from_slice(self.generate_channel_updates().as_slice());
        }
    }

    pub(crate) fn msg_clicancelreq(
        &mut self,
        replies: &mut Vec<(NodeId, ChatMessage)>,
        cli_node_id: NodeId,
    ) {
        info!(target: format!("Server {}", self.own_id).as_str(), "Received cancel registration request");
        for val in self.channel_info.values_mut() {
            val.1.retain(|&x| x != cli_node_id);
        }
        self.channels
            .remove_by_left(&(u64::from(cli_node_id) << 32 | 0x8));
        self.usernames.remove_by_left(&cli_node_id);
        replies.extend_from_slice(self.generate_channel_updates().as_slice());
    }

    pub(crate) fn msg_clileave(
        &mut self,
        replies: &mut Vec<(NodeId, ChatMessage)>,
        cli_node_id: NodeId,
    ) {
        info!(target: format!("Server {}", self.own_id).as_str(), "Received leave request from client {cli_node_id}");
        for val in self
            .channel_info
            .iter_mut()
            .filter(|(id, _x)| **id != 0x1 && **id != u64::from(cli_node_id) << 32 | 0x8)
        {
            trace!(target: format!("Server {}", self.own_id).as_str(), "Removing client {cli_node_id} from channel {}", val.0);
            val.1 .1.remove(&cli_node_id);
        }
        replies.extend_from_slice(self.generate_channel_updates().as_slice());
    }
}
