mod server_message_handling;

use bimap::BiHashMap;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{
    Channel, ChannelsList, ChatMessage, ClientData, DiscoveryResponse,
    ErrorMessage,
};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ServerCommand, ServerEvent};
use crossbeam::channel::Sender;
use log::{debug, error, info, trace};
use map_macro::hash_map;
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

#[derive(Debug)]
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
        trace!(target: format!("Server {}", self.own_id).as_str(), "Current state: {self:?}");
        info!(target: format!("Server {}", self.own_id).as_str(), "Received message: {message:?}");
        if let Some(kind) = message.message_kind {
            match kind {
                MessageKind::CliRegisterRequest(req) => {
                    self.msg_cliregisterrequest(&mut replies, &cli_node_id, req)
                }
                MessageKind::CliCancelReg(..) => self.msg_clicancelreq(&mut replies, &cli_node_id),
                MessageKind::CliRequestChannels(..) => {
                    info!(target: format!("Server {}", self.own_id).as_str(), "Received channel request");
                    replies.extend_from_slice(self.generate_channel_updates().as_slice());
                }
                MessageKind::CliJoin(data) => self.msg_clijoin(&mut replies, data, &cli_node_id),
                MessageKind::CliLeave(..) => self.msg_clileave(&mut replies, &cli_node_id),
                MessageKind::SendMsg(msg) => self.msg_sendmsg(&mut replies, &cli_node_id, msg),
                MessageKind::Err(e) => {
                    error!(target: format!("Server {}", self.own_id).as_str(), "Received error message: {e:?}")
                }
                MessageKind::DsvReq(..) => {
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
        trace!(target: format!("Server {}", self.own_id).as_str(), "Current state: {self:?}");
        info!(target: format!("Server {}", self.own_id).as_str(), "Sending back replies: {replies:?}");
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
        ChatServerInternal {
            own_id: id,
            channels,
            channel_info,
            usernames: BiHashMap::default(),
        }
    }
}

pub type ChatServer = PacketHandler<ServerCommand, ServerEvent, ChatServerInternal>;

impl ChatServerInternal {
    fn generate_channel_updates(&mut self) -> Vec<(NodeId, ChatMessage)> {
        let mut updates = vec![];
        let mut channel_list = vec![];
        for (id, name) in &self.channels {
            trace!(target: format!("Server {}", self.own_id).as_str(), "Adding {name}({id}) to channel list for generation");
            if let Some((is_group, clients)) = self.channel_info.get(id) {
                let mut clients_res = vec![];
                for x in clients {
                    trace!(target: format!("Server {}", self.own_id).as_str(), "Adding client {x} to channel members for generation:");
                    if let Some(name) = self.usernames.get_by_left(x) {
                        trace!(target: format!("Server {}", self.own_id).as_str(), "Client {x} has username {name}");
                        clients_res.push(ClientData {
                            username: name.clone(),
                            id: u64::from(*x),
                        });
                    } else {
                        error!(target: format!("Server {}", self.own_id).as_str(), "Client {x} doesn't have a username");
                    }
                }
                channel_list.push(Channel {
                    channel_name: name.clone(),
                    channel_id: *id,
                    channel_is_group: *is_group,
                    connected_clients: clients_res,
                });
            } else {
                error!(target: format!("Server {}", self.own_id).as_str(), "Channel {name}({id}) doesn't have info");
            }
        }
        debug!(target: format!("Server {}", self.own_id).as_str(), "Generated channel list: {channel_list:?}");
        for id in self.usernames.left_values() {
            trace!(target: format!("Server {}", self.own_id).as_str(), "Adding client {id} to channel updates");
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
        debug!(target: format!("Server {}", self.own_id).as_str(), "Generated channel updates: {updates:?}");
        updates
    }
}
