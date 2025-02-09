use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{Channel, ChatMessage, Empty, ErrorMessage, JoinChannel};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ChatClientCommand, ChatClientEvent, ServerType};
use crossbeam::channel::Sender;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use log::{info, trace};
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

pub struct ChatClientInternal {
    discovered_servers: Vec<(NodeId, String)>,
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
        info!(target: format!("Node {}", self.own_id).as_str(), "Received message: {:?}", message);
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
                MessageKind::Err(err) => {
                    events.push(ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Error: {} - {}",
                        err.error_type, err.error_message
                    )));
                }
                MessageKind::DsvRes(res) => {
                    #[allow(clippy::cast_possible_truncation)]
                    self.discovered_servers
                        .push((res.server_id as NodeId, res.server_type));
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
            discovered_servers: vec![],
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
    fn handle_message(
        &mut self,
        message: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        info!(target: format!("Node {}", self.own_id).as_str(), "Handling text message: {:?}", message);
        if message.starts_with('/') {
            let msg = message.chars().skip(1).collect::<String>();
            let (cmd, remainder) = msg.split_once(' ').unwrap_or((msg.as_str(), ""));
            trace!(target: format!("Node {}", self.own_id).as_str(), "First split: {cmd}, {remainder}");
            let (arg, freeform) = remainder.split_once(' ').unwrap_or((remainder, ""));
            trace!(target: format!("Node {}", self.own_id).as_str(), "First split: {arg}, {remainder}");
            return self.handle_command(cmd, arg, freeform);
        }
        match (self.currently_connected_server, self.currently_connected_channel) {
            (Some(connected_server), Some(connected_channel)) => {
                if self.server_usernames.contains_key(&connected_server) {
                    (
                    vec![(
                        connected_server,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::SendMsg(
                                chat_common::messages::SendMessage {
                                    message: message.to_string(),
                                    channel_id: connected_channel,
                                },
                            )),
                        },
                    )],
                    vec![],
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
        info!(target: format!("Node {}", self.own_id).as_str(), "Handling text command: [{} - {} - {}]", command, arg, freeform);
        match command {
            "help" => (
                vec![],
                vec![ChatClientEvent::MessageReceived(HELP_MESSAGE.to_string())],
            ),
            "servers" => {
                let servers_list = self
                    .discovered_servers
                    .iter()
                    .filter(|(_, x)| x.as_str() == "chat")
                    .map(|(id, _)| id.to_string())
                    .join(", ");
                (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Available servers: {servers_list}"
                    ))],
                )
            }
            "connect" => {
                self.channels_list.clear();
                self.currently_connected_server = None;
                self.currently_connected_channel = None;
                match self
                    .discovered_servers
                    .iter()
                    .find(|(id, typ)| typ == "chat" && id.to_string() == arg)
                {
                    Some((id, _)) => {
                        self.currently_connected_server = Some(*id);
                        self.currently_connected_channel = None;
                        (
                            vec![(
                                *id,
                                ChatMessage {
                                    own_id: u32::from(self.own_id),
                                    message_kind: Some(MessageKind::CliRequestChannels(Empty {})),
                                },
                            )],
                            vec![ChatClientEvent::MessageReceived(format!(
                                "[SYSTEM] Connecting to server {id}"
                            ))],
                        )
                    }
                    None => (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Error: Server not found".to_string(),
                        )],
                    ),
                }
            }
            "register" => {
                if arg.contains(' ') || arg.contains('#') || arg.contains('@') {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Error: Username cannot contain spaces, '#' or '@'"
                                .to_string(),
                        )],
                    )
                } else if let Some(server_id) = self.currently_connected_server {
                    match self.server_usernames.get(&server_id) {
                        Some(prev) => (
                            vec![],
                            vec![ChatClientEvent::MessageReceived(format!(
                                "[SYSTEM] Error: Already registered with username {prev}"
                            ))],
                        ),
                        None => (
                            vec![(
                                server_id,
                                ChatMessage {
                                    own_id: u32::from(self.own_id),
                                    message_kind: Some(MessageKind::CliRegisterRequest(
                                        arg.to_string(),
                                    )),
                                },
                            )],
                            vec![ChatClientEvent::MessageReceived(format!(
                                "[SYSTEM] Registering with username {arg}"
                            ))],
                        ),
                    }
                } else {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Error: Not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server before registering.".to_string(),
                        )],
                    )
                }
            }
            "unregister" => {
                if let Some(server_id) = self.currently_connected_server {
                    match self.server_usernames.get(&server_id) {
                        Some(_) => (
                            vec![(
                                server_id,
                                ChatMessage {
                                    own_id: u32::from(self.own_id),
                                    message_kind: Some(MessageKind::CliCancelReg(Empty {})),
                                },
                            )],
                            vec![ChatClientEvent::MessageReceived(
                                "[SYSTEM] Removing registration...".to_string(),
                            )],
                        ),
                        None => (
                            vec![],
                            vec![ChatClientEvent::MessageReceived(
                                "[SYSTEM] Not registered to this server!".to_string(),
                            )],
                        ),
                    }
                } else {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Error: Not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server before registering.".to_string(),
                        )],
                    )
                }
            }
            "channels" => {
                if self.currently_connected_server.is_some() {
                    let chan_list = self
                        .channels_list
                        .iter()
                        .filter(|x| x.channel_is_group && x.channel_id != 0x1)
                        .map(|x| format!("#{}", x.channel_name))
                        .join(",");
                    let user_list = self
                        .channels_list
                        .iter()
                        .find(|x| x.channel_id == 0x1)
                        .map_or(String::new(), |x| {
                            x.connected_clients
                                .iter()
                                .map(|x| format!("@{}", x.username))
                                .join(",")
                        });
                    let msg = format!(
                        "[SYSTEM] Available channels: {chan_list}\n[SYSTEM] Available IMs: {user_list}"
                    );
                    (vec![], vec![ChatClientEvent::MessageReceived(msg)])
                } else {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Error: Not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server before registering.".to_string(),
                        )],
                        )
                }
            }
            "join" => {
                self.currently_connected_channel = None;
                if arg.contains('#') || arg.contains('@') || arg.contains(' ') {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Error: Channel name cannot contain spaces, '#' or '@'"
                                .to_string(),
                        )],
                    )
                } else {
                    match (
                    self.currently_connected_server,
                    self.channels_list.iter().find(|x| arg == x.channel_name),
                ) {
                    (Some(server_id), Some(channel)) =>
                        (
                            vec![(server_id, ChatMessage{
                                own_id: u32::from(self.own_id),
                                message_kind: Some(MessageKind::CliJoin(
                                    JoinChannel {
                                        channel_id: Some(channel.channel_id),
                                        channel_name: String::new(),
                                    }
                                )
                            )})
                            ],
                            vec![ChatClientEvent::MessageReceived(
                                "[SYSTEM] Joining channel...".to_string(),
                            )],
                        ),
                    (Some(server_id), None) => (
                            vec![(server_id, ChatMessage{
                                own_id: u32::from(self.own_id),
                                message_kind: Some(MessageKind::CliJoin(
                                    JoinChannel {
                                        channel_id: None,
                                        channel_name: arg.to_string(),
                                    }
                                )
                            )})
                            ],
                            vec![ChatClientEvent::MessageReceived(
                                "[SYSTEM] Creating channel...".to_string(),
                            )],
                        ),

                    (None, _) => (vec![], vec![ChatClientEvent::MessageReceived( "[SYSTEM] You are not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server.".to_string(), )],)
                }
                }
            }
            "leave" => {
                match (
                    self.currently_connected_server,
                    self.currently_connected_channel,
                ) {
                    (Some(server_id), Some(_)) =>
                        (
                            vec![(server_id, ChatMessage{
                                own_id: self.own_id.into(),
                                message_kind: Some(MessageKind::CliLeave(Empty {})
                            )})
                            ],
                            vec![ChatClientEvent::MessageReceived(
                                "[SYSTEM] Leaving channel...".to_string(),
                            )],
                        ),
                    (Some(_), None) => (
                            vec![],
                            vec![ChatClientEvent::MessageReceived(
                                "[SYSTEM] Error: You are not connected to a channel.".to_string(),
                            )],
                        ),

                    (None, _) => (vec![], vec![ChatClientEvent::MessageReceived( "[SYSTEM] You are not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server.".to_string(), )],)
                }
            }
            "msg" => {
                if let Some(server_id) = self.currently_connected_server {
                    if self.server_usernames.contains_key(&server_id) {
                        let all_channel = self.channels_list.iter().find(|x| x.channel_id == 0x1);
                        if let Some(all) = all_channel {
                            if let Some(dst_id) =
                                all.connected_clients.iter().find(|x| x.username == arg)
                            {
                                (
                                    vec![(
                                        server_id,
                                        ChatMessage {
                                            own_id: u32::from(self.own_id),
                                            message_kind: Some(MessageKind::SendMsg(
                                                chat_common::messages::SendMessage {
                                                    message: freeform.to_string(),
                                                    channel_id: dst_id.id,
                                                },
                                            )),
                                        },
                                    )],
                                    vec![],
                                )
                            } else {
                                (
                                    vec![],
                                    vec![ChatClientEvent::MessageReceived(
                                        "[SYSTEM] Error: User not found".to_string(),
                                    )],
                                )
                            }
                        } else {
                            (
                                vec![],
                                vec![ChatClientEvent::MessageReceived(
                                    "[SYSTEM] Error: No 'all' channel found".to_string(),
                                )],
                            )
                        }
                    } else {
                        (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] Please set your username with /register <username> and try /msg-ing again.".to_string(),
                        )],
                        )
                    }
                } else {
                    (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] You are not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server.".to_string(),
                    )],
                    )
                }
            }
            _ => (
                vec![],
                vec![ChatClientEvent::MessageReceived(format!(
                    "[SYSTEM] Unknown command {command}. Use /help to list available commands."
                ))],
            ),
        }
    }
}

pub type ChatClient = PacketHandler<ChatClientCommand, ChatClientEvent, ChatClientInternal>;

const HELP_MESSAGE: &str = r"
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
";
