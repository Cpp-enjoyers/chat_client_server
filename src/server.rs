use bimap::BiHashMap;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{
    Channel, ChannelsList, ChatMessage, ClientData, ConfirmRegistration, DiscoveryResponse,
    ErrorMessage, MessageData,
};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ServerCommand, ServerEvent};
use crossbeam::channel::Sender;
use rand::{rng, RngCore};
use std::collections::{HashMap, HashSet};
use log::{debug, error, info};
use map_macro::hash_map;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

pub struct ChatServerInternal {
    own_id: NodeId,
    channels: BiHashMap<u64, String>,
    channel_info: HashMap<u64, (bool, HashSet<NodeId>)>,
    usernames: BiHashMap<NodeId, String>,
}

impl CommandHandler<ServerCommand, ServerEvent> for ChatServerInternal {
    fn get_node_type() -> NodeType {
        NodeType::Server
    }

    fn handle_protocol_message(
        &mut self,
        message: ChatMessage,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ServerEvent>)
    where
        Self: Sized,
    {
        let mut replies: Vec<(NodeId, ChatMessage)> = vec![];
        let cli_node_id = message.own_id as NodeId;
        // TODO
        info!(target: format!("Server {}", self.own_id).as_str(), "Received message: {message:?}");
        if let Some(kind) = message.message_kind {
            match kind {
                MessageKind::CliRegisterRequest(req) => {
                    info!(target: format!("Server {}", self.own_id).as_str(), "Received register request: {req:?}");
                    if self.usernames.contains_left(&cli_node_id) {
                        info!(target: format!("Server {}", self.own_id).as_str(), "Client {cli_node_id} already registered");
                        replies.push((
                            cli_node_id,
                            ChatMessage {
                                own_id: self.own_id.into(),
                                message_kind: Some(MessageKind::SrvConfirmReg(
                                    ConfirmRegistration {
                                        successful: false,
                                        error: Some("Client already registered".to_string()),
                                        username: req,
                                    },
                                )),
                            },
                        ));
                    } else if self.usernames.contains_right(&req) {
                        info!(target: format!("Server {}", self.own_id).as_str(), "Username {req} already exists");
                        replies.push((
                            cli_node_id,
                            ChatMessage {
                                own_id: self.own_id.into(),
                                message_kind: Some(MessageKind::SrvConfirmReg(
                                    ConfirmRegistration {
                                        successful: false,
                                        error: Some("Username already exists".to_string()),
                                        username: req,
                                    },
                                )),
                            },
                        ));
                    } else {
                        info!(target: format!("Server {}", self.own_id).as_str(), "Registering client {cli_node_id} with username {req}");
                        replies.push((
                            cli_node_id,
                            ChatMessage {
                                own_id: self.own_id.into(),
                                message_kind: Some(MessageKind::SrvConfirmReg(
                                    ConfirmRegistration {
                                        successful: true,
                                        error: None,
                                        username: req.clone(),
                                    },
                                )),
                            },
                        ));
                        debug!(target: format!("Server {}", self.own_id).as_str(), "Sending channel updates");
                        self.channel_info
                            .get_mut(&0x1)
                            .map(|(_, clients)| clients.insert(cli_node_id));
                        self.channels
                            .insert(u64::from(cli_node_id) << 32 | 0x8, req);
                        self.channel_info.insert(
                            u64::from(cli_node_id) << 32 | 0x8,
                            (false, map_macro::hash_set! {cli_node_id}),
                        );
                    }
                }
                MessageKind::CliCancelReg(..) => {
                    for val in self.channel_info.values_mut() {
                        val.1.retain(|&x| x != cli_node_id);
                    }
                    self.channels
                        .remove_by_left(&(u64::from(cli_node_id) << 32 | 0x8));
                    self.usernames.remove_by_left(&cli_node_id);
                    replies.extend_from_slice(self.generate_channel_updates().as_slice());
                }
                MessageKind::CliRequestChannels(..) => {
                    replies.extend_from_slice(self.generate_channel_updates().as_slice());
                }
                MessageKind::CliJoin(data) => {
                    let channelinfo;
                    let channel_id;
                    if let (Some(id), Some(data)) = (
                        data.channel_id,
                        data.channel_id
                            .and_then(|id| self.channel_info.get_mut(&id)),
                    ) {
                        channelinfo = data;
                        channel_id = id;
                    } else if let (Some(id), Some(data)) = (
                        self.channels.get_by_right(&data.channel_name),
                        self.channels
                            .get_by_right(&data.channel_name)
                            .and_then(|id| self.channel_info.get_mut(id)),
                    ) {
                        channelinfo = data;
                        channel_id = *id;
                    } else if !data.channel_name.is_empty() {
                        let mut id = rng().next_u64() & 0xFFFF_FFFF_FFFF_FFF0 | 0x2;
                        while self.channels.contains_left(&id)
                            || self.channel_info.contains_key(&id)
                        {
                            id = rng().next_u64() & 0xFFFF_FFFF_FFFF_FFF0 | 0x2;
                        }
                        self.channels.insert(id, data.channel_name.clone());
                        self.channel_info.insert(id, (true, HashSet::new()));
                        // This is safe, since we just inserted the channel
                        channelinfo = self.channel_info.get_mut(&id).unwrap();
                        channel_id = id;
                    } else {
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
                        return (replies, vec![]);
                    }
                    if channelinfo.1.contains(&cli_node_id) {
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
                        channelinfo.1.insert(cli_node_id);
                        replies.push((
                            cli_node_id,
                            ChatMessage {
                                own_id: self.own_id.into(),
                                message_kind: Some(MessageKind::SrvChannelCreationSuccessful(
                                    channel_id,
                                )),
                            },
                        ));
                        replies.extend_from_slice(self.generate_channel_updates().as_slice());
                    }
                }
                MessageKind::CliLeave(..) => {
                    for val in self.channel_info.values_mut().filter(|x| x.0) {
                        val.1.remove(&cli_node_id);
                    }
                    replies.extend_from_slice(self.generate_channel_updates().as_slice());
                }
                MessageKind::SendMsg(msg) => {
                    match (
                        self.channel_info.get(&msg.channel_id),
                        self.usernames.get_by_left(&cli_node_id),
                    ) {
                        (Some(channel_data), Some(username)) => {
                            for id in &channel_data.1 {
                                replies.push((
                                    *id,
                                    ChatMessage {
                                        own_id: u32::from(self.own_id),
                                        message_kind: Some(MessageKind::SrvDistributeMessage(
                                            MessageData {
                                                username: username.clone(),
                                                timestamp: chrono::Utc::now().timestamp_millis()
                                                    as u64,
                                                message: msg.message.clone(),
                                                channel_id: msg.channel_id,
                                            },
                                        )),
                                    },
                                ));
                            }
                        }
                        (None, Some(_)) => {
                            replies.push((
                                cli_node_id,
                                ChatMessage {
                                    own_id: self.own_id.into(),
                                    message_kind: Some(MessageKind::Err(ErrorMessage {
                                        error_type: "NOT_REGISTERED".to_string(),
                                        error_message: "Can't send message, you're not registered"
                                            .to_string(),
                                    })),
                                },
                            ));
                        }
                        (_, None) => {
                            replies.push((
                                cli_node_id,
                                ChatMessage {
                                    own_id: self.own_id.into(),
                                    message_kind: Some(MessageKind::Err(ErrorMessage {
                                        error_type: "CHANNEL_NOT_EXISTS".to_string(),
                                        error_message: "Can't send message, channel doesn't exist"
                                            .to_string(),
                                    })),
                                },
                            ));
                        }
                    }
                }
                MessageKind::Err(e) => {
                    error!(target: format!("Server {}", self.own_id).as_str(), "Received error message: {e:?}");
                }
                MessageKind::DsvReq(..) => {
                    // TODO: Change DiscoveryResponse so it doesn't need server_id, since it's already in own_id
                    info!(target: format!("Server {}", self.own_id).as_str(), "Sending back discovery response");
                    replies.push((
                        message.own_id as NodeId,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::DsvRes(DiscoveryResponse {
                                server_id: u32::from(self.own_id),
                                server_type: "chat".to_string(),
                            })),
                        },
                    ));
                }
                _ => {
                    replies.push((
                        message.own_id as NodeId,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::Err(ErrorMessage {
                                error_type: "INVALID_CLI_MESSAGE".to_string(),
                                error_message: format!("Invalid message: {kind:?}"),
                            })),
                        },
                    ));
                }
            }
        }
        (replies, vec![])
    }

    fn report_sent_packet(&mut self, packet: Packet) -> ServerEvent
    where
        Self: Sized,
    {
        ServerEvent::PacketSent(packet)
    }

    fn handle_controller_command(
        &mut self,
        sender_hash: &mut HashMap<NodeId, Sender<Packet>>,
        command: ServerCommand,
    ) -> (Option<Packet>, Vec<(NodeId, ChatMessage)>, Vec<ServerEvent>)
    where
        Self: Sized,
    {
        info!(target: format!("Server {}", self.own_id).as_str(), "Received controller command: {command:?}");
        match command {
            ServerCommand::AddSender(id, sender) => {
                sender_hash.insert(id, sender);
                (None, vec![], vec![])
            }
            ServerCommand::RemoveSender(id) => {
                sender_hash.remove(&id);
                (None, vec![], vec![])
            }
            ServerCommand::Shortcut(p) => (Some(p), vec![], vec![]),
        }
    }

    fn add_node(&mut self, _id: NodeId, _typ: NodeType) -> Option<(NodeId, ChatMessage)> {
        None
    }

    fn new(id: NodeId) -> Self
    where
        Self: Sized,
    {
        let mut channels = BiHashMap::default();
        channels.insert(0x1, "All".to_string());
        let channel_info = hash_map! {0x1 => (true, HashSet::new())};
        ChatServerInternal { own_id: id, channels, channel_info, usernames: BiHashMap::default() }
    }
}

pub type ChatServer = PacketHandler<ServerCommand, ServerEvent, ChatServerInternal>;

impl ChatServerInternal {
    fn generate_channel_updates(&mut self) -> Vec<(NodeId, ChatMessage)> {
        let mut updates = vec![];
        let mut channel_list = vec![];
        for (id, name) in &self.channels {
            if let Some((is_group, clients)) = self.channel_info.get(id) {
                let mut clients_res = vec![];
                for x in clients {
                    if let Some(name) = self.usernames.get_by_left(x) {
                        clients_res.push(ClientData {
                            username: name.clone(),
                            id: u64::from(*x),
                        });
                    }
                }
                channel_list.push(Channel {
                    channel_name: name.clone(),
                    channel_id: *id,
                    channel_is_group: *is_group,
                    connected_clients: clients_res,
                });
            }
        }
        for id in self.usernames.left_values() {
            updates.push((
                *id,
                ChatMessage {
                    own_id: u32::from(self.own_id),
                    message_kind: Some(MessageKind::SrvReturnChannels(ChannelsList {
                        channels: channel_list.clone(),
                    })),
                },
            ));
        }
        updates
    }
}
