use std::collections::HashMap;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{ChatMessage, ErrorMessage};
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ServerCommand, ServerEvent};
use crossbeam::channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};

pub struct ChatServerInternal {
    own_id: NodeId
}

impl CommandHandler<ServerCommand, ServerEvent> for ChatServerInternal {
    fn get_node_type() -> NodeType {
        NodeType::Server
    }

    fn handle_protocol_message(&mut self, message: ChatMessage) -> (Vec<(NodeId, ChatMessage)>, Vec<ServerEvent>)
    where
        Self: Sized
    {
        let mut replies: Vec<(NodeId, ChatMessage)> = vec![];
        // TODO
        if let Some(kind) = message.message_kind {
            match kind {
                MessageKind::CliRegisterRequest(_) => {}
                MessageKind::CliRequestChannels(_) => {}
                MessageKind::CliJoin(_) => {}
                MessageKind::CliLeave(_) => {}
                MessageKind::SendMsg(_) => {}
                MessageKind::Err(_) => {}
                MessageKind::DsvReq(_) => {}
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

    fn report_sent_packet(&mut self, packet: Packet) -> ServerEvent
    where
        Self: Sized
    {
        ServerEvent::PacketSent(packet)
    }

    fn handle_controller_command(&mut self, sender_hash: &mut HashMap<NodeId, Sender<Packet>>, command: ServerCommand) -> (Option<Packet>, Vec<(NodeId, ChatMessage)>, Vec<ServerEvent>)
    where
        Self: Sized
    {
        match command{
            ServerCommand::AddSender(id, sender) => {sender_hash.insert(id, sender); (None, vec![], vec![])},
            ServerCommand::RemoveSender(id) => {sender_hash.remove(&id); (None, vec![], vec![])}
            ServerCommand::Shortcut(p) => {(Some(p), vec![], vec![])}
        }
    }

    fn add_node(&mut self, _id: NodeId, _typ: NodeType) -> Option<(NodeId, ChatMessage)> {
        None
    }

    fn new(id: NodeId) -> Self
    where
        Self: Sized
    {
        ChatServerInternal {
            own_id: id
        }
    }
}

pub type ChatServer = PacketHandler<ServerCommand, ServerEvent, ChatServerInternal>;