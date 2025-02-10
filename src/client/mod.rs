mod client_command_handling;
mod client_message_handling;

use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{Channel, ChatMessage, ErrorMessage, MessageData};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ChatClientCommand, ChatClientEvent, ServerType};
use crossbeam::channel::Sender;
use log::info;
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

#[derive(Debug)]
pub struct ChatClientInternal {
    discovered_servers: HashMap<NodeId, String>,
    discovered_nodes: HashSet<NodeId>,
    currently_connected_server: Option<NodeId>,
    currently_connected_channel: Option<u64>,
    server_usernames: HashMap<NodeId, String>,
    channels_list: Vec<Channel>, // bool is for "is_group_channel"
    own_id: u8,
    // Client ID is the NodeId shifted left by 32 bits, with the last 4 bits set to 0x8
    // Channels will be random, with the last 4 bits as 0x2
    // The special "all" channel has only the last 4 bits as 0x1
    own_channel_id: u64,
}
impl CommandHandler<ChatClientCommand, ChatClientEvent> for ChatClientInternal {
    fn get_node_type() -> NodeType {
        NodeType::Client
    }

    fn handle_protocol_message(
        &mut self,
        message: ChatMessage,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>)
    where
        Self: Sized,
    {
        let mut replies: Vec<(NodeId, ChatMessage)> = vec![];
        let mut events: Vec<ChatClientEvent> = vec![];
        info!(target: format!("Client {}", self.own_id).as_str(), "Received message: {:?}", message);
        if let Some(kind) = message.message_kind {
            match kind {
                MessageKind::SrvConfirmReg(reg) => {
                    match (self.currently_connected_server, reg.successful) {
                        (Some(server_id), true) if message.own_id == u32::from(server_id) => {
                            self.server_usernames.insert(server_id, reg.username);
                        }
                        (Some(_), true) => {
                            events.push(ChatClientEvent::MessageReceived("[SYSTEM] Error: Received registration confirmation from another server".to_string()));
                        }
                        (Some(_), false) => {
                            events.push(ChatClientEvent::MessageReceived(format!(
                                "[SYSTEM] Error: Registration failed - {}",
                                reg.error.unwrap_or("Unknown error".to_string())
                            )));
                        }
                        (None, _) => {
                            events.push(ChatClientEvent::MessageReceived(format!(
                                "[SYSTEM] Error: Registration failed, not connected to server - {}",
                                reg.error.unwrap_or("Unknown error".to_string())
                            )));
                        }
                    }
                }
                MessageKind::SrvReturnChannels(channels) => match self.currently_connected_server {
                    Some(server_id) if message.own_id == u32::from(server_id) => {
                        self.channels_list = channels.channels;
                    }
                    Some(_) => {
                        // Ignore for other servers
                    }
                    None => {
                        events.push(ChatClientEvent::MessageReceived("[SYSTEM] Error: Received channel list without being connected to a server".to_string()));
                    }
                },
                MessageKind::SrvDistributeMessage(msg) => {
                    self.msg_srvdistributemessage(&mut events, msg);
                }
                MessageKind::Err(err) => {
                    events.push(ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Error: {} - {}",
                        err.error_type, err.error_message
                    )));
                }
                MessageKind::DsvRes(res) => {
                    #[allow(clippy::cast_possible_truncation)]
                    self.discovered_servers
                        .insert(res.server_id as NodeId, res.server_type);
                }
                MessageKind::SrvChannelCreationSuccessful(chan) => {
                    self.currently_connected_channel = Some(chan);
                }
                _ => {
                    #[allow(clippy::cast_possible_truncation)]
                    replies.push((
                        message.own_id as NodeId,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::Err(ErrorMessage {
                                error_type: "INVALID_SRV_MESSAGE".to_string(),
                                error_message: format!("Invalid message: {kind:?}"),
                            })),
                        },
                    ));
                }
            }
        }
        (replies, events)
    }

    fn report_sent_packet(&mut self, packet: Packet) -> ChatClientEvent
    where
        Self: Sized,
    {
        ChatClientEvent::PacketSent(packet)
    }

    fn handle_controller_command(
        &mut self,
        sender_hash: &mut HashMap<NodeId, Sender<Packet>>,
        command: ChatClientCommand,
    ) -> (
        Option<Packet>,
        Vec<(NodeId, ChatMessage)>,
        Vec<ChatClientEvent>,
    )
    where
        Self: Sized,
    {
        match command {
            ChatClientCommand::AddSender(id, sender) => {
                sender_hash.insert(id, sender);
                (None, vec![], vec![])
            }
            ChatClientCommand::RemoveSender(id) => {
                sender_hash.remove(&id);
                (None, vec![], vec![])
            }
            ChatClientCommand::Shortcut(p) => (Some(p), vec![], vec![]),
            ChatClientCommand::AskServersTypes => {
                let mut map = HashMap::new();
                self.discovered_servers.iter().for_each(|(id, srv_type)| {
                    if srv_type == "chat" {
                        map.insert(*id, ServerType::ChatServer);
                    }
                });
                (None, vec![], vec![ChatClientEvent::ServersTypes(map)])
            }
            ChatClientCommand::SendMessage(m) => {
                let x = self.handle_message(m.as_str());
                (None, x.0, x.1)
            }
        }
    }

    fn add_node(&mut self, id: NodeId, typ: NodeType) -> Option<(NodeId, ChatMessage)> {
        if self.discovered_nodes.contains(&id) || typ != NodeType::Server {
            None
        } else {
            self.discovered_nodes.insert(id);
            Some((
                id,
                ChatMessage {
                    own_id: u32::from(self.own_id),
                    message_kind: Some(MessageKind::DsvReq("chat".to_string())),
                },
            ))
        }
    }

    fn new(id: NodeId) -> Self
    where
        Self: Sized,
    {
        ChatClientInternal {
            discovered_servers: HashMap::default(),
            discovered_nodes: HashSet::default(),
            currently_connected_server: None,
            currently_connected_channel: None,
            server_usernames: HashMap::default(),
            channels_list: vec![],
            own_id: id,
            own_channel_id: u64::from(id) << 32 | 0x8,
        }
    }
}

impl ChatClientInternal {
    fn msg_srvdistributemessage(&mut self, events: &mut Vec<ChatClientEvent>, msg: MessageData) {
        if msg.channel_id == self.own_channel_id
            && self.currently_connected_channel == Some(self.own_channel_id)
        {
            events.push(ChatClientEvent::MessageReceived(format!(
                "[@{}] {}",
                msg.username, msg.message
            )));
        } else {
            match self
                .channels_list
                .iter()
                .find(|chan| chan.channel_id == msg.channel_id)
            {
                Some(chan) => {
                    if chan.channel_is_group {
                        events.push(ChatClientEvent::MessageReceived(format!(
                            "[#{} @{}] {}",
                            chan.channel_name, msg.username, msg.message
                        )));
                    } else {
                        events.push(ChatClientEvent::MessageReceived(format!(
                            "[IM @{}] {}",
                            msg.username, msg.message
                        )));
                    }
                }
                None => {
                    events.push(ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Error: Received message from unknown channel\n[#{} @{}] {}",
                        msg.channel_id, msg.username, msg.message
                    )));
                }
            }
        }
    }
}

pub type ChatClient = PacketHandler<ChatClientCommand, ChatClientEvent, ChatClientInternal>;
