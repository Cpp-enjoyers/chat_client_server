use itertools::Itertools;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{ChatMessage, DiscoveryRequest, ErrorMessage};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ChatClientCommand, ChatClientEvent, ServerType};
use crossbeam::channel::Sender;
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

pub struct ChatClientInternal {
    discovered_servers: Vec<(NodeId, String)>,
    discovered_nodes: HashSet<NodeId>,
    currently_connected_server: Option<NodeId>,
    currently_connected_channel: Option<u64>,
    server_usernames: HashMap<NodeId, String>,
    channels_list: Vec<(String, u64, bool)>, // bool is for "is_group_channel"
    own_id: u8,
    // Client ID is the NodeId shifted left by 32 bits, with the last 4 bits set to 0x8
    // Servers will be the same except the last 4 bits are 0x4
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
        if let Some(kind) = message.message_kind {
            match kind {
                MessageKind::SrvConfirmReg(reg) => {
                    match (self.currently_connected_server, reg.successful) {
                        (Some(server_id), true) if message.own_id == server_id as u32 => {
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
                MessageKind::SrvReturnChannels(channels) => {
                    match self.currently_connected_server {
                        Some(server_id) if message.own_id == server_id as u32 => {
                            self.channels_list = channels
                                .channels
                                .iter()
                                .map(|channel| {
                                    (
                                        channel.channel_name.clone(),
                                        channel.channel_id,
                                        channel.channel_is_group,
                                    )
                                })
                                .collect();
                        }
                        Some(_) => {
                            events.push(ChatClientEvent::MessageReceived("[SYSTEM] Error: Received channel list from another server".to_string()));
                        }
                        None => {
                            events.push(ChatClientEvent::MessageReceived("[SYSTEM] Error: Received channel list without being connected to a server".to_string()));
                        }

                    }
                }
                MessageKind::SrvDistributeMessage(msg) => {
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
                            .find(|(_, id, _)| id == &msg.channel_id)
                        {
                            Some((name, _, is_group_channel)) => {
                                if *is_group_channel {
                                    events.push(ChatClientEvent::MessageReceived(format!(
                                        "[#{} @{}] {}",
                                        name, msg.username, msg.message
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
                MessageKind::Err(err) => {
                    events.push(ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Error: {} - {}",
                        err.error_type, err.error_message
                    )));
                }
                MessageKind::DsvRes(res) => {
                    self.discovered_servers
                        .push((res.server_id as NodeId, res.server_type));
                }
                _ => {
                    replies.push((
                        message.own_id as NodeId,
                        ChatMessage {
                            own_id: self.own_id as u32,
                            message_kind: Some(MessageKind::Err(ErrorMessage {
                                error_type: "INVALID_SRV_MESSAGE".to_string(),
                                error_message: format!("Invalid message: {:?}", kind),
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
                let x = self.handle_message(m);
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
                    own_id: self.own_id as u32,
                    message_kind: Some(MessageKind::DsvReq(DiscoveryRequest {
                        server_type_requested: "chat".to_string(),
                    })),
                },
            ))
        }
    }

    fn new(id: NodeId) -> Self
    where
        Self: Sized,
    {
        ChatClientInternal {
            discovered_servers: vec![],
            discovered_nodes: Default::default(),
            currently_connected_server: None,
            currently_connected_channel: None,
            server_usernames: Default::default(),
            channels_list: vec![],
            own_id: id,
            own_channel_id: (id as u64) << 32 | 0x8,
        }
    }
}

impl ChatClientInternal {
    fn handle_message(
        &mut self,
        message: String,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        if message.starts_with('/') {
            let (cmd, remainder) = message.split_once(' ').unwrap_or(("", ""));
            let (arg, freeform) = remainder.split_once(' ').unwrap_or(("", ""));
            return self.handle_command(cmd, arg, freeform);
        }
        match (self.currently_connected_server, self.currently_connected_channel) {
            (Some(connected_server), Some(connected_channel)) => {
                if let Some(username) = self.server_usernames.get(&connected_server) {
                    (
                    vec![(
                        connected_server,
                        ChatMessage {
                            own_id: self.own_id as u32,
                            message_kind: Some(MessageKind::SendMsg(
                                chat_common::messages::SendMessage {
                                    message: message.to_string(),
                                    channel_id: connected_channel,
                                },
                            )),
                        },
                    )],
                    vec![ChatClientEvent::MessageReceived(format!("[@{}] {}", username, message))],
                )
                } else {
                    (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] Please set your username with /register <username> and try /join-ing again.".to_string(),
                    )],
                    )
                }
            }
            (Some(_), None) => {
                (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] You are not in a channel. Use /channels to see available channels and /join <channel_id> to join one.".to_string(),
                    )],
                )
            }
            (None, _) => {
                (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] You are not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server.".to_string(),
                    )],
                )
            }
        }
    }
    fn handle_command(
        &mut self,
        command: &str,
        arg: &str,
        freeform: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        // TODO: Implement commands
        match command {
            "help" => {
                return (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(HELP_MESSAGE.to_string())],
                );
            }
            "servers" => {
                let servers_list = self.discovered_servers.iter().filter(|(_,x)| x.as_str() == "chat").map(|(id, _)| id.to_string()).join(", ");
                return (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        format!("[SYSTEM] Available servers: {}", servers_list)
                    )],
                );
            }
            "connect" => {}
            "register" => {}
            "unregister" => {}
            "channels" => {}
            "join" => {}
            "leave" => {}
            "msg" => {}
            _ => {
                return (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        format!("[SYSTEM] Unknown command {}. Use /help to list available commands.", command),
                    )],
                );
            }
        }
        (vec![], vec![])
    }
}

pub type ChatClient = PacketHandler<ChatClientCommand, ChatClientEvent, ChatClientInternal>;

const HELP_MESSAGE: &str = r#"
[SYSTEM] Commands:
[SYSTEM]    /help - Display this message
[SYSTEM]    /servers - Lists discovered servers
[SYSTEM]    /connect <server_id> - Connect to a server
[SYSTEM]    /register <username> - Register with a server. Username cannot contain spaces or '#' and '@'.
[SYSTEM]    /unregister - Unregister from the current server.
[SYSTEM]    /channels - List all channels available on the server.
[SYSTEM]    /join <channel> - Join a channel. You can only be in one channel at a time.
[SYSTEM]    /leave <channel> - Leave the current channel. You will still receive DMs and system communications.
[SYSTEM]    /msg <user> <text> - Send a direct message to a user.
"#;
