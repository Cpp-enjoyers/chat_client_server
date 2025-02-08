use std::collections::{HashMap, HashSet};
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{ChatMessage, DiscoveryRequest, ErrorMessage};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ChatClientCommand, ChatClientEvent, ServerType};
use crossbeam::channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

pub struct ChatClientInternal {
    discovered_servers: Vec<(NodeId,String)>,
    discovered_nodes: HashSet<NodeId>,
    currently_connected_server: Option<NodeId>,
    currently_connected_channel: Option<u64>,
    server_usernames: HashMap<NodeId, String>,
    channels_list: Vec<(String, u64)>,
    own_id: u8
}
impl CommandHandler<ChatClientCommand, ChatClientEvent> for ChatClientInternal {
    fn get_node_type() -> NodeType {
        NodeType::Client
    }

    fn handle_protocol_message(&mut self, message: ChatMessage) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>)
    where
        Self: Sized
    { 
        // TODO
        let mut replies: Vec<(NodeId, ChatMessage)> = vec![];
        if let Some(kind) = message.message_kind {
            match kind {
                MessageKind::SrvConfirmReg(_) => {}
                MessageKind::SrvReturnChannels(_) => {}
                MessageKind::SrvChannelData(_) => {}
                MessageKind::SrvDistributeMessage(_) => {}
                MessageKind::Err(_) => {}
                MessageKind::DsvRes(_) => {}
                _ => {
                    replies.push((message.own_id as NodeId, ChatMessage {
                        own_id: self.own_id as u32,
                        message_kind: Some(MessageKind::Err(ErrorMessage {
                            error_type: "INVALID_MESSAGE".to_string(),
                            error_message: format!("Invalid message: {:?}", kind)
                        }))
                    }));
                }
            }
        }
        (replies, vec![])
    }

    fn report_sent_packet(&mut self, packet: Packet) -> ChatClientEvent
    where
        Self: Sized
    {
        ChatClientEvent::PacketSent(packet)
    }

    fn handle_controller_command(&mut self, sender_hash: &mut HashMap<NodeId, Sender<Packet>>, command: ChatClientCommand) -> (Option<Packet>, Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>)
    where
        Self: Sized
    {
        match command{
            ChatClientCommand::AddSender(id, sender) => {sender_hash.insert(id, sender); (None, vec![], vec![])},
            ChatClientCommand::RemoveSender(id) => {sender_hash.remove(&id); (None, vec![], vec![])}
            ChatClientCommand::Shortcut(p) => {(Some(p), vec![], vec![])}
            ChatClientCommand::AskServersTypes => {
                let mut map = HashMap::new();
                self.discovered_servers.iter().for_each(|(id, srv_type)|{
                    if srv_type == "chat" {map.insert(*id, ServerType::ChatServer);}
                });
                (None,vec![],vec![ChatClientEvent::ServersTypes(map)])
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
            Some((id, ChatMessage{own_id: self.own_id as u32, message_kind: Some(MessageKind::DsvReq(DiscoveryRequest{server_type_requested: "chat".to_string()}))}))
        }
    }

    fn new(id: NodeId) -> Self
    where
        Self: Sized
    {
        ChatClientInternal {
            discovered_servers: vec![],
            discovered_nodes: Default::default(),
            currently_connected_server: None,
            currently_connected_channel: None,
            server_usernames: Default::default(),
            channels_list: vec![],
            own_id: id,
        }
    }
}

impl ChatClientInternal {
    fn handle_message(&mut self, message: String) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        if message.starts_with('/') {
            let (cmd,remainder) = message.split_once(' ').unwrap_or(("", ""));
            let (arg, freeform) = remainder.split_once(' ').unwrap_or(("", ""));
            self.handle_command(cmd, arg, freeform);
        }

        // TODO: Implement regular message sending
        (vec![], vec![])
    }
    fn handle_command(&mut self, command: &str, arg: &str, freeform: &str) ->  (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        // TODO: Implement commands
        match command {
            "help" => {
                return (vec![], vec![ChatClientEvent::MessageReceived(HELP_MESSAGE.to_string())]);
            }
            "servers" => {}
            "connect" => {}
            "register" => {}
            "channels" => {}
            "join" => {}
            "leave" => {}
            "msg" => {}
            _ => {}
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
[SYSTEM]    /channels - List all channels available on the server.
[SYSTEM]    /join <channel> - Join a channel. You can only be in one channel at a time.
[SYSTEM]    /leave <channel> - Leave the current channel. You will still receive DMs and system communications.
[SYSTEM]    /msg <user> <text> - Send a direct message to a user.
"#;